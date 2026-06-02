//! `synthesist import` -- replay a `cmd_export` dump into the current store.
//!
//! Path B Stage 2: v3-native import. Reads the export shape (stdin or
//! `--file <path>`) and replays each `claims_raw` entry via
//! `SynthStore::append_replay`.
//!
//! ## Re-mint semantics
//!
//! v3 claim ids are content hashes over
//! `(claim_type, props, asserter, generated_at)`. `append_replay`
//! samples a fresh wall clock for `generated_at`, so **the import
//! re-mints every @id**. Logical content (props, supersession chain
//! topology) is preserved; the @id strings change.
//!
//! Concretely, the extractor here pulls (claim_type, props, supersedes)
//! out of each raw v3 doc and drops the envelope (`@context`, `@id`,
//! `@type`, `prov:generatedAtTime`, `prov:wasAttributedTo`,
//! `nomograph:parentAsserter`, `synthesist:supersedes`). The supersedes
//! ref is preserved (as the bare short hash) so the import re-links the
//! chain by walking entries in log order -- but only if the export
//! ordered later supersession steps after earlier ones (which
//! `iter_claims` does for any single asserter dir; across asserters it
//! ranks dirs lexicographically, which is a weaker guarantee). For
//! typical exports of a single estate this works.
//!
//! Stable-id round-trip is a 3.0.0-final concern: it would require a
//! SynthStore raw-write helper that writes a JSON-LD doc verbatim
//! (preserving the exporter's @id, asserter, and timestamps). That
//! helper would let the import path bypass `append_inner`'s id
//! computation entirely. Not in scope for pre.1.

use std::fs;
use std::io::{self, Read};

use anyhow::{Context, Result, anyhow, bail};
use crate::claim_type::ClaimType;
use serde_json::{Map, Value, json};

use crate::store::{ClaimId, SynthStore, json_out};
use crate::wire_format::{self, MODULE_PREFIX};

pub fn cmd_import(file: &Option<String>) -> Result<()> {
    let raw = read_input(file.as_deref())?;
    let doc: Value = serde_json::from_str(&raw).context("parse import JSON")?;

    let claims_raw = doc
        .get("claims_raw")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!(
            "import payload missing `claims_raw` array; \
             expected the output shape of `synthesist export`"
        ))?;

    let mut imported = 0usize;
    let mut skipped = 0usize;

    for entry in claims_raw {
        match extract_replay_args(entry) {
            Ok((claim_type, props, supersedes, asserter)) => {
                let mut store = SynthStore::discover()?.with_asserter(asserter);
                match store.append_replay(claim_type, props, supersedes) {
                    Ok(_) => imported += 1,
                    Err(_) => skipped += 1,
                }
            }
            Err(_) => {
                skipped += 1;
            }
        }
    }

    json_out(&json!({ "imported": imported, "skipped": skipped }))
}

fn read_input(file: Option<&str>) -> Result<String> {
    match file {
        Some(p) if !p.is_empty() => fs::read_to_string(p)
            .with_context(|| format!("read import file {p}")),
        _ => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("read import payload from stdin")?;
            Ok(buf)
        }
    }
}

/// Convert a raw v3 JSON-LD doc into (claim_type, props, supersedes,
/// asserter) suitable for `SynthStore::append_replay`.
///
/// The extractor:
/// 1. Reads `@type` -> snake_case claim type (`synthesist:Spec` -> `Spec`).
/// 2. Reads `prov:wasAttributedTo` -> asserter (`asserter:user:...` -> `user:...`).
/// 3. Reads `synthesist:supersedes` -> bare short hash, if present.
/// 4. Drops envelope keys (`@context`, `@id`, `@type`, `prov:*`,
///    `nomograph:parentAsserter`, `synthesist:supersedes`).
/// 5. Reverses `lowerCamelCase` predicate names back to `snake_case`
///    props keys (`synthesist:dependsOn` -> `depends_on`).
///
/// Returns `Err` if the doc is missing required envelope fields or has
/// an `@type` that doesn't decode to a known `ClaimType`.
fn extract_replay_args(
    doc: &Value,
) -> Result<(ClaimType, Value, Option<ClaimId>, String)> {
    let obj = doc
        .as_object()
        .ok_or_else(|| anyhow!("claims_raw entry is not a JSON object"))?;

    let type_str = obj
        .get("@type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("claims_raw entry missing @type"))?;
    let claim_type = decode_type_iri(type_str)?;

    let asserter_iri = obj
        .get(wire_format::ATTRIBUTED_TO_PRED)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("claims_raw entry missing prov:wasAttributedTo"))?;
    let asserter = asserter_iri
        .strip_prefix("asserter:")
        .unwrap_or(asserter_iri)
        .to_string();

    let supersedes = obj
        .get(wire_format::SUPERSEDES_PRED)
        .and_then(|v| v.as_str())
        .map(|s| {
            s.strip_prefix("https://nomograph.org/synthesist/claim/")
                .or_else(|| s.strip_prefix("synthesist:claim/"))
                .unwrap_or(s)
                .to_string()
        });

    let mut props = Map::new();
    let synth_prefix = format!("{MODULE_PREFIX}:");
    for (k, v) in obj {
        // Envelope keys: drop.
        if k.starts_with('@') {
            continue;
        }
        if k.starts_with("prov:") {
            continue;
        }
        if k == wire_format::PARENT_ASSERTER_PRED {
            continue;
        }
        if k == wire_format::SUPERSEDES_PRED {
            continue;
        }

        // synthesist:<lowerCamel> -> <snake_case>.
        let snake = if let Some(rest) = k.strip_prefix(&synth_prefix) {
            lower_camel_to_snake(rest)
        } else {
            // Unknown namespace -- keep the key verbatim. Schema
            // validation in `append_replay` is bypassed, so this is
            // safe; downstream readers may simply ignore unknown
            // predicates.
            k.clone()
        };
        props.insert(snake, v.clone());
    }

    Ok((claim_type, Value::Object(props), supersedes, asserter))
}

/// Decode `synthesist:Spec` -> `ClaimType::Spec`.
///
/// Accepts both the compact form (`synthesist:Spec`) and the expanded
/// IRI (`https://nomograph.org/synthesist/Spec`) so docs that survived
/// a graph_view rebuild are still decodable.
fn decode_type_iri(s: &str) -> Result<ClaimType> {
    let local = s
        .strip_prefix("https://nomograph.org/synthesist/")
        .or_else(|| s.strip_prefix("synthesist:"))
        .ok_or_else(|| anyhow!("unrecognized @type IRI: {s}"))?;
    let snake = title_to_snake(local);
    claim_type_from_snake(&snake)
        .ok_or_else(|| anyhow!("unknown claim type: {snake} (from @type {s})"))
}

/// `TitleCase` -> `snake_case`. Inverse of `wire_format::camel_case`.
fn title_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `lowerCamelCase` -> `snake_case`. Inverse of `wire_format::lower_camel_case`.
fn lower_camel_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Reverse of `ClaimType::as_str()`. Maintained here (rather than added
/// to the substrate) to keep the substrate API surface stable; the
/// substrate offers `as_str` but no inverse, and the import path is the
/// only caller that needs the inverse today.
fn claim_type_from_snake(s: &str) -> Option<ClaimType> {
    Some(match s {
        "tree" => ClaimType::Tree,
        "spec" => ClaimType::Spec,
        "task" => ClaimType::Task,
        "discovery" => ClaimType::Discovery,
        "campaign" => ClaimType::Campaign,
        "session" => ClaimType::Session,
        "phase" => ClaimType::Phase,
        "intent" => ClaimType::Intent,
        "heartbeat" => ClaimType::Heartbeat,
        "outcome" => ClaimType::Outcome,
        "directive" => ClaimType::Directive,
        "stakeholder" => ClaimType::Stakeholder,
        "topic" => ClaimType::Topic,
        "signal" => ClaimType::Signal,
        "disposition" => ClaimType::Disposition,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_to_snake_handles_simple_and_compound() {
        assert_eq!(title_to_snake("Task"), "task");
        assert_eq!(title_to_snake("Spec"), "spec");
        assert_eq!(title_to_snake("AgreeSnapshot"), "agree_snapshot");
    }

    #[test]
    fn lower_camel_to_snake_handles_simple_and_compound() {
        assert_eq!(lower_camel_to_snake("id"), "id");
        assert_eq!(lower_camel_to_snake("dependsOn"), "depends_on");
        assert_eq!(lower_camel_to_snake("agreeSnapshot"), "agree_snapshot");
    }

    #[test]
    fn decode_type_iri_compact_form() {
        assert!(matches!(
            decode_type_iri("synthesist:Spec").unwrap(),
            ClaimType::Spec
        ));
        assert!(matches!(
            decode_type_iri("synthesist:Task").unwrap(),
            ClaimType::Task
        ));
    }

    #[test]
    fn decode_type_iri_expanded_form() {
        assert!(matches!(
            decode_type_iri("https://nomograph.org/synthesist/Phase").unwrap(),
            ClaimType::Phase
        ));
    }

    #[test]
    fn decode_type_iri_unknown_errors() {
        assert!(decode_type_iri("synthesist:Bogus").is_err());
        assert!(decode_type_iri("other:Thing").is_err());
    }

    #[test]
    fn extract_replay_args_strips_envelope_and_renames_predicates() {
        let doc = json!({
            "@context": {"synthesist": "https://nomograph.org/synthesist/"},
            "@id": "synthesist:claim/abcdef0123456789",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:supersedes": "synthesist:claim/1111222233334444",
            "synthesist:tree": "proj",
            "synthesist:spec": "s1",
            "synthesist:id": "t1",
            "synthesist:status": "pending",
            "synthesist:dependsOn": ["t0"]
        });
        let (ty, props, sup, asserter) = extract_replay_args(&doc).unwrap();
        assert!(matches!(ty, ClaimType::Task));
        assert_eq!(asserter, "user:local:agd");
        assert_eq!(sup.as_deref(), Some("1111222233334444"));
        let obj = props.as_object().unwrap();
        assert_eq!(obj.get("tree").and_then(|v| v.as_str()), Some("proj"));
        assert_eq!(obj.get("id").and_then(|v| v.as_str()), Some("t1"));
        assert!(obj.contains_key("depends_on"));
        assert!(!obj.contains_key("@id"));
        assert!(!obj.contains_key("prov:generatedAtTime"));
        assert!(!obj.contains_key("synthesist:supersedes"));
    }

    #[test]
    fn extract_replay_args_missing_type_errors() {
        let doc = json!({
            "@id": "synthesist:claim/abc",
            "prov:wasAttributedTo": "asserter:x",
        });
        assert!(extract_replay_args(&doc).is_err());
    }

    #[test]
    fn extract_replay_args_missing_asserter_errors() {
        let doc = json!({
            "@id": "synthesist:claim/abc",
            "@type": "synthesist:Task",
        });
        assert!(extract_replay_args(&doc).is_err());
    }
}

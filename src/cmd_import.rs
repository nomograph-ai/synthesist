//! `synthesist import` -- replay a `cmd_export` dump into the current store.
//!
//! Path B Stage 2: v3-native import. Reads the export shape (stdin or
//! `--file <path>`) and replays each `claims_raw` entry via
//! `SynthStore::append_replay`.
//!
//! ## Re-mint semantics and supersedes remap
//!
//! v3 claim ids are content hashes over
//! `(claim_type, props, asserter, generated_at)`. `append_replay`
//! samples a fresh wall clock for `generated_at`, so **the import
//! re-mints every @id**: the imported claim gets a brand new id that
//! has no relation to the exporter's.
//!
//! That re-mint is the source of a data-integrity hazard for
//! supersession chains. Each `synthesist:supersedes` ref in the export
//! points at an exporter-side @id. If the import wrote that ref
//! verbatim, the new claim would supersede an id that does not exist in
//! the freshly-imported log: the chain would break, every version of a
//! multi-step chain would go live (instead of a single head), and
//! `synthesist check` would report `dangling_supersedes`.
//!
//! To prevent that, the import **remaps** supersedes refs. It processes
//! `claims_raw` in log order (the order `cmd_export` emits, which places
//! an earlier supersession step before the later step that supersedes
//! it) and maintains an `old @id -> new @id` map:
//!
//! 1. For each entry, read its exporter @id (the old id) and its
//!    supersedes ref (an old id, if present).
//! 2. Rewrite the supersedes ref through the map to the new id of the
//!    claim it supersedes. Because that prior claim was written earlier
//!    in log order, its new id is already in the map.
//! 3. Write the claim via `append_replay`, obtaining the new id.
//! 4. Record `old @id -> new @id` so later steps can resolve refs to
//!    this claim.
//!
//! The result: a supersession chain that had one live head before
//! export has exactly one live head after import, and `check` reports
//! zero dangling supersedes. If a supersedes ref cannot be resolved
//! through the map (e.g. a partial export that omits the superseded
//! claim) the ref is passed through unremapped, which surfaces as a
//! dangling edge -- the honest signal that the export was incomplete.
//!
//! The extractor pulls (claim_type, props, supersedes, asserter, @id)
//! out of each raw v3 doc and drops the envelope (`@context`, `@id`,
//! `@type`, `prov:generatedAtTime`, `prov:wasAttributedTo`,
//! `nomograph:parentAsserter`, `synthesist:supersedes`).

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};

use crate::claim_type::ClaimType;
use anyhow::{Context, Result, anyhow};
use serde_json::{Map, Value, json};

use crate::store::{ClaimId, SynthStore, json_out};
use crate::wire_format::{self, MODULE_PREFIX};

pub fn cmd_import(file: &Option<String>) -> Result<()> {
    let raw = read_input(file.as_deref())?;
    let doc: Value = serde_json::from_str(&raw).context("parse import JSON")?;

    let claims_raw = doc
        .get("claims_raw")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            anyhow!(
                "import payload missing `claims_raw` array; \
             expected the output shape of `synthesist export`"
            )
        })?;

    let mut imported = 0usize;
    let mut skipped = 0usize;
    // Supersedes refs that pointed at a claim ALSO present in this export but
    // could not be resolved through the remap (i.e. a genuinely broken chain
    // we wrote as a dangling edge). Distinct from a legitimate partial-export
    // passthrough, where the superseded claim is simply absent. Surfaced in
    // the result so a chain break is never reported as a clean import.
    let mut dangling_supersedes = 0usize;

    // old exporter @id -> new re-minted id. Built incrementally as we
    // walk `claims_raw` in log order so a supersedes ref (which always
    // points at an earlier step) is resolvable by the time we reach the
    // step that carries it. See the module docstring for why this is the
    // load-bearing invariant that keeps a chain at one live head.
    let mut id_remap: HashMap<String, ClaimId> = HashMap::new();
    let mut store = SynthStore::discover()?;

    // Branch on export shape. A v2.5.2 export carries top-level
    // `"version":"2"` and FLAT SQL rows in `claims_raw` (keys: `id`,
    // `claim_type`, `props`, `asserted_by`, `supersedes`, ...) -- NOT v3
    // JSON-LD. The v3 extractor requires `@type` / `prov:wasAttributedTo`
    // and would skip every flat row.
    //
    // A top-level `"version":"2"` forces the whole payload to the v2
    // extractor. ABSENT that marker we discriminate PER ROW (not once off
    // the first row): a row is v2-flat when it has `claim_type` and lacks
    // `@type`, else it routes to the v3 extractor. Per-row routing removes a
    // first-row dependency that would otherwise misroute an entire
    // version-less payload whose leading row happens to be a non-claim object.
    let force_v2 = doc.get("version").and_then(|v| v.as_str()) == Some("2");
    let row_is_v2 = |entry: &Value| -> bool {
        force_v2
            || entry
                .as_object()
                .map(|o| o.contains_key("claim_type") && !o.contains_key("@type"))
                .unwrap_or(false)
    };

    // Set of bare old-ids PRESENT in this export, so a supersedes ref can be
    // classified as "target present but unresolved" (a real dangling edge) vs
    // "target simply absent" (a legitimate partial-export passthrough).
    let present_ids: std::collections::HashSet<String> = claims_raw
        .iter()
        .filter_map(|e| extract_old_id(e, row_is_v2(e)))
        .collect();

    // Reorder so a superseded row is written BEFORE the row that supersedes
    // it, making the remap order-independent: the incremental remap invariant
    // (docstring) requires the target's new id to exist by the time we reach
    // its superseder, but a v2 exporter does not guarantee topological order.
    // Stable topological sort over the in-export supersedes edges; rows whose
    // target is absent keep their relative order. Cyclic/unresolved rows fall
    // through to the tail and surface as dangling.
    let ordered = topo_order_rows(claims_raw, &present_ids, &row_is_v2);

    for entry in ordered {
        let extracted = if row_is_v2(entry) {
            extract_v2_row(entry)
        } else {
            extract_replay_args(entry)
        };
        match extracted {
            Ok(args) => {
                // Remap the supersedes ref through the map. If the
                // superseded claim was imported earlier we point at its
                // new id; if it is absent from this export we pass the
                // original ref through (surfaces as a dangling edge --
                // the honest signal of an incomplete export) rather than
                // silently dropping the link.
                let supersedes = args.supersedes.map(|old| {
                    match id_remap.get(&old).cloned() {
                        Some(new_id) => new_id,
                        None => {
                            // Unresolved. If the target was present in this
                            // export, the chain is genuinely broken (a real
                            // integrity defect), so count it. If absent, it
                            // is an expected partial-export passthrough.
                            if present_ids.contains(&old) {
                                dangling_supersedes += 1;
                            }
                            old
                        }
                    }
                });
                store = store.with_asserter(args.asserter);
                match store.append_replay(args.claim_type, args.props, supersedes) {
                    Ok(new_id) => {
                        if let Some(old_id) = args.old_id {
                            id_remap.insert(old_id, new_id);
                        }
                        imported += 1;
                    }
                    Err(_) => skipped += 1,
                }
            }
            Err(_) => {
                skipped += 1;
            }
        }
    }

    json_out(&json!({
        "imported": imported,
        "skipped": skipped,
        "dangling_supersedes": dangling_supersedes,
    }))
}

/// Extract a row's bare exporter id (the `id` field for v2-flat rows, the
/// `@id` for v3 JSON-LD docs), normalized via `bare_claim_hash` so it shares
/// the remap/supersedes key space.
fn extract_old_id(entry: &Value, is_v2: bool) -> Option<String> {
    let obj = entry.as_object()?;
    let key = if is_v2 { "id" } else { "@id" };
    obj.get(key).and_then(|v| v.as_str()).map(bare_claim_hash)
}

/// Extract a row's bare supersedes target, if present and non-null.
fn extract_supersedes(entry: &Value, is_v2: bool) -> Option<String> {
    let obj = entry.as_object()?;
    let key = if is_v2 {
        "supersedes"
    } else {
        wire_format::SUPERSEDES_PRED
    };
    obj.get(key).and_then(|v| v.as_str()).map(bare_claim_hash)
}

/// Stable topological reorder of `claims_raw` so a row is emitted AFTER any
/// IN-EXPORT row it supersedes. Edges to ids not in `present_ids` are ignored
/// (partial-export passthroughs). The order is otherwise the input order; a
/// row whose dependency is unsatisfiable (cycle / dangling within the export)
/// is emitted in input order once no further progress is possible, so it still
/// surfaces as a dangling edge rather than being dropped.
fn topo_order_rows<'a>(
    rows: &'a [Value],
    present_ids: &std::collections::HashSet<String>,
    row_is_v2: &dyn Fn(&Value) -> bool,
) -> Vec<&'a Value> {
    let n = rows.len();
    // Per-row: its own id, and the in-export id it depends on (supersedes).
    let dep: Vec<Option<String>> = rows
        .iter()
        .map(|r| extract_supersedes(r, row_is_v2(r)).filter(|t| present_ids.contains(t)))
        .collect();

    let mut emitted = vec![false; n];
    let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<&Value> = Vec::with_capacity(n);

    // Kahn-style passes preserving input order. Repeat until a full pass
    // emits nothing, then flush any remaining rows (unsatisfiable deps) in
    // input order.
    loop {
        let mut progressed = false;
        for i in 0..n {
            if emitted[i] {
                continue;
            }
            let ready = match &dep[i] {
                None => true,
                Some(target) => done.contains(target),
            };
            if ready {
                emitted[i] = true;
                progressed = true;
                if let Some(id) = extract_old_id(&rows[i], row_is_v2(&rows[i])) {
                    done.insert(id);
                }
                out.push(&rows[i]);
            }
        }
        if !progressed {
            break;
        }
    }
    // Flush stragglers (cycle / unresolved) in input order.
    for i in 0..n {
        if !emitted[i] {
            out.push(&rows[i]);
        }
    }
    out
}

fn read_input(file: Option<&str>) -> Result<String> {
    match file {
        Some(p) if !p.is_empty() => {
            fs::read_to_string(p).with_context(|| format!("read import file {p}"))
        }
        _ => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("read import payload from stdin")?;
            Ok(buf)
        }
    }
}

/// The pieces of a raw v3 JSON-LD doc the replay path needs.
struct ReplayArgs {
    claim_type: ClaimType,
    props: Value,
    /// Supersedes ref as a bare hash (the exporter-side id of the
    /// superseded claim), if present. Remapped to a new id before write.
    supersedes: Option<ClaimId>,
    asserter: String,
    /// The exporter-side @id as a bare hash, if present. Used as the key
    /// when recording `old -> new` so later supersedes refs resolve.
    /// Normalized identically to `supersedes` so the two match.
    old_id: Option<ClaimId>,
}

/// Convert a raw v3 JSON-LD doc into the pieces `append_replay` needs.
///
/// The extractor:
/// 1. Reads `@type` -> snake_case claim type (`synthesist:Spec` -> `Spec`).
/// 2. Reads `prov:wasAttributedTo` -> asserter (`asserter:user:...` -> `user:...`).
/// 3. Reads `synthesist:supersedes` -> bare short hash, if present.
/// 4. Reads `@id` -> bare short hash, if present (the remap key).
/// 5. Drops envelope keys (`@context`, `@id`, `@type`, `prov:*`,
///    `nomograph:parentAsserter`, `synthesist:supersedes`).
/// 6. Reverses `lowerCamelCase` predicate names back to `snake_case`
///    props keys (`synthesist:dependsOn` -> `depends_on`).
///
/// Returns `Err` if the doc is missing required envelope fields or has
/// an `@type` that doesn't decode to a known `ClaimType`.
fn extract_replay_args(doc: &Value) -> Result<ReplayArgs> {
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
    let asserter_bare = asserter_iri
        .strip_prefix("asserter:")
        .unwrap_or(asserter_iri);
    // Route through the legacy normalizer defensively: it is a no-op for an
    // already-valid v3 asserter, but keeps the v2 and v3 import paths on one
    // code path so a legacy-shape attribution that survived into a v3 export
    // (e.g. a re-exported migrated estate) is repaired rather than dropped.
    let asserter = nomograph_claim::asserter::normalize_legacy(asserter_bare);
    // Validate the imported attribution before it is ever used to route a
    // write. An import file is untrusted input; a malicious
    // `prov:wasAttributedTo` (e.g. `user:..:..:x` or one carrying a path
    // separator) could otherwise drive `LogWriter::append` to write
    // outside the claims tree. Mirror the migration path, which rejects
    // unparseable asserters. The caller treats this `Err` as "skip".
    nomograph_claim::asserter::parse(&asserter)
        .map_err(|e| anyhow!("invalid prov:wasAttributedTo {asserter:?}: {e}"))?;

    let supersedes = obj
        .get(wire_format::SUPERSEDES_PRED)
        .and_then(|v| v.as_str())
        .map(bare_claim_hash);

    let old_id = obj.get("@id").and_then(|v| v.as_str()).map(bare_claim_hash);

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

    Ok(ReplayArgs {
        claim_type,
        props: Value::Object(props),
        supersedes,
        asserter,
        old_id,
    })
}

/// Convert one FLAT v2.5.2 export row into the pieces `append_replay`
/// needs. A v2 row is a raw SQL projection, not JSON-LD:
///
/// ```json
/// { "id": "<hash>", "claim_type": "spec", "props": { ... },
///   "asserted_by": "user:local:agd", "supersedes": "<hash>|null", ... }
/// ```
///
/// Mapping:
/// 1. `claim_type` (snake/lowercase) -> `ClaimType` via `claim_type_from_snake`.
/// 2. `props` -> the object VERBATIM (it is already the snake_case prop map;
///    it is NOT re-stringified or re-namespaced).
/// 3. `asserted_by` -> asserter, VALIDATED via `asserter::parse` (untrusted
///    input -- mirror the v3 path; parse failure is treated as skip).
/// 4. `supersedes` -> bare hash, if present and non-null (the remap key
///    space is shared with `id`, so the remap loop resolves chains).
/// 5. `id` -> the exporter-side id (the remap key for this row).
///
/// Returns `Err` (treated as skip) on a missing/unknown `claim_type`, a
/// missing `asserted_by`, or an unparseable asserter.
fn extract_v2_row(entry: &Value) -> Result<ReplayArgs> {
    let obj = entry
        .as_object()
        .ok_or_else(|| anyhow!("claims_raw entry is not a JSON object"))?;

    let type_str = obj
        .get("claim_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("v2 row missing claim_type"))?;
    let claim_type = claim_type_from_snake(type_str)
        .ok_or_else(|| anyhow!("unknown v2 claim_type: {type_str}"))?;

    let asserted_by_raw = obj
        .get("asserted_by")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("v2 row missing asserted_by"))?;
    // LOSSLESS legacy-asserter normalization (migration/import only). Maps
    // known v2 legacy shapes -- a 2-segment `user:migration-v1-v2` artifact,
    // and path-unsafe chars in a segment (e.g. a `/` in a session) -- into the
    // strict v3 grammar BEFORE the strict parse, so historical claims are not
    // dropped. It does NOT relax `parse`; the normalized string is still
    // validated below. It is a no-op for already-valid v3 asserters.
    let asserter = nomograph_claim::asserter::normalize_legacy(asserted_by_raw);
    // Untrusted input -- a malicious asserter could drive a write outside
    // the claims tree. Validate the NORMALIZED string before it routes any
    // write. Mirror the v3 path; the caller treats this Err as "skip".
    nomograph_claim::asserter::parse(&asserter)
        .map_err(|e| anyhow!("invalid asserted_by {asserter:?}: {e}"))?;

    // props verbatim (already the snake_case prop object). Default to an
    // empty object when absent so append_replay still has a valid map.
    let props = obj
        .get("props")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));

    // supersedes: bare hash, only when present AND non-null.
    let supersedes = obj
        .get("supersedes")
        .and_then(|v| v.as_str())
        .map(bare_claim_hash);

    let old_id = obj.get("id").and_then(|v| v.as_str()).map(bare_claim_hash);

    Ok(ReplayArgs {
        claim_type,
        props,
        supersedes,
        asserter,
        old_id,
    })
}

/// Strip the compact / expanded claim-IRI prefix from a claim reference,
/// leaving the bare hash. Applied identically to `@id` and to
/// `synthesist:supersedes` so a supersedes ref and the @id it points at
/// normalize to the same map key.
fn bare_claim_hash(s: &str) -> String {
    s.strip_prefix("https://nomograph.org/synthesist/claim/")
        .or_else(|| s.strip_prefix("synthesist:claim/"))
        .unwrap_or(s)
        .to_string()
}

/// Decode `synthesist:Spec` -> `ClaimType::Spec`.
///
/// Accepts both the compact form (`synthesist:Spec`) and the expanded
/// IRI (`https://nomograph.org/synthesist/Spec`) so docs that survived
/// a gamma index rebuild are still decodable.
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
        let args = extract_replay_args(&doc).unwrap();
        assert!(matches!(args.claim_type, ClaimType::Task));
        assert_eq!(args.asserter, "user:local:agd");
        assert_eq!(args.supersedes.as_deref(), Some("1111222233334444"));
        assert_eq!(args.old_id.as_deref(), Some("abcdef0123456789"));
        let obj = args.props.as_object().unwrap();
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

    // ---- v2 flat-row extractor ----

    #[test]
    fn extract_v2_row_maps_flat_fields() {
        let row = json!({
            "id": "a1f0c3d2e4b50617",
            "claim_type": "spec",
            "props": { "title": "v2->v3 migration", "status": "draft" },
            "valid_from": "2026-01-02T09:05:00Z",
            "valid_until": null,
            "supersedes": null,
            "asserted_by": "user:local:agd",
            "asserted_at": "2026-01-02T09:05:00Z"
        });
        let args = extract_v2_row(&row).unwrap();
        assert!(matches!(args.claim_type, ClaimType::Spec));
        assert_eq!(args.asserter, "user:local:agd");
        assert_eq!(args.old_id.as_deref(), Some("a1f0c3d2e4b50617"));
        assert!(args.supersedes.is_none());
        // props passed through verbatim (NOT re-namespaced).
        let obj = args.props.as_object().unwrap();
        assert_eq!(
            obj.get("title").and_then(|v| v.as_str()),
            Some("v2->v3 migration")
        );
        assert_eq!(obj.get("status").and_then(|v| v.as_str()), Some("draft"));
    }

    #[test]
    fn extract_v2_row_carries_supersedes_for_remap() {
        // Two rows: the second supersedes the first. After extraction the
        // second's supersedes key equals the first's old_id, so the shared
        // remap loop resolves the chain.
        let earlier = json!({
            "id": "b2e1d4c3f5a61728",
            "claim_type": "spec",
            "props": { "status": "draft" },
            "supersedes": null,
            "asserted_by": "user:local:agd"
        });
        let later = json!({
            "id": "f6a5b8c7d9e05162",
            "claim_type": "spec",
            "props": { "status": "accepted" },
            "supersedes": "b2e1d4c3f5a61728",
            "asserted_by": "user:local:agd"
        });
        let a = extract_v2_row(&earlier).unwrap();
        let b = extract_v2_row(&later).unwrap();
        assert_eq!(a.old_id.as_deref(), Some("b2e1d4c3f5a61728"));
        assert_eq!(b.supersedes.as_deref(), Some("b2e1d4c3f5a61728"));
        assert_eq!(b.supersedes, a.old_id);
    }

    #[test]
    fn extract_v2_row_invalid_asserter_is_skip() {
        let row = json!({
            "id": "deadbeef",
            "claim_type": "task",
            "props": { "summary": "x" },
            "asserted_by": "not a valid asserter with spaces"
        });
        assert!(extract_v2_row(&row).is_err());
    }

    #[test]
    fn extract_v2_row_unknown_type_is_skip() {
        let row = json!({
            "id": "deadbeef",
            "claim_type": "bogus",
            "props": {},
            "asserted_by": "user:local:agd"
        });
        assert!(extract_v2_row(&row).is_err());
    }

    #[test]
    fn extract_v2_row_missing_asserter_is_skip() {
        let row = json!({
            "id": "deadbeef",
            "claim_type": "task",
            "props": {}
        });
        assert!(extract_v2_row(&row).is_err());
    }
}

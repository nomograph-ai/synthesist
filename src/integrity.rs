//! Integrity walk helpers for `synthesist check`.
//!
//! `cmd_check` runs three distinct integrity checks:
//!
//! 1. Schema validation -- every live claim's props must pass the
//!    per-type validator (`crate::schema::validate_claim`).
//! 2. Dangling `synthesist:supersedes` -- a claim points at a prior
//!    id that has no document in the log.
//! 3. Dangling task `depends_on` -- a Task references a sibling task
//!    id (in the same tree/spec) that has no live Task claim.
//!
//! Check 1 needs v2-shape snake_case props (the per-type validators in
//! `crate::schema::*` are written against the v2 contract). The v3
//! substrate stores JSON-LD docs with envelope predicates plus
//! `synthesist:`-prefixed lowerCamelCase keys. [`v3_to_v2_props`] is
//! the inverse of `wire_format::predicate_iri` + the envelope wrap in
//! `store::build_jsonld_doc` / `migrations::v2_to_v3::v2_claim_to_v3`.
//! It is the integrity-check side of the dual-write mapping.
//!
//! Checks 2 and 3 are SPARQL-shaped (one SELECT each, client-side
//! compare). They live in `cmd_init::cmd_check`; this module exposes
//! only the prop-shape helper and the @type-IRI -> ClaimType helper
//! they share.

use crate::claim_type::ClaimType;
use serde_json::{Map, Value};

/// Envelope predicates dropped during the v3-to-v2 props rewrite.
///
/// These mirror `crate::schema::ENVELOPE_PREDICATES` but are inlined
/// here so the integrity walk has a self-contained mapping. Keep both
/// lists in sync when adding a substrate-level predicate.
const ENVELOPE_PREDICATES: &[&str] = &[
    "@context",
    "@id",
    "@type",
    "prov:generatedAtTime",
    "prov:wasAttributedTo",
    "prov:wasRevisionOf",
    "nomograph:parentAsserter",
    "synthesist:supersedes",
];

/// Module prefixes stripped during the v3-to-v2 props rewrite.
const MODULE_PREFIXES: &[&str] = &["synthesist:", "nomograph:"];

/// Reverse-map a v3 JSON-LD document into the v2 bare-key snake_case
/// `props` object that `crate::schema::*` validators expect.
///
/// Steps applied to every top-level key of `doc`:
/// 1. Drop envelope predicates ([`ENVELOPE_PREDICATES`]).
/// 2. Strip module prefixes ([`MODULE_PREFIXES`]) from the key.
/// 3. Convert the remaining lowerCamelCase tail back to snake_case.
///
/// Step 3 is the inverse of `wire_format::lower_camel_case`. The
/// substrate dual-write path runs `predicate_iri(k) = "synthesist:" +
/// lower_camel_case(k)`; this helper undoes both halves so a v3 doc
/// with `synthesist:dependsOn` round-trips to a props object with
/// `depends_on` -- which is what `schema::task::validate` checks for.
///
/// Returns a flat `Value::Object`. Returns an empty object for a
/// non-object input rather than failing; the caller surfaces that as
/// a schema failure downstream via the validator.
pub fn v3_to_v2_props(doc: &Value) -> Value {
    let Some(map) = doc.as_object() else {
        return Value::Object(Map::new());
    };

    let mut out = Map::with_capacity(map.len());
    for (k, v) in map {
        if ENVELOPE_PREDICATES.iter().any(|p| k == p) {
            continue;
        }
        let stripped = MODULE_PREFIXES
            .iter()
            .find_map(|p| k.strip_prefix(*p))
            .unwrap_or(k.as_str());
        let snake = lower_camel_to_snake(stripped);
        out.insert(snake, v.clone());
    }
    Value::Object(out)
}

/// Convert `lowerCamelCase` to `snake_case`. Inverse of
/// `wire_format::lower_camel_case` for the alphanumeric tail names we
/// use as predicates.
///
/// - `id` -> `id`
/// - `dependsOn` -> `depends_on`
/// - `agreeSnapshot` -> `agree_snapshot`
/// - `verifyCmd` -> `verify_cmd`
///
/// Inserts `_` before every ASCII uppercase letter (other than at the
/// start) and lowercases the letter. Numeric and other non-letter
/// runs pass through unchanged. The keys we round-trip are simple
/// identifiers, so this minimal implementation suffices.
fn lower_camel_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse a `@type` IRI on a v3 doc into a [`ClaimType`].
///
/// Accepts both expanded form (`https://nomograph.org/synthesist/Task`)
/// and compact form (`synthesist:Task`). The inverse of
/// `wire_format::type_iri`. Returns `None` for types that are not
/// synthesist-owned or are unrecognized; the caller surfaces those as
/// `no_validator` warnings rather than schema errors.
pub fn claim_type_from_iri(iri: &str) -> Option<ClaimType> {
    let bare = iri
        .strip_prefix("https://nomograph.org/synthesist/")
        .or_else(|| iri.strip_prefix("synthesist:"))
        .unwrap_or(iri);
    match bare {
        "Tree" => Some(ClaimType::Tree),
        "Spec" => Some(ClaimType::Spec),
        "Task" => Some(ClaimType::Task),
        "Discovery" => Some(ClaimType::Discovery),
        "Campaign" => Some(ClaimType::Campaign),
        "Session" => Some(ClaimType::Session),
        "Phase" => Some(ClaimType::Phase),
        "Outcome" => Some(ClaimType::Outcome),
        "Intent" => Some(ClaimType::Intent),
        "Heartbeat" => Some(ClaimType::Heartbeat),
        "Directive" => Some(ClaimType::Directive),
        "Stakeholder" => Some(ClaimType::Stakeholder),
        "Topic" => Some(ClaimType::Topic),
        "Signal" => Some(ClaimType::Signal),
        "Disposition" => Some(ClaimType::Disposition),
        _ => None,
    }
}

/// Pull the short `@id` value off a v3 doc. Returns the empty string
/// if absent or non-string.
pub fn doc_id(doc: &Value) -> String {
    doc.get("@id")
        .and_then(|v| v.as_str())
        .map(|s| {
            // Strip the `synthesist:claim/` compact form so the issue's
            // `claim_id` field carries the raw hash, matching v2.
            s.strip_prefix("synthesist:claim/")
                .unwrap_or(s)
                .to_string()
        })
        .unwrap_or_default()
}

/// Pull the bare type name off a v3 doc's `@type` IRI.
pub fn doc_type_str(doc: &Value) -> String {
    doc.get("@type")
        .and_then(|v| v.as_str())
        .map(|s| {
            s.strip_prefix("https://nomograph.org/synthesist/")
                .or_else(|| s.strip_prefix("synthesist:"))
                .unwrap_or(s)
                .to_string()
        })
        .map(|s| {
            // Lowercase first char so `Task` reads `task` for the
            // `issues[].claim_type` field. Matches v2 wire shape.
            let mut chars = s.chars();
            match chars.next() {
                Some(c) => c.to_lowercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lower_camel_to_snake_handles_simple_keys() {
        assert_eq!(lower_camel_to_snake("id"), "id");
        assert_eq!(lower_camel_to_snake("tree"), "tree");
        assert_eq!(lower_camel_to_snake("dependsOn"), "depends_on");
        assert_eq!(lower_camel_to_snake("agreeSnapshot"), "agree_snapshot");
        assert_eq!(lower_camel_to_snake("verifyCmd"), "verify_cmd");
        assert_eq!(lower_camel_to_snake("linkedSpec"), "linked_spec");
        assert_eq!(lower_camel_to_snake("blockedBy"), "blocked_by");
        assert_eq!(lower_camel_to_snake("sessionId"), "session_id");
    }

    #[test]
    fn v3_to_v2_props_drops_envelope_and_unprefixes_keys() {
        let doc = json!({
            "@context": {"foo": "bar"},
            "@id": "synthesist:claim/abc",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:supersedes": "synthesist:claim/zzz",
            "synthesist:tree": "k",
            "synthesist:spec": "s",
            "synthesist:id": "t1",
            "synthesist:summary": "do x",
            "synthesist:status": "pending",
            "synthesist:dependsOn": ["t0"],
        });
        let v2 = v3_to_v2_props(&doc);
        let obj = v2.as_object().unwrap();
        // Envelope dropped.
        assert!(!obj.contains_key("@id"));
        assert!(!obj.contains_key("@type"));
        assert!(!obj.contains_key("prov:generatedAtTime"));
        assert!(!obj.contains_key("synthesist:supersedes"));
        // Props present, snake_case.
        assert_eq!(obj["tree"], json!("k"));
        assert_eq!(obj["status"], json!("pending"));
        assert_eq!(obj["depends_on"], json!(["t0"]));
    }

    #[test]
    fn v3_to_v2_props_is_callable_by_validator() {
        // A v3 Task doc should normalize into something
        // schema::task::validate accepts.
        let doc = json!({
            "@id": "synthesist:claim/abc",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:tree": "k",
            "synthesist:spec": "s",
            "synthesist:id": "t1",
            "synthesist:summary": "do x",
            "synthesist:status": "pending",
            "synthesist:dependsOn": ["t0"]
        });
        let v2 = v3_to_v2_props(&doc);
        crate::schema::validate_props(&ClaimType::Task, &v2)
            .expect("v3-to-v2 props validates against the v2 Task schema");
    }

    #[test]
    fn claim_type_from_iri_accepts_both_iri_forms() {
        assert_eq!(
            claim_type_from_iri("synthesist:Task"),
            Some(ClaimType::Task)
        );
        assert_eq!(
            claim_type_from_iri("https://nomograph.org/synthesist/Spec"),
            Some(ClaimType::Spec)
        );
        assert_eq!(claim_type_from_iri("synthesist:Unknown"), None);
    }

    #[test]
    fn doc_id_strips_compact_prefix() {
        let doc = json!({
            "@id": "synthesist:claim/abcdef0123456789",
            "@type": "synthesist:Task",
        });
        assert_eq!(doc_id(&doc), "abcdef0123456789");
    }

    #[test]
    fn doc_type_str_lowercases_first_char() {
        let doc = json!({"@type": "synthesist:Task"});
        assert_eq!(doc_type_str(&doc), "task");
        let doc = json!({"@type": "https://nomograph.org/synthesist/Discovery"});
        assert_eq!(doc_type_str(&doc), "discovery");
    }
}

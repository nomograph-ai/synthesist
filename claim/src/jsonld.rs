//! Compact JSON-LD form used on disk by the v3 substrate.
//!
//! Every claim is one JSON-LD document. Lines in
//! `claims/<asserter>/log.jsonl` are compact-form JSON-LD with the
//! `@context` declared by URI; tools embed a local copy of the context
//! body for offline parsing.
//!
//! Form: each claim has `@id`, `@type`, `prov:generatedAtTime`,
//! `prov:wasAttributedTo`, and zero-or-more module-prefixed
//! predicates. Optional fields: `<module>:supersedes`,
//! `nomograph:parentAsserter`.
//!
//! See `docs/jsonld-form.md` for the full specification.

use serde_json::{Value, json};

/// Canonical URI for the base @context. Tools embed
/// [`BASE_CONTEXT_BODY`] locally; this URI is the identity.
pub const BASE_CONTEXT_URI: &str = "https://nomograph.org/v3/context.jsonld";

/// IRI prefix for the substrate's own vocabulary (Asserter,
/// Supersedes, ParentAsserter, AssertedAt, AsserterClass).
pub const NOMOGRAPH_NS: &str = "https://nomograph.org/v3/";

/// PROV-O namespace. We use `prov:generatedAtTime` and
/// `prov:wasAttributedTo` on every claim; `prov:wasRevisionOf` is the
/// canonical superseding edge if a module prefers it over its own
/// `<module>:supersedes`.
pub const PROV_NS: &str = "http://www.w3.org/ns/prov#";

/// XSD namespace, used for typed literals (xsd:dateTime).
pub const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema#";

/// Inline @context body. JSON-LD parsers can load this in lieu of
/// fetching [`BASE_CONTEXT_URI`].
///
/// The body covers the universal envelope only: prov attribution,
/// asserter IRI typing, supersession (as @id). Module-specific
/// predicates layer on at write time via the module's own context.
pub const BASE_CONTEXT_BODY: &str = r#"{
  "@context": {
    "nomograph": "https://nomograph.org/v3/",
    "prov":     "http://www.w3.org/ns/prov#",
    "xsd":      "http://www.w3.org/2001/XMLSchema#",
    "prov:generatedAtTime": { "@type": "xsd:dateTime" },
    "prov:wasAttributedTo": { "@type": "@id" },
    "prov:wasRevisionOf":   { "@type": "@id" },
    "nomograph:parentAsserter": { "@type": "@id" }
  }
}"#;

/// The base @context parsed as `serde_json::Value`. Pre-parsed so
/// callers can splice it into JSON-LD docs without re-parsing per call.
pub fn base_context_value() -> Value {
    serde_json::from_str(BASE_CONTEXT_BODY).expect("BASE_CONTEXT_BODY is valid JSON")
}

/// Inline @context value (just the inner object, not the wrapping
/// `{"@context": ...}`). Useful when constructing a JSON-LD doc and
/// you want to merge module contexts.
pub fn base_context_inner() -> Value {
    base_context_value()["@context"].clone()
}

/// Construct a claim IRI for a module: `<module>:claim/<hash>`. The
/// hash is a content-addressed identifier (blake3, truncated). Caller
/// supplies the hash; this function just composes the IRI string.
pub fn claim_iri(module_prefix: &str, hash: &str) -> String {
    format!("{}:claim/{}", module_prefix, hash)
}

/// Construct an asserter IRI from a parsed asserter string. Callers
/// using the [`crate::asserter`] module should prefer
/// `Asserter::to_iri()` directly; this helper is for plumbing that
/// has the raw string only.
pub fn asserter_iri(asserter: &str) -> String {
    format!("asserter:{}", asserter)
}

/// Module context merge: given a module's @context body (e.g.
/// synthesist's), return a combined @context object with the base
/// envelope plus the module's predicates.
///
/// JSON-LD's @context can be an array; this helper picks the array
/// form so both contexts apply.
pub fn merge_contexts(module_context: Value) -> Value {
    json!([base_context_inner(), module_context])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn base_context_body_parses_as_json() {
        let v: Value = serde_json::from_str(BASE_CONTEXT_BODY).unwrap();
        assert!(v.is_object());
        assert!(v.get("@context").unwrap().is_object());
    }

    #[test]
    fn base_context_value_round_trips() {
        let v = base_context_value();
        let back = serde_json::to_string(&v).unwrap();
        let again: Value = serde_json::from_str(&back).unwrap();
        assert_eq!(v, again);
    }

    #[test]
    fn base_context_declares_universal_predicates() {
        let ctx = base_context_inner();
        assert!(ctx.get("prov:generatedAtTime").is_some());
        assert!(ctx.get("prov:wasAttributedTo").is_some());
        assert!(ctx.get("nomograph:parentAsserter").is_some());
        assert_eq!(
            ctx["prov:generatedAtTime"]["@type"].as_str(),
            Some("xsd:dateTime")
        );
        assert_eq!(ctx["prov:wasAttributedTo"]["@type"].as_str(), Some("@id"));
    }

    #[test]
    fn claim_iri_composes_correctly() {
        assert_eq!(claim_iri("synthesist", "abc123"), "synthesist:claim/abc123");
        assert_eq!(
            claim_iri("nomograph", "deadbeef"),
            "nomograph:claim/deadbeef"
        );
    }

    #[test]
    fn asserter_iri_composes_correctly() {
        assert_eq!(asserter_iri("user:local:agd"), "asserter:user:local:agd");
        assert_eq!(
            asserter_iri("user:local:agd:work"),
            "asserter:user:local:agd:work"
        );
    }

    #[test]
    fn merge_contexts_produces_array_with_base_first() {
        let module = json!({"synthesist": "https://nomograph.org/synthesist/"});
        let merged = merge_contexts(module.clone());
        let arr = merged.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr[0].is_object());
        assert_eq!(arr[1], module);
    }

    /// The base context, spliced into a minimal claim, yields a doc
    /// whose envelope predicates round-trip through serde unchanged.
    /// (The alpha variant parsed this through oxjsonld; the gamma index
    /// reads the compact JSON-LD keys directly, so a serde round-trip is
    /// the relevant invariant.)
    #[test]
    fn minimal_claim_carries_envelope_predicates() {
        let doc = json!({
            "@context": base_context_inner(),
            "@id": "synthesist:claim/test123",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T01:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd"
        });
        let bytes = serde_json::to_vec(&doc).unwrap();
        let back: Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(back["@type"].as_str(), Some("synthesist:Task"));
        assert_eq!(
            back["prov:generatedAtTime"].as_str(),
            Some("2026-05-29T01:00:00.000Z")
        );
        assert_eq!(
            back["prov:wasAttributedTo"].as_str(),
            Some("asserter:user:local:agd")
        );
    }
}

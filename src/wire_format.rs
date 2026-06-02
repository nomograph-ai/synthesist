//! Canonical v3 JSON-LD wire format for synthesist-owned claims.
//!
//! Single source of truth for:
//!
//! - The inline `@context` shape (prefixes, `@type @id` declarations).
//! - Case conversions between v2 props (snake_case) and v3 JSON-LD
//!   (`TitleCase` for `@type`, `lowerCamelCase` for predicate names).
//! - Compact-IRI builders (`synthesist:claim/<short>`, `synthesist:Type`,
//!   `synthesist:TypeShape`, predicate names) consumed by the gamma
//!   index retarget.
//!
//! Every synthesist component that emits or matches v3 IRIs should
//! reach into this module rather than hand-coding the strings:
//!
//! - `store` (the live write path)
//! - `migrations::v2_to_v3` (the migration path)
//! - `bin::emit_shacl` (the SHACL emitter)
//! - `skill` (skill content / schema reference)
//! - the gamma index typed-query helpers
//!
//! Co-locating the wire format here prevents the drift Scan B
//! surfaced: five hand-written test `@context` helpers with each
//! omitting a different subset of canonical entries, plus two
//! independent `camel_case` definitions in store and migrations.

use std::sync::LazyLock;

use serde_json::{Value, json};

/// Synthesist module prefix used in compact-form IRIs and `@context`.
pub const MODULE_PREFIX: &str = "synthesist";

// ---------------------------------------------------------------------------
// Substrate-level predicate keys
//
// These are the four IRI keys that `jsonld_context` declares typing for
// (`@type @id` for the supersedes and parent-asserter refs;
// `xsd:dateTime` for generatedAtTime). Co-locating the literals here
// closes the drift surface Scan C identified: previously both
// `store` (the write path) and `migrations::v2_to_v3::v2_claim_to_v3`
// hand-coded the same strings, so a rename in one and not the other
// would silently produce divergent docs.
// ---------------------------------------------------------------------------

/// Predicate key for the supersession reference. IRI-typed via `@context`.
pub const SUPERSEDES_PRED: &str = "synthesist:supersedes";

/// Predicate key for the parent-asserter reference. IRI-typed via `@context`.
pub const PARENT_ASSERTER_PRED: &str = "nomograph:parentAsserter";

/// Predicate key for the generation time. `xsd:dateTime` typed via `@context`.
pub const GENERATED_AT_PRED: &str = "prov:generatedAtTime";

/// Predicate key for the attribution. IRI-typed via `@context`.
pub const ATTRIBUTED_TO_PRED: &str = "prov:wasAttributedTo";

/// Expanded IRI the `synthesist:` prefix maps to.
///
/// JSON-LD parsers without context resolution treat `synthesist:Spec`
/// as the bare URI scheme `<synthesist:Spec>`. Declaring this prefix in
/// the inline `@context` (below) tells parsers to expand to
/// `<https://nomograph.org/synthesist/Spec>` instead, which is the IRI
/// the gamma index keys on for typed queries.
pub const NAMESPACE_IRI: &str = "https://nomograph.org/synthesist/";

/// Claim hash truncation length for the compact IRI form.
///
/// 16 hex chars give 64 bits of collision resistance, more than enough
/// for the storr-scale and team-scale corpora pre.1 targets.
pub const ID_TRUNCATION: usize = 16;

/// One-shot cached `@context` value. Built on first access and cloned
/// for every caller -- the migration of a 143-claim corpus would
/// otherwise re-run `json!{}` 143 times.
static CACHED_CONTEXT: LazyLock<Value> = LazyLock::new(|| {
    json!({
        "nomograph":  "https://nomograph.org/v3/",
        "synthesist": NAMESPACE_IRI,
        "prov":       "http://www.w3.org/ns/prov#",
        "xsd":        "http://www.w3.org/2001/XMLSchema#",
        GENERATED_AT_PRED:    {"@type": "xsd:dateTime"},
        ATTRIBUTED_TO_PRED:   {"@type": "@id"},
        "prov:wasRevisionOf": {"@type": "@id"},
        PARENT_ASSERTER_PRED: {"@type": "@id"},
        SUPERSEDES_PRED:      {"@type": "@id"},
        "synthesist:agreeSnapshot": {"@type": "@id", "@container": "@set"}
    })
});

/// Canonical inline `@context` for v3 JSON-LD docs.
///
/// Declares the `synthesist`, `nomograph`, `prov`, `xsd` prefixes plus
/// `@type @id` for IRI-reference predicates. The gamma index respects
/// an inline `@context` object when shredding docs into its POS/PSO
/// tables, so this survives the index rebuild and produces IRI-typed
/// edges matching what the typed query helpers expect.
///
/// The value is cached (see `CACHED_CONTEXT`) and cloned per call.
pub fn jsonld_context() -> Value {
    CACHED_CONTEXT.clone()
}

/// Convert a `snake_case` or `kebab-case` string to `TitleCase`.
///
/// Used for v3 `@type` IRIs (e.g. `task` -> `Task`, `agree_snapshot` ->
/// `AgreeSnapshot`). Pairs with `type_iri` and `shape_iri`.
pub fn camel_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            out.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Convert a `snake_case` or `kebab-case` string to `lowerCamelCase`.
///
/// Used for v3 predicate names so the JSON-LD predicate aligns with
/// the SHACL ontology and the gamma index predicate keys (e.g.
/// `agree_snapshot` -> `agreeSnapshot`). Single-word inputs pass
/// through unchanged.
pub fn lower_camel_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            out.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Truncate a claim hash to `ID_TRUNCATION` chars for the compact form.
pub fn short_id(claim_id: &str) -> &str {
    &claim_id[..claim_id.len().min(ID_TRUNCATION)]
}

/// Build a compact claim IRI: `synthesist:claim/<short>`.
pub fn claim_iri(claim_id: &str) -> String {
    format!("{}:claim/{}", MODULE_PREFIX, short_id(claim_id))
}

/// Build a `@type` IRI: `synthesist:<TitleCase(type)>` (e.g. `synthesist:Spec`).
pub fn type_iri(claim_type: &str) -> String {
    format!("{}:{}", MODULE_PREFIX, camel_case(claim_type))
}

/// Build a SHACL shape IRI: `synthesist:<TitleCase(type)>Shape`.
///
/// Used by the `emit-shacl` binary and by tests that assert skill
/// content references the right shape names. The lib's non-test build
/// has no caller today; future overlays that traverse SHACL shapes
/// would consume this.
///
/// **Input contract**: `claim_type` is a snake_case or single-word
/// TitleCase claim type name (`"tree"`, `"agree_snapshot"`, `"Tree"`).
/// Do NOT pass a string that already includes the `Shape` suffix --
/// `shape_iri("TaskShape")` produces the malformed
/// `"synthesist:TaskShapeShape"`. The function does not strip the
/// suffix or otherwise sanitize the input.
#[allow(dead_code)]
pub fn shape_iri(claim_type: &str) -> String {
    debug_assert!(
        !claim_type.ends_with("Shape"),
        "shape_iri: claim_type must not already include the 'Shape' suffix; got {claim_type:?}"
    );
    format!("{}:{}Shape", MODULE_PREFIX, camel_case(claim_type))
}

/// Build a predicate IRI: `synthesist:<lowerCamel(key)>`.
pub fn predicate_iri(key: &str) -> String {
    format!("{}:{}", MODULE_PREFIX, lower_camel_case(key))
}

/// Build an asserter IRI: `asserter:<asserter>`. Mirrors the format
/// the write path emits for `prov:wasAttributedTo` values.
pub fn asserter_iri(asserter: &str) -> String {
    format!("asserter:{}", asserter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_handles_compound_words() {
        assert_eq!(camel_case("task"), "Task");
        assert_eq!(camel_case("agree_snapshot"), "AgreeSnapshot");
        assert_eq!(camel_case("plan-at-risk"), "PlanAtRisk");
    }

    #[test]
    fn lower_camel_case_handles_compound_words() {
        assert_eq!(lower_camel_case("id"), "id");
        assert_eq!(lower_camel_case("agree_snapshot"), "agreeSnapshot");
        assert_eq!(lower_camel_case("depends_on"), "dependsOn");
        assert_eq!(lower_camel_case("verify-cmd"), "verifyCmd");
    }

    #[test]
    fn short_id_truncates_at_16() {
        let id = "abcdef0123456789aaaa";
        assert_eq!(short_id(id), "abcdef0123456789");
    }

    #[test]
    fn short_id_handles_already_short_input() {
        assert_eq!(short_id("abc"), "abc");
    }

    #[test]
    fn claim_iri_uses_truncated_hash() {
        let iri = claim_iri("abcdef0123456789aaaa");
        assert_eq!(iri, "synthesist:claim/abcdef0123456789");
    }

    #[test]
    fn type_iri_uses_title_case() {
        assert_eq!(type_iri("task"), "synthesist:Task");
        assert_eq!(type_iri("agree_snapshot"), "synthesist:AgreeSnapshot");
    }

    #[test]
    fn shape_iri_uses_title_case_with_shape_suffix() {
        assert_eq!(shape_iri("task"), "synthesist:TaskShape");
        assert_eq!(shape_iri("spec"), "synthesist:SpecShape");
    }

    #[test]
    fn predicate_iri_uses_lower_camel_case() {
        assert_eq!(predicate_iri("id"), "synthesist:id");
        assert_eq!(predicate_iri("agree_snapshot"), "synthesist:agreeSnapshot");
        assert_eq!(predicate_iri("depends_on"), "synthesist:dependsOn");
    }

    #[test]
    fn asserter_iri_prefixes_with_asserter_scheme() {
        assert_eq!(asserter_iri("user:local:agd"), "asserter:user:local:agd");
    }



    #[test]
    fn jsonld_context_declares_all_iri_typed_predicates() {
        let ctx = jsonld_context();
        let inner = ctx.as_object().expect("context is an object");

        for key in [
            "prov:generatedAtTime",
            "prov:wasAttributedTo",
            "prov:wasRevisionOf",
            "nomograph:parentAsserter",
            "synthesist:supersedes",
            "synthesist:agreeSnapshot",
        ] {
            assert!(inner.contains_key(key), "context missing predicate {key}");
        }

        // Prefix mappings sanity check.
        assert_eq!(inner["synthesist"].as_str(), Some(NAMESPACE_IRI));
        assert_eq!(
            inner["nomograph"].as_str(),
            Some("https://nomograph.org/v3/")
        );

        // The IRI-typed predicates carry `{"@type": "@id"}` so parsers
        // treat values as IRIs not literals.
        assert_eq!(
            inner["synthesist:supersedes"]["@type"].as_str(),
            Some("@id")
        );
        assert_eq!(
            inner["synthesist:agreeSnapshot"]["@type"].as_str(),
            Some("@id")
        );
    }
}

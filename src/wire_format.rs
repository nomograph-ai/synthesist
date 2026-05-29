//! Canonical v3 JSON-LD wire format for synthesist-owned claims.
//!
//! Single source of truth for:
//!
//! - The inline `@context` shape (prefixes, `@type @id` declarations).
//! - Case conversions between v2 props (snake_case) and v3 JSON-LD
//!   (`TitleCase` for `@type`, `lowerCamelCase` for predicate names).
//! - Compact-IRI builders (`synthesist:claim/<short>`, `synthesist:Type`,
//!   `synthesist:TypeShape`, predicate names).
//! - The SPARQL `PREFIX` preamble overlays and queries should share.
//!
//! Every synthesist component that emits or matches v3 IRIs should
//! reach into this module rather than hand-coding the strings:
//!
//! - `store::v3_dual_write` (the live write path)
//! - `migrations::v2_to_v3` (the migration path)
//! - `bin::emit_shacl` (the SHACL emitter)
//! - `skill` (skill content / schema reference)
//! - overlay test fixtures and SPARQL queries
//!
//! Co-locating the wire format here prevents the drift Scan B
//! surfaced: five hand-written test `@context` helpers with each
//! omitting a different subset of canonical entries, plus two
//! independent `camel_case` definitions in store and migrations.

use serde_json::{Value, json};

/// Synthesist module prefix used in compact-form IRIs and `@context`.
pub const MODULE_PREFIX: &str = "synthesist";

/// Expanded IRI the `synthesist:` prefix maps to.
///
/// JSON-LD parsers without context resolution treat `synthesist:Spec`
/// as the bare URI scheme `<synthesist:Spec>`. Declaring this prefix in
/// the inline `@context` (below) tells parsers to expand to
/// `<https://nomograph.org/synthesist/Spec>` instead, which is what
/// the overlay SPARQL `PREFIX synthesist: <...>` declarations expect.
pub const NAMESPACE_IRI: &str = "https://nomograph.org/synthesist/";

/// Claim hash truncation length for the compact IRI form.
///
/// 16 hex chars give 64 bits of collision resistance, more than enough
/// for the storr-scale and team-scale corpora pre.1 targets.
pub const ID_TRUNCATION: usize = 16;

/// Canonical inline `@context` for v3 JSON-LD docs.
///
/// Declares the `synthesist`, `nomograph`, `prov`, `xsd` prefixes plus
/// `@type @id` for IRI-reference predicates. `graph_view::rebuild`
/// respects an inline `@context` object (only the bare-URI form is
/// replaced with `base_context_inner`), so this survives the rebuild
/// round-trip and produces IRI-typed triples matching what overlay
/// SPARQL queries expect.
pub fn jsonld_context() -> Value {
    json!({
        "nomograph":  "https://nomograph.org/v3/",
        "synthesist": NAMESPACE_IRI,
        "prov":       "http://www.w3.org/ns/prov#",
        "xsd":        "http://www.w3.org/2001/XMLSchema#",
        "prov:generatedAtTime": {"@type": "xsd:dateTime"},
        "prov:wasAttributedTo": {"@type": "@id"},
        "prov:wasRevisionOf":   {"@type": "@id"},
        "nomograph:parentAsserter": {"@type": "@id"},
        "synthesist:supersedes":    {"@type": "@id"},
        "synthesist:agreeSnapshot": {"@type": "@id", "@container": "@set"}
    })
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
/// the SHACL ontology and the overlay SPARQL (e.g. `agree_snapshot`
/// -> `agreeSnapshot`). Single-word inputs pass through unchanged.
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
#[allow(dead_code)]
pub fn shape_iri(claim_type: &str) -> String {
    format!("{}:{}Shape", MODULE_PREFIX, camel_case(claim_type))
}

/// Build a predicate IRI: `synthesist:<lowerCamel(key)>`.
pub fn predicate_iri(key: &str) -> String {
    format!("{}:{}", MODULE_PREFIX, lower_camel_case(key))
}

/// Build an asserter IRI: `asserter:<asserter>`. Mirrors the format
/// the dual-write emits for `prov:wasAttributedTo` values.
pub fn asserter_iri(asserter: &str) -> String {
    format!("asserter:{}", asserter)
}

/// SPARQL `PREFIX` preamble for synthesist overlay queries.
///
/// Every overlay query (`overlay/plan_at_risk.rs`, `overlay/demo.rs`,
/// future overlays) should prepend this so the prefix IRIs stay in
/// lockstep with `jsonld_context`. Use as `format!("{}{}", PREAMBLE, body)`.
pub const SPARQL_PREFIX_PREAMBLE: &str = "\
PREFIX rdf:        <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
PREFIX rdfs:       <http://www.w3.org/2000/01/rdf-schema#>
PREFIX synthesist: <https://nomograph.org/synthesist/>
PREFIX nomograph:  <https://nomograph.org/v3/>
PREFIX prov:       <http://www.w3.org/ns/prov#>
PREFIX xsd:        <http://www.w3.org/2001/XMLSchema#>
";

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

    #[test]
    fn sparql_preamble_declares_synthesist_prefix() {
        assert!(SPARQL_PREFIX_PREAMBLE.contains("PREFIX synthesist:"));
        assert!(SPARQL_PREFIX_PREAMBLE.contains(NAMESPACE_IRI));
    }
}

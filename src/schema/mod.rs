//! Synthesist domain schemas.
//!
//! Single source of truth for every claim type synthesist owns. Each
//! sub-module defines the schema for one claim type by:
//!
//! 1. Declaring enum value sets as `pub const` slices, so the same
//!    constant is referenced by the validator AND by clap's
//!    `PossibleValuesParser` in the CLI. CLI accepts iff schema
//!    accepts is structural — there is nothing to keep in sync
//!    because there is only one definition.
//! 2. Exposing a `validate(props: &Value)` function that uses the
//!    `nomograph_claim::validation` helpers to check field presence,
//!    types, and enum membership. Returns a structured `SchemaError`
//!    that names the claim type, field, actual value, and expected
//!    set so callers can format for humans, surface to agents, or
//!    pattern-match for retry.
//!
//! The substrate (`nomograph-claim` 0.2+) is type-agnostic; it stores
//! whatever we hand it. Validation is synthesist's responsibility,
//! applied at the API boundary inside `SynthStore::append_validated`
//! (or any CLI entry that constructs a typed claim before persisting).
//!
//! Adding a new claim type: create `schema/<name>.rs`, add `pub mod
//! <name>` below, add a match arm in `validate_claim`, and reference
//! the same module's enum constants from `cli.rs` via
//! `clap::builder::PossibleValuesParser::new(crate::schema::<name>::STATUSES)`.

use nomograph_claim::{Claim, ClaimType, SchemaError, SchemaResult};
use serde_json::Value;

pub mod campaign;
pub mod discovery;
pub mod outcome;
pub mod phase;
pub mod session;
pub mod spec;
pub mod task;
pub mod tree;

/// Validate `props` for the given `claim_type` on the WRITE PATH.
///
/// Strict by design. Synthesist's API boundary should reject claim
/// types it does not have a validator for, even if the substrate
/// would accept them. This defends against agents (LLM or otherwise)
/// hallucinating claim types that don't exist or writing garbage
/// into types whose schema we cannot enforce. The substrate stays
/// type-agnostic; the strictness is a synthesist policy, applied
/// where synthesist is the entry point.
///
/// Lattice and the reserved coordination types (intent, heartbeat,
/// directive) have no synthesist-side validator. Writes for those
/// types through synthesist are rejected. When lattice ships its
/// own CLI, that binary becomes the validating boundary for
/// stakeholder/topic/signal/disposition; coordination types start
/// being writable when their first consumer ships with a validator.
pub fn validate_props(claim_type: &ClaimType, props: &Value) -> SchemaResult<()> {
    // Accept both v2 (bare keys) and v3 (synthesist:-prefixed JSON-LD
    // keys) shapes. The per-type validators read bare keys; we
    // normalize before dispatching so the 8 validators stay
    // unchanged through the v2-to-v3 migration.
    let normalized = normalize_jsonld_props(props);
    let props = normalized.as_ref().unwrap_or(props);
    match claim_type {
        ClaimType::Tree => tree::validate(props),
        ClaimType::Spec => spec::validate(props),
        ClaimType::Task => task::validate(props),
        ClaimType::Discovery => discovery::validate(props),
        ClaimType::Campaign => campaign::validate(props),
        ClaimType::Session => session::validate(props),
        ClaimType::Phase => phase::validate(props),
        ClaimType::Outcome => outcome::validate(props),
        ClaimType::Intent
        | ClaimType::Heartbeat
        | ClaimType::Directive => Err(SchemaError::Other {
            claim_type: claim_type.as_str().to_string(),
            message:
                "claim_type is reserved for future coordination protocol; synthesist has no validator yet"
                    .to_string(),
        }),
        ClaimType::Stakeholder | ClaimType::Topic | ClaimType::Signal | ClaimType::Disposition => {
            Err(SchemaError::Other {
                claim_type: claim_type.as_str().to_string(),
                message: "claim_type is owned by `lattice`; use the lattice CLI to write it"
                    .to_string(),
            })
        }
    }
}

/// Result of the lenient read-path validator used by `synthesist check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationOutcome {
    /// Synthesist owns this claim type and the props validate.
    Ok,
    /// Synthesist owns this claim type and the props fail.
    SchemaFail(SchemaError),
    /// Synthesist does not own this claim type. The claim was likely
    /// written by another consumer (lattice, future coordination
    /// protocol writers) or is a legacy claim from before
    /// type-agnostic writes were introduced. Not an error; surface as
    /// a warning so operators can see what's in their log.
    NotOwnedBySynthesist,
}

/// Validate an existing [`Claim`] read from disk on the READ PATH.
///
/// Tolerates claims of types synthesist does not own. Used by
/// `synthesist check` which walks the entire claim log; we want to
/// surface schema regressions for synthesist-owned claims without
/// crying about lattice claims that synthesist isn't responsible
/// for. The write path ([`validate_props`]) is the strict gate.
pub fn validate_claim(claim: &Claim) -> ValidationOutcome {
    let owned = matches!(
        claim.claim_type,
        ClaimType::Tree
            | ClaimType::Spec
            | ClaimType::Task
            | ClaimType::Discovery
            | ClaimType::Campaign
            | ClaimType::Session
            | ClaimType::Phase
            | ClaimType::Outcome
    );
    if !owned {
        return ValidationOutcome::NotOwnedBySynthesist;
    }
    match validate_props(&claim.claim_type, &claim.props) {
        Ok(()) => ValidationOutcome::Ok,
        Err(e) => ValidationOutcome::SchemaFail(e),
    }
}

/// Format a `SchemaError` as a human-friendly one-line string for CLI
/// output. Structured variants are preserved at the library boundary;
/// this is the formatting consumers see when an error rolls up to a
/// CLI invocation that doesn't otherwise pattern-match.
#[allow(dead_code)]
pub fn format_error(err: &SchemaError) -> String {
    err.to_string()
}

/// Module prefixes the validator strips during JSON-LD normalization.
const MODULE_PREFIXES: &[&str] = &["synthesist:", "nomograph:"];

/// Envelope predicates the validator drops during JSON-LD normalization.
const ENVELOPE_PREDICATES: &[&str] = &[
    "@context",
    "@id",
    "@type",
    "prov:generatedAtTime",
    "prov:wasAttributedTo",
    "prov:wasRevisionOf",
];

/// Normalize a v3 JSON-LD props object to v2 bare-key form. Returns
/// `None` for already-bare props so callers can skip the rewrite.
///
/// Drops envelope predicates (@id, @type, prov:*) and strips module
/// prefixes (synthesist:, nomograph:) from remaining keys. Lets the
/// per-type validators (task.rs, spec.rs, ...) stay in bare-key form
/// while validate_props accepts both v2 stores and v3 substrate writes.
fn normalize_jsonld_props(props: &Value) -> Option<Value> {
    let map = props.as_object()?;
    let needs_rewrite = map.keys().any(|k| {
        ENVELOPE_PREDICATES.iter().any(|p| k == p)
            || MODULE_PREFIXES.iter().any(|p| k.starts_with(p))
    });
    if !needs_rewrite {
        return None;
    }
    let mut out = serde_json::Map::with_capacity(map.len());
    for (k, v) in map {
        if ENVELOPE_PREDICATES.iter().any(|p| k == p) {
            continue;
        }
        let bare_key = MODULE_PREFIXES
            .iter()
            .find_map(|p| k.strip_prefix(*p).map(|s| s.to_string()))
            .unwrap_or_else(|| k.to_string());
        out.insert(bare_key, v.clone());
    }
    Some(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomograph_claim::ClaimType;
    use serde_json::json;

    #[test]
    fn bare_v2_tree_props_validate() {
        let p = json!({"name": "keaton", "description": "the harness"});
        assert!(validate_props(&ClaimType::Tree, &p).is_ok());
    }

    #[test]
    fn v3_jsonld_tree_props_validate() {
        let p = json!({
            "@id": "synthesist:claim/x",
            "@type": "synthesist:Tree",
            "prov:generatedAtTime": "2026-05-29T10:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:name": "keaton",
            "synthesist:description": "the harness"
        });
        assert!(validate_props(&ClaimType::Tree, &p).is_ok());
    }

    #[test]
    fn v3_jsonld_task_with_valid_enum_passes() {
        let p = json!({
            "@id": "synthesist:claim/t1",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T10:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:tree": "keaton",
            "synthesist:spec": "v3",
            "synthesist:id": "t1",
            "synthesist:summary": "Test",
            "synthesist:status": "pending"
        });
        assert!(validate_props(&ClaimType::Task, &p).is_ok());
    }

    #[test]
    fn v3_jsonld_task_with_invalid_enum_fails() {
        let p = json!({
            "@id": "synthesist:claim/t1",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T10:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:tree": "keaton",
            "synthesist:spec": "v3",
            "synthesist:id": "t1",
            "synthesist:summary": "Test",
            "synthesist:status": "not-a-status"
        });
        assert!(validate_props(&ClaimType::Task, &p).is_err());
    }

    #[test]
    fn normalize_returns_none_for_bare() {
        let p = json!({"name": "keaton", "status": "active"});
        assert!(normalize_jsonld_props(&p).is_none());
    }

    #[test]
    fn normalize_strips_envelope_predicates() {
        let p = json!({
            "@id": "synthesist:claim/x",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
            "synthesist:status": "pending"
        });
        let n = normalize_jsonld_props(&p).unwrap();
        let obj = n.as_object().unwrap();
        assert!(obj.contains_key("status"));
        assert!(!obj.contains_key("@id"));
        assert!(!obj.contains_key("prov:generatedAtTime"));
    }

    #[test]
    fn normalize_strips_module_prefixes() {
        let p = json!({
            "@id": "synthesist:claim/x",
            "synthesist:goal": "ship v3",
            "nomograph:parentAsserter": "asserter:user:local:agd"
        });
        let n = normalize_jsonld_props(&p).unwrap();
        let obj = n.as_object().unwrap();
        assert!(obj.contains_key("goal"));
        assert!(obj.contains_key("parentAsserter"));
    }
}

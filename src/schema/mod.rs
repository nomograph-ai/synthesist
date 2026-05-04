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

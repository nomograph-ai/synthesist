//! Synthesist schema layer — thin delegate to `nomograph-claim::schema`.
//!
//! v1 held SQLite DDL here; v2 has none because `view.sqlite` is a
//! cache rebuilt from the claim log. This module exposes just the
//! validation entry point so command handlers can early-reject
//! invalid props before calling [`SynthStore::append`](crate::store::SynthStore::append).

use anyhow::{Context, Result};
use nomograph_claim::{schema as claim_schema, Claim, ClaimType};
use serde_json::Value;

/// Validate `props` for the given claim type. Delegates to
/// [`nomograph_claim::schema::validate_claim`] with a transient
/// asserter. Errors are prescriptive.
pub fn validate(claim_type: ClaimType, props: &Value) -> Result<()> {
    let probe = Claim::new(claim_type, props.clone(), "user:validate");
    claim_schema::validate_claim(&probe).context("validate props")?;
    Ok(())
}

//! Synthesist's Store surface.
//!
//! Wraps [`nomograph_workflow::Store`] with synthesist-side schema
//! validation at the API boundary. Every typed `append` runs the
//! claim through [`crate::schema::validate_claim`] before delegating
//! to the substrate, so consumers get a structured `SchemaError` (via
//! `anyhow`'s formatting at the binary edge) without garbage entering
//! the claim log.
//!
//! The substrate (`nomograph-claim` 0.2+) is type-agnostic for
//! validation; the workflow layer delegates the responsibility up,
//! and this is where it lands.
//!
//! Existing call sites that did `store.append(...)` continue to work
//! unchanged — `SynthStore` provides the same signature. Read-only
//! methods (`query`, `root`, `inner`, `sync_view`, `with_asserter`)
//! transparently delegate to the wrapped workflow store via `Deref`.

use std::ops::{Deref, DerefMut};
use std::path::Path;

use anyhow::{Context, Result};
use nomograph_claim::{ClaimId, ClaimType};
use serde_json::Value;

pub use nomograph_workflow::{
    CLAIMS_DIR, find_legacy_v1_db, json_out, legacy_migration_error, parse_tree_spec, today,
};

/// Synthesist-flavored Store: workflow's CRDT-backed Store with the
/// synthesist schema validator applied at every `append`. Existing
/// call sites work through `Deref` for read-only operations and
/// through the inherent `append` below for writes.
pub struct SynthStore {
    inner: nomograph_workflow::Store,
}

#[allow(dead_code)]
impl SynthStore {
    pub fn discover() -> Result<Self> {
        Ok(Self {
            inner: nomograph_workflow::Store::discover()?,
        })
    }

    pub fn discover_from(start: &Path) -> Result<Self> {
        Ok(Self {
            inner: nomograph_workflow::Store::discover_from(start)?,
        })
    }

    pub fn discover_for(session: &Option<String>) -> Result<Self> {
        Ok(Self {
            inner: nomograph_workflow::Store::discover_for(session)?,
        })
    }

    pub fn open_at(claims_dir: &Path) -> Result<Self> {
        Ok(Self {
            inner: nomograph_workflow::Store::open_at(claims_dir)?,
        })
    }

    pub fn init_at(claims_dir: &Path) -> Result<Self> {
        Ok(Self {
            inner: nomograph_workflow::Store::init_at(claims_dir)?,
        })
    }

    pub fn with_asserter(mut self, asserted_by: impl Into<String>) -> Self {
        self.inner = self.inner.with_asserter(asserted_by);
        self
    }

    /// Append a typed claim. Validates `props` against the synthesist
    /// schema for `claim_type` before persisting. Returns the new
    /// claim id on success or a structured schema error on rejection.
    ///
    /// Validation runs at this synthesist boundary because the
    /// workflow layer (and the substrate beneath it) is type-agnostic
    /// since v0.2.0. The same `crate::schema::<type>::*` constants
    /// drive both this validator and the CLI's clap parsers, so
    /// CLI-accepts-iff-schema-accepts is structural.
    pub fn append(
        &mut self,
        claim_type: ClaimType,
        props: Value,
        supersedes: Option<ClaimId>,
    ) -> Result<ClaimId> {
        crate::schema::validate_props(&claim_type, &props)
            .map_err(anyhow::Error::from)
            .context("validate claim before append")?;
        self.inner.append(claim_type, props, supersedes)
    }

    /// Replay an existing claim into the store without running
    /// synthesist's per-type validator.
    ///
    /// **Use this only for migration and import paths** — moving
    /// existing claims (from a v1 SQLite estate via `cmd_migrate`,
    /// from a JSON export via `cmd_import`) into the new store. New
    /// consumer-driven writes must go through [`Self::append`]
    /// instead, which is the strict-on-write boundary that defends
    /// against agents hallucinating fake claim types.
    ///
    /// The name carries the warning: this is replay, not creation.
    /// The substrate's structural checks (content hash, append
    /// lock, IO durability) still apply, so this is "skip domain
    /// validation," not "skip all validation."
    ///
    /// Visibility is `pub(crate)` to keep the bypass within
    /// synthesist's own modules — no external consumer should ever
    /// hold a `SynthStore` and reach for this.
    ///
    /// Per the claims-forward compat policy: new binaries must be
    /// able to read existing claim logs (including lattice and
    /// coordination types written by other consumers or migrated
    /// from v1). This is that read path's write side.
    pub(crate) fn append_replay(
        &mut self,
        claim_type: ClaimType,
        props: Value,
        supersedes: Option<ClaimId>,
    ) -> Result<ClaimId> {
        self.inner.append(claim_type, props, supersedes)
    }
}

impl Deref for SynthStore {
    type Target = nomograph_workflow::Store;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for SynthStore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Back-compat alias retained from the v2 rewrite. Prefer `SynthStore`
/// at call sites.
pub type Store = SynthStore;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    /// Method-resolution proof: `synth_store.append(...)` resolves to the
    /// inherent validating method on `SynthStore`, not the unvalidating
    /// one reachable through `Deref` to `nomograph_workflow::Store`.
    /// Rust's method resolution prefers inherent methods, but it's
    /// worth proving because the silent-fall-through to the workflow
    /// layer would be exactly the regression that the SynthStore
    /// wrapper exists to prevent.
    #[test]
    fn append_inherent_method_runs_validation() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter("user:local:test:t1");
        // Bad spec: missing required `goal`. If validation runs, this
        // returns Err with a structured SchemaError. If Deref shadowed
        // the inherent method, the unvalidating workflow::Store::append
        // would let it through and we'd get Ok.
        let bad = json!({
            "tree": "k",
            "id": "x",
            "status": "active",
            "topics": ["x"],
        });
        let err = store.append(ClaimType::Spec, bad, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("validate claim before append")
                || msg.contains("goal"),
            "expected validator error, got: {msg}"
        );
    }

    /// Strict-on-write: synthesist rejects appends for claim types it
    /// does not own (lattice or coordination types). This is the
    /// hallucination-defense from the adversarial review: agents that
    /// invent fake claim types get a clear rejection at the synthesist
    /// boundary instead of writing nonsense into the substrate.
    #[test]
    fn append_rejects_unowned_claim_types() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter("user:local:test:t1");
        for unowned in [
            ClaimType::Stakeholder,
            ClaimType::Topic,
            ClaimType::Signal,
            ClaimType::Disposition,
            ClaimType::Intent,
            ClaimType::Heartbeat,
            ClaimType::Directive,
        ] {
            let result = store.append(unowned.clone(), json!({}), None);
            assert!(
                result.is_err(),
                "synthesist must reject claim_type {unowned:?} at write boundary"
            );
        }
    }

    /// `append_replay` deliberately bypasses the synthesist
    /// validator for migration / import paths. The structural checks
    /// in the substrate (content hash, append lock) still run, but
    /// per-type schema validation is skipped. Verifying that the
    /// bypass actually bypasses, so we can move existing claims of
    /// any type without the strict-on-write gate.
    #[test]
    fn append_replay_skips_synthesist_validator() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter("user:local:test:t1");
        // A Stakeholder claim with empty props would be rejected by
        // both the synthesist write validator (unowned type) and any
        // future lattice validator (missing required fields). The
        // unvalidated path just stores it, which is what import wants.
        let id = store
            .append_replay(ClaimType::Stakeholder, json!({"id": "alice"}), None)
            .expect("unvalidated append accepts unowned types");
        assert!(!id.is_empty());
    }
}

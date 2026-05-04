//! Physical compaction of the on-disk claim log.
//!
//! Delegates to [`nomograph_claim::Store::compact`]: writes
//! `snapshot.amc` containing the full Automerge document, deletes
//! superseded `changes/*.amc` files, all under the substrate
//! `claims/.lock`. Logical claim history in the Automerge document is
//! unchanged — compaction is a physical re-encoding of incremental
//! changes into a snapshot, not a semantic GC.
//!
//! Trial impact, anonymized large estate: working tree under
//! `claims/` shrank from ~7.6 GB (incremental change files) to
//! ~5.8 MB (single snapshot) with no logical state change. The
//! shrink is the encoding difference, not data loss.
//!
//! Reference implementation and trial methodology:
//! `nomograph/synthesist!8` by Josh Meekhof (issue #7). The
//! design — `ClaimCompaction` trait separating the concern from
//! `SynthStore`, the operational `--dry-run` and `--yes` safety
//! belts in the CLI surface, the script-driven trial that proves
//! safety on a copy before touching a canonical estate — comes
//! from his work. See CHANGELOG entry for v2.4.0 for full
//! attribution.
//!
//! Operator considerations:
//!
//! - **Cost**: compaction is heavy (rewrites the snapshot). Prefer
//!   quiet windows.
//! - **Concurrency**: substrate's `claims/.lock` flock serializes
//!   against concurrent appends. A running CLI write blocks
//!   compaction; compaction blocks writes.
//! - **Recovery**: change files are git-tracked; even after sweep,
//!   a `git checkout` can resurrect the pre-compaction state.
//!   Compaction is fully reversible from version control.

use anyhow::{Context, Result};

use crate::store::SynthStore;

/// Compact incremental change files into a local snapshot.
///
/// See [`nomograph_claim::Store::compact`] for the substrate-side
/// semantics: the full Automerge doc is serialized to `snapshot.amc`,
/// and superseded `changes/*.amc` files are removed under the
/// directory lock. This is purely a physical re-encoding — the
/// logical claim graph (including history) is unchanged.
pub trait ClaimCompaction {
    fn compact_claim_log(&mut self) -> Result<()>;
}

impl ClaimCompaction for SynthStore {
    fn compact_claim_log(&mut self) -> Result<()> {
        self.inner()
            .compact()
            .map_err(anyhow::Error::from)
            .context("compact claim log")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ClaimCompaction;
    use crate::store::SynthStore;
    use tempfile::tempdir;

    #[test]
    fn compact_smoke_empty_estate() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims).unwrap();
        store.compact_claim_log().unwrap();
    }

    #[test]
    fn compact_handles_non_empty_estate() {
        // Strengthens MR !8's empty-estate smoke: writes a few claims
        // through SynthStore::append (which validates), runs
        // compaction, then verifies the store still opens and the
        // claims are intact.
        use nomograph_claim::ClaimType;
        use serde_json::json;
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter("user:local:test:t1");
        store
            .append(
                ClaimType::Tree,
                json!({"name": "k", "description": ""}),
                None,
            )
            .unwrap();
        store
            .append(
                ClaimType::Spec,
                json!({"tree": "k", "id": "x", "goal": "g", "status": "active", "topics": ["x"]}),
                None,
            )
            .unwrap();
        store.compact_claim_log().unwrap();
        // Re-open after compaction; should succeed and surface the
        // same logical claims.
        let store2 = SynthStore::open_at(&claims).unwrap();
        let rows = store2
            .query(
                "SELECT count(*) AS n FROM claims WHERE claim_type IN ('tree', 'spec')",
                &[],
            )
            .unwrap();
        let n = rows[0]
            .get("n")
            .and_then(|v| v.as_i64())
            .unwrap_or_default();
        assert_eq!(n, 2, "claims survive compaction");
    }
}

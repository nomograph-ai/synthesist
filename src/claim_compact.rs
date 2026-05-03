//! Physical compaction of the on-disk claim log.
//!
//! Delegates to [`nomograph_claim::Store::compact`]: writes `snapshot.amc`,
//! deletes superseded `changes/*.amc`, under the substrate `claims/.lock`.
//! Logical claim history in the Automerge document is unchanged.

use anyhow::{Context, Result};

use crate::store::SynthStore;

/// Compact incremental change files into a local snapshot (see nomograph-claim docs).
pub trait ClaimCompaction {
    /// Serialize the full Automerge document to `snapshot.amc` and remove
    /// superseded `changes/*.amc` files. Concurrent [`Store::append`](nomograph_claim::Store::append)
    /// calls serialize on the same advisory directory lock.
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
}

//! TODO PATH-B: compaction was a v2 Automerge concern. v3 has no
//! equivalent (the JSON-LD log is plain append; the graph view's
//! snapshot cache is automatic). Module is retained as a no-op shim
//! so existing `mod compaction;` references compile.

#![allow(dead_code)]

use anyhow::Result;

use crate::store::SynthStore;

pub trait ClaimCompaction {
    fn compact_claim_log(&mut self) -> Result<()>;
}

impl ClaimCompaction for SynthStore {
    fn compact_claim_log(&mut self) -> Result<()> {
        // No-op in v3: there is no Automerge snapshot to rewrite.
        Ok(())
    }
}

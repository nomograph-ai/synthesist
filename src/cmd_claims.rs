//! `claims` CLI — on-disk claim store maintenance (compaction).

use anyhow::Result;
use serde_json::json;

use crate::claim_compact::ClaimCompaction;
use crate::cli::ClaimsCmd;
use crate::store::{SynthStore, json_out};

pub fn run(cmd: &ClaimsCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        ClaimsCmd::Compact => cmd_compact(session),
    }
}

fn cmd_compact(session: &Option<String>) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    store.compact_claim_log()?;
    let root = store.root().display().to_string();
    json_out(&json!({ "ok": true, "claims_root": root }))
}

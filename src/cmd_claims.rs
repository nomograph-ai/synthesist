//! `claims` CLI surface (Path B Stage 1).
//!
//! TODO PATH-B: `claims compact` was specific to the v2 Automerge
//! substrate (compact `claims/changes/*.amc` into snapshot.amc). v3
//! has no compaction model -- the JSON-LD log is append-only and the
//! view rebuild handles read efficiency. Subsequent agent removes the
//! command or replaces with a v3 maintenance operation.

use anyhow::Result;
use serde_json::json;

use crate::cli::ClaimsCmd;
use crate::store::json_out;

pub fn run(cmd: &ClaimsCmd, _session: &Option<String>) -> Result<()> {
    match cmd {
        ClaimsCmd::Compact { .. } => {
            json_out(&json!({
                "ok": true,
                "todo_path_b": "claims compact retired in Path B (v2 substrate concept)"
            }))
        }
    }
}

//! `synthesist export` -- emit the claim log as JSON.
//!
//! TODO PATH-B: re-implement on `SynthStore::iter_claims`. The output
//! shape was a JSON array of {id, claim_type, props, asserted_at,
//! supersedes, parent_asserter, asserted_by} records lifted from the
//! v2 Automerge store. v3 has the same info on each log line; the
//! reformat is straightforward but not yet wired.

use anyhow::Result;
use serde_json::json;

use crate::store::{SynthStore, json_out};

pub fn cmd_export() -> Result<()> {
    let store = SynthStore::discover()?;
    let claims: Vec<_> = store.iter_claims()?.collect();
    let count = claims.len();
    json_out(&json!({
        "claims": claims,
        "count": count,
        "todo_path_b": "cmd_export emits raw v3 JSON-LD docs; v2-shape projection not yet ported"
    }))
}

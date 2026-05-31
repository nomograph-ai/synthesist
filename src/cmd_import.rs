//! `synthesist import` -- replays a `cmd_export` dump into the substrate.
//!
//! TODO PATH-B: rewire on top of `SynthStore::append_replay`. The v2
//! version walked `claims_raw` and `inner().append(claim)` to preserve
//! the original ids and asserters; the v3 equivalent reconstructs the
//! JSON-LD document per line and appends via the LogWriter.

use anyhow::Result;
use serde_json::json;

use crate::store::json_out;

pub fn cmd_import(_file: &Option<String>) -> Result<()> {
    anyhow::bail!("cmd_import: TODO PATH-B (v3-native import not yet wired)");

    #[allow(unreachable_code)]
    json_out(&json!({}))
}

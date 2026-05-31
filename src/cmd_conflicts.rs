//! `synthesist conflicts` -- surface diamond conflicts in the claim log.
//!
//! TODO PATH-B: port to v3 SPARQL. The check is `for each ?prior,
//! count distinct ?c where ?c synthesist:supersedes ?prior, return
//! ?prior with count > 1`. Returns empty conflict list for now.

use anyhow::Result;
use serde_json::json;

use crate::store::json_out;

pub fn cmd_conflicts() -> Result<()> {
    json_out(&json!({
        "conflicts": [],
        "todo_path_b": "cmd_conflicts not yet ported to v3 SPARQL"
    }))
}

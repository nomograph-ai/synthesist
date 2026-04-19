//! Ad-hoc SQL query command (read-only) over the claim view.
//!
//! v2: a thin wrapper around [`SynthStore::query`], which itself
//! delegates to [`nomograph_claim::View::query`]. The view already
//! rejects non-`SELECT/WITH/PRAGMA` statements with a prescriptive
//! error; we catch the error and re-emit a synthesist-branded message
//! so the CLI experience doesn't leak `View::query` internals.

use anyhow::{bail, Result};
use serde_json::json;

use crate::store::{json_out, SynthStore};

/// SQL keywords we reject up front — the underlying view does the
/// same, but a pre-check produces a clearer error (and short-circuits
/// before touching the database).
const WRITE_KEYWORDS: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "DROP", "CREATE", "ALTER", "REPLACE", "TRUNCATE",
];

pub fn cmd_sql(query: &str) -> Result<()> {
    // Reject writes with a specific message before we hit the view.
    let first_word = query
        .trim_start()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_uppercase();
    if WRITE_KEYWORDS.contains(&first_word.as_str()) {
        bail!(
            "synthesist sql is read-only (rejected `{first_word}`). \
             The claim substrate is append-only: use the CLI \
             (tree/spec/task/discovery/etc.) to record new facts; \
             SQL is for reading the view only."
        );
    }

    let store = SynthStore::discover()?;
    let rows = store.query(query, &[])?;

    // Pull column names from the first row so `count: 0` queries still
    // emit a `columns: []` shape the caller can rely on.
    let columns: Vec<String> = rows
        .first()
        .and_then(|v| v.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();

    let count = rows.len();
    json_out(&json!({
        "columns": columns,
        "rows": rows,
        "count": count,
    }))
}

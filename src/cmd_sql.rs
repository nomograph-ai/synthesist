//! TODO PATH-B: `synthesist sql` was a thin wrapper around the v2
//! SQLite view's `query` method. Path B retires the SQLite view; the
//! v3 substitute is `synthesist query --sparql ...` (already
//! SPARQL-native via `cmd_query`). Subsequent agent decides whether
//! to keep `sql` as an alias or remove the surface entirely.

use anyhow::Result;

pub fn cmd_sql(_query: &str) -> Result<()> {
    anyhow::bail!(
        "synthesist sql: retired in Path B; use `synthesist query --sparql ...` instead"
    )
}

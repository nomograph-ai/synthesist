//! Ad-hoc SQL query command (read-only).

use anyhow::{bail, Result};
use serde_json::json;

use crate::store::{json_out, Store};

pub fn cmd_sql(query: &str) -> Result<()> {
    // Validate query is read-only: reject anything that isn't SELECT, EXPLAIN, or PRAGMA.
    let trimmed = query.trim_start();
    let first_word = trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_uppercase();
    if !matches!(first_word.as_str(), "SELECT" | "EXPLAIN" | "PRAGMA" | "WITH") {
        bail!("synthesist sql only allows read-only queries (SELECT, EXPLAIN, PRAGMA, WITH). Use the CLI commands for writes.");
    }

    // Open database in read-only mode as defense in depth.
    let store = Store::discover()?;

    let mut stmt = store.conn.prepare(query)?;
    let col_count = stmt.column_count();
    let col_names: Vec<String> = (0..col_count)
        .map(|i| stmt.column_name(i).unwrap().to_string())
        .collect();

    let rows: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            let mut m = serde_json::Map::new();
            for (i, name) in col_names.iter().enumerate() {
                let val: rusqlite::types::Value = row.get(i)?;
                m.insert(
                    name.clone(),
                    match val {
                        rusqlite::types::Value::Null => serde_json::Value::Null,
                        rusqlite::types::Value::Integer(n) => json!(n),
                        rusqlite::types::Value::Real(f) => json!(f),
                        rusqlite::types::Value::Text(s) => json!(s),
                        rusqlite::types::Value::Blob(_) => json!("<blob>"),
                    },
                );
            }
            Ok(serde_json::Value::Object(m))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;

    json_out(&json!({"columns": col_names, "rows": rows, "count": rows.len()}))
}

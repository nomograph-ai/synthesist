//! Export and import commands.

use std::io::Read;

use anyhow::Result;
use serde_json::json;

use crate::store::{json_out, Store};

/// All tables to export, in dependency order.
const EXPORT_TABLES: &[&str] = &[
    "trees",
    "specs",
    "tasks",
    "task_deps",
    "task_files",
    "acceptance",
    "discoveries",
    "stakeholders",
    "stakeholder_orgs",
    "dispositions",
    "signals",
    "campaign_active",
    "campaign_backlog",
    "campaign_blocked_by",
    "session_meta",
    "phase",
    "config",
];

pub fn cmd_export() -> Result<()> {
    let store = Store::discover()?;
    let mut result = serde_json::Map::new();
    result.insert("version".into(), json!("1"));
    result.insert("exported".into(), json!(Store::today()));

    for table in EXPORT_TABLES {
        let mut stmt = store.conn.prepare(&format!("SELECT * FROM [{table}]"))?;
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
                            rusqlite::types::Value::Blob(b) => {
                                json!(base64_encode(&b))
                            }
                        },
                    );
                }
                Ok(serde_json::Value::Object(m))
            })?
            .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;

        result.insert(table.to_string(), json!(rows));
    }

    json_out(&serde_json::Value::Object(result))
}

pub fn cmd_import(file: &Option<String>) -> Result<()> {
    let store = Store::discover()?;

    let json_str = match file {
        Some(path) => std::fs::read_to_string(path)?,
        None => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };

    let data: serde_json::Value = serde_json::from_str(&json_str)?;
    let obj = data.as_object().ok_or_else(|| anyhow::anyhow!("expected JSON object"))?;

    let mut imported = serde_json::Map::new();

    // Disable FK checks during import (data may arrive out of dependency order).
    // Re-enable after commit. FK integrity is verified by `synthesist check`.
    store.conn.execute("PRAGMA foreign_keys = OFF", [])?;

    // Wrap entire import in a transaction for atomicity and performance.
    store.conn.execute("BEGIN IMMEDIATE", [])?;

    let result = (|| -> Result<()> {
    for table in EXPORT_TABLES {
        if let Some(rows) = obj.get(*table).and_then(|v| v.as_array()) {
            if rows.is_empty() {
                continue;
            }

            // Get column names from first row, bracket-quote to prevent injection.
            let first = rows[0].as_object().ok_or_else(|| anyhow::anyhow!("expected object in {table}"))?;
            let cols: Vec<&String> = first.keys().collect();
            let placeholders: Vec<String> = (1..=cols.len()).map(|i| format!("?{i}")).collect();
            let col_list: Vec<String> = cols.iter().map(|s| format!("[{}]", s)).collect();

            let sql = format!(
                "INSERT OR REPLACE INTO [{table}] ({}) VALUES ({})",
                col_list.join(", "),
                placeholders.join(", "),
            );

            let mut count = 0;
            for row in rows {
                let row_obj = row.as_object().ok_or_else(|| anyhow::anyhow!("expected object"))?;
                let values: Vec<Box<dyn rusqlite::types::ToSql>> = cols
                    .iter()
                    .map(|col| -> Box<dyn rusqlite::types::ToSql> {
                        match row_obj.get(*col) {
                            Some(serde_json::Value::String(s)) => Box::new(s.clone()),
                            Some(serde_json::Value::Number(n)) => {
                                if let Some(i) = n.as_i64() {
                                    Box::new(i)
                                } else if let Some(f) = n.as_f64() {
                                    Box::new(f)
                                } else {
                                    Box::new(rusqlite::types::Null)
                                }
                            }
                            Some(serde_json::Value::Null) | None => {
                                Box::new(rusqlite::types::Null)
                            }
                            Some(other) => Box::new(other.to_string()),
                        }
                    })
                    .collect();

                let refs: Vec<&dyn rusqlite::types::ToSql> =
                    values.iter().map(|v| v.as_ref()).collect();
                store.conn.execute(&sql, refs.as_slice())?;
                count += 1;
            }

            imported.insert(table.to_string(), json!(count));
        }
    }
    Ok(())
    })();

    match result {
        Ok(()) => {
            store.conn.execute("COMMIT", [])?;
            store.conn.execute("PRAGMA foreign_keys = ON", [])?;
        }
        Err(e) => {
            store.conn.execute("ROLLBACK", []).ok();
            store.conn.execute("PRAGMA foreign_keys = ON", []).ok();
            return Err(e);
        }
    };

    json_out(&json!({"status": "complete", "imported": imported}))
}

fn base64_encode(data: &[u8]) -> String {
    // Simple base64 without external dependency
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((n >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

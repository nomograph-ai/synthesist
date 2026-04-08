//! Tree commands.

use anyhow::Result;
use serde_json::json;

use crate::cli::TreeCmd;
use crate::store::{json_out, Store};

pub fn run(cmd: &TreeCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        TreeCmd::Add {
            name,
            description,
            status,
        } => cmd_tree_add(name, description, status, session),
        TreeCmd::List => cmd_tree_list(session),
    }
}

fn cmd_tree_add(name: &str, description: &str, status: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    store.conn.execute(
        "INSERT INTO trees (name, status, description) VALUES (?1, ?2, ?3)",
        rusqlite::params![name, status, description],
    )?;
    json_out(&json!({"name": name, "status": status, "description": description}))
}

fn cmd_tree_list(session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let mut stmt = store
        .conn
        .prepare("SELECT name, status, description FROM trees ORDER BY name")?;
    let trees: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            Ok(json!({
                "name": row.get::<_, String>(0)?,
                "status": row.get::<_, String>(1)?,
                "description": row.get::<_, String>(2)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    json_out(&json!({"trees": trees}))
}

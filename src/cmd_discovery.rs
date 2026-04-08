//! Discovery commands.

use anyhow::Result;
use serde_json::json;

use crate::cli::DiscoveryCmd;
use crate::store::{json_out, parse_tree_spec, Store};

pub fn run(cmd: &DiscoveryCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        DiscoveryCmd::Add {
            tree_spec,
            finding,
            impact,
            action,
            author,
            date,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_add(tree, spec, finding, impact.as_deref(), action.as_deref(), author.as_deref(), date.as_deref(), session)
        }
        DiscoveryCmd::List { tree_spec } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_list(tree, spec, session)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_add(
    tree: &str,
    spec: &str,
    finding: &str,
    impact: Option<&str>,
    action: Option<&str>,
    author: Option<&str>,
    date: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    let store = Store::discover_for(session)?;
    let id = store.next_id("discoveries", tree, spec, "d")?;
    let date = date.unwrap_or(&Store::today()).to_string();
    store.conn.execute(
        "INSERT INTO discoveries (tree, spec, id, date, author, finding, impact, action) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![tree, spec, id, date, author, finding, impact, action],
    )?;
    json_out(&json!({"id": id, "finding": finding, "date": date}))
}

fn cmd_list(tree: &str, spec: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let mut stmt = store.conn.prepare(
        "SELECT id, date, author, finding, impact, action FROM discoveries WHERE tree = ?1 AND spec = ?2 ORDER BY date DESC, id",
    )?;
    let discoveries: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params![tree, spec], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "date": row.get::<_, String>(1)?,
                "author": row.get::<_, Option<String>>(2)?,
                "finding": row.get::<_, String>(3)?,
                "impact": row.get::<_, Option<String>>(4)?,
                "action": row.get::<_, Option<String>>(5)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    json_out(&json!({"tree": tree, "spec": spec, "discoveries": discoveries}))
}

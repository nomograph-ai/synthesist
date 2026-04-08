//! Campaign commands.

use anyhow::Result;
use serde_json::json;

use crate::cli::CampaignCmd;
use crate::store::{json_out, Store};

pub fn run(cmd: &CampaignCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        CampaignCmd::Add {
            tree,
            spec_id,
            summary,
            phase,
            backlog,
            title,
            blocked_by,
        } => {
            if *backlog {
                cmd_campaign_backlog(tree, spec_id, title.as_deref().unwrap_or(""), summary, session)
            } else {
                cmd_campaign_active(tree, spec_id, summary, phase.as_deref(), session)
            }?;
            // Add blocked_by entries
            if !blocked_by.is_empty() {
                let store = Store::discover_for(session)?;
                for dep in blocked_by {
                    if dep.is_empty() {
                        continue;
                    }
                    store.conn.execute(
                        "INSERT OR IGNORE INTO campaign_blocked_by (tree, spec_id, blocked_by) VALUES (?1, ?2, ?3)",
                        rusqlite::params![tree, spec_id, dep],
                    )?;
                }
            }
            Ok(())
        }
        CampaignCmd::List { tree } => cmd_campaign_list(tree, session),
    }
}

fn cmd_campaign_active(tree: &str, spec_id: &str, summary: &str, phase: Option<&str>, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    store.conn.execute(
        "INSERT OR REPLACE INTO campaign_active (tree, spec_id, summary, phase) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![tree, spec_id, summary, phase],
    )?;
    json_out(&json!({"tree": tree, "spec_id": spec_id, "list": "active"}))
}

fn cmd_campaign_backlog(tree: &str, spec_id: &str, title: &str, summary: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    store.conn.execute(
        "INSERT OR REPLACE INTO campaign_backlog (tree, spec_id, title, summary) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![tree, spec_id, title, summary],
    )?;
    json_out(&json!({"tree": tree, "spec_id": spec_id, "list": "backlog"}))
}

fn cmd_campaign_list(tree: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;

    let mut stmt = store.conn.prepare(
        "SELECT spec_id, summary, phase FROM campaign_active WHERE tree = ?1 ORDER BY spec_id",
    )?;
    let active: Vec<serde_json::Value> = stmt
        .query_map([tree], |row| {
            Ok(json!({
                "spec_id": row.get::<_, String>(0)?,
                "summary": row.get::<_, String>(1)?,
                "phase": row.get::<_, Option<String>>(2)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;

    let mut stmt = store.conn.prepare(
        "SELECT spec_id, title, summary FROM campaign_backlog WHERE tree = ?1 ORDER BY spec_id",
    )?;
    let backlog: Vec<serde_json::Value> = stmt
        .query_map([tree], |row| {
            Ok(json!({
                "spec_id": row.get::<_, String>(0)?,
                "title": row.get::<_, String>(1)?,
                "summary": row.get::<_, String>(2)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;

    json_out(&json!({"tree": tree, "active": active, "backlog": backlog}))
}

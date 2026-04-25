//! Tree commands — ported to the claim substrate.

use anyhow::{Result, anyhow};
use nomograph_claim::ClaimType;
use serde_json::json;

use crate::cli::TreeCmd;
use crate::store::{SynthStore, json_out};

pub fn run(cmd: &TreeCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        TreeCmd::Add {
            name,
            description,
            status,
        } => cmd_tree_add(name, description, status, session),
        TreeCmd::List => cmd_tree_list(session),
        TreeCmd::Show { name } => cmd_tree_show(name, session),
    }
}

fn cmd_tree_add(
    name: &str,
    description: &str,
    status: &str,
    session: &Option<String>,
) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let props = json!({ "name": name, "description": description });
    store.append(ClaimType::Tree, props, None)?;
    json_out(&json!({
        "name": name,
        "status": status,
        "description": description,
    }))
}

fn cmd_tree_list(session: &Option<String>) -> Result<()> {
    let store = SynthStore::discover_for(session)?;
    let rows = store.query(
        "SELECT props FROM claims WHERE claim_type = 'tree' ORDER BY asserted_at",
        &[],
    )?;
    let trees: Vec<serde_json::Value> = rows
        .into_iter()
        .filter_map(|row| {
            let props_str = row.get("props")?.as_str()?.to_string();
            let props: serde_json::Value = serde_json::from_str(&props_str).ok()?;
            Some(json!({
                "name": props.get("name").cloned().unwrap_or_default(),
                "status": "active",
                "description": props.get("description").cloned().unwrap_or_default(),
            }))
        })
        .collect();
    json_out(&json!({ "trees": trees }))
}

fn cmd_tree_show(name: &str, session: &Option<String>) -> Result<()> {
    let store = SynthStore::discover_for(session)?;

    let tree_rows = store.query(
        "SELECT props FROM claims \
         WHERE claim_type = 'tree' AND json_extract(props, '$.name') = ?1 \
         ORDER BY asserted_at DESC LIMIT 1",
        &[&name],
    )?;
    let tree_row = tree_rows
        .first()
        .ok_or_else(|| anyhow!("tree not found: {name}. Try `synthesist tree list`."))?;
    let props_str = tree_row
        .get("props")
        .and_then(|v| v.as_str())
        .unwrap_or("{}");
    let props: serde_json::Value = serde_json::from_str(props_str).unwrap_or(json!({}));

    let spec_count_rows = store.query(
        "SELECT COUNT(*) AS n FROM claims \
         WHERE claim_type = 'spec' AND json_extract(props, '$.tree') = ?1",
        &[&name],
    )?;
    let spec_count = spec_count_rows
        .first()
        .and_then(|r| r.get("n"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let session_count_rows = store.query(
        "SELECT COUNT(*) AS n FROM claims \
         WHERE claim_type = 'session' AND json_extract(props, '$.tree') = ?1",
        &[&name],
    )?;
    let session_count = session_count_rows
        .first()
        .and_then(|r| r.get("n"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    json_out(&json!({
        "name": props.get("name").cloned().unwrap_or_else(|| json!(name)),
        "description": props.get("description").cloned().unwrap_or_default(),
        "spec_count": spec_count,
        "session_count": session_count,
    }))
}

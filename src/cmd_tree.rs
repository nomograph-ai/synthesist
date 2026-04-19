//! Tree commands — ported to the claim substrate.

use anyhow::Result;
use nomograph_claim::ClaimType;
use serde_json::json;

use crate::cli::TreeCmd;
use crate::store::{json_out, SynthStore};

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

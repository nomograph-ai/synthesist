//! Spec commands.

use anyhow::{bail, Result};
use serde_json::json;

use crate::cli::SpecCmd;
use crate::store::{json_out, parse_tree_spec, Store};

pub fn run(cmd: &SpecCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        SpecCmd::Add {
            tree_spec,
            goal,
            constraints,
            decisions,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_spec_add(tree, spec, goal.as_deref(), constraints.as_deref(), decisions.as_deref(), session)
        }
        SpecCmd::Show { tree_spec } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_spec_show(tree, spec, session)
        }
        SpecCmd::Update {
            tree_spec,
            goal,
            constraints,
            decisions,
            status,
            outcome,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_spec_update(
                tree,
                spec,
                goal.as_deref(),
                constraints.as_deref(),
                decisions.as_deref(),
                status.as_deref(),
                outcome.as_deref(),
                session,
            )
        }
        SpecCmd::List { tree } => cmd_spec_list(tree, session),
    }
}

fn cmd_spec_add(
    tree: &str,
    spec: &str,
    goal: Option<&str>,
    constraints: Option<&str>,
    decisions: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    let store = Store::discover_for(session)?;
    let today = Store::today();
    // Auto-ensure parent tree exists (idempotent) for FK integrity.
    store.conn.execute(
        "INSERT OR IGNORE INTO trees (name) VALUES (?1)",
        rusqlite::params![tree],
    )?;
    store.conn.execute(
        "INSERT INTO specs (tree, id, goal, constraints, decisions, status, created) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6)",
        rusqlite::params![tree, spec, goal, constraints, decisions, today],
    )?;
    json_out(&json!({
        "tree": tree,
        "id": spec,
        "goal": goal,
        "status": "active",
        "created": today,
    }))
}

fn cmd_spec_show(tree: &str, spec: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let row = store.conn.query_row(
        "SELECT tree, id, goal, constraints, decisions, status, outcome, created FROM specs WHERE tree = ?1 AND id = ?2",
        rusqlite::params![tree, spec],
        |row| {
            Ok(json!({
                "tree": row.get::<_, String>(0)?,
                "id": row.get::<_, String>(1)?,
                "goal": row.get::<_, Option<String>>(2)?,
                "constraints": row.get::<_, Option<String>>(3)?,
                "decisions": row.get::<_, Option<String>>(4)?,
                "status": row.get::<_, String>(5)?,
                "outcome": row.get::<_, Option<String>>(6)?,
                "created": row.get::<_, Option<String>>(7)?,
            }))
        },
    );
    match row {
        Ok(v) => json_out(&v),
        Err(_) => bail!("spec not found: {tree}/{spec}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_spec_update(
    tree: &str,
    spec: &str,
    goal: Option<&str>,
    constraints: Option<&str>,
    decisions: Option<&str>,
    status: Option<&str>,
    outcome: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    let store = Store::discover_for(session)?;
    let mut sets = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    macro_rules! maybe_set {
        ($field:expr, $col:expr) => {
            if let Some(v) = $field {
                sets.push(format!("{} = ?{}", $col, idx));
                params.push(Box::new(v.to_string()));
                idx += 1;
            }
        };
    }

    maybe_set!(goal, "goal");
    maybe_set!(constraints, "constraints");
    maybe_set!(decisions, "decisions");
    maybe_set!(status, "status");
    maybe_set!(outcome, "outcome");

    if sets.is_empty() {
        bail!("no fields to update");
    }

    let sql = format!(
        "UPDATE specs SET {} WHERE tree = ?{} AND id = ?{}",
        sets.join(", "),
        idx,
        idx + 1
    );
    params.push(Box::new(tree.to_string()));
    params.push(Box::new(spec.to_string()));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let affected = store.conn.execute(&sql, param_refs.as_slice())?;
    if affected == 0 {
        bail!("spec not found: {tree}/{spec}");
    }
    json_out(&json!({"tree": tree, "id": spec, "updated": true}))
}

fn cmd_spec_list(tree: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let mut stmt = store.conn.prepare(
        "SELECT id, goal, status, outcome, created FROM specs WHERE tree = ?1 ORDER BY id",
    )?;
    let specs: Vec<serde_json::Value> = stmt
        .query_map([tree], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "goal": row.get::<_, Option<String>>(1)?,
                "status": row.get::<_, String>(2)?,
                "outcome": row.get::<_, Option<String>>(3)?,
                "created": row.get::<_, Option<String>>(4)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    json_out(&json!({"tree": tree, "specs": specs}))
}

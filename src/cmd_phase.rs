//! Phase enforcement commands.

use anyhow::{bail, Result};
use serde_json::json;

use crate::cli::PhaseCmd;
use crate::store::{json_out, Store};
use crate::types::Phase;

pub fn run(cmd: &PhaseCmd, force: bool) -> Result<()> {
    match cmd {
        PhaseCmd::Set { name } => cmd_phase_set(name, force),
        PhaseCmd::Show => cmd_phase_show(),
    }
}

fn cmd_phase_set(name: &str, force: bool) -> Result<()> {
    let target = Phase::from_str(name)
        .ok_or_else(|| anyhow::anyhow!("unknown phase: {name} (valid: orient, plan, agree, execute, reflect, replan, report)"))?;

    let store = Store::discover()?;

    // Read current phase and validate the transition.
    let current_str: String = store
        .conn
        .query_row("SELECT name FROM phase WHERE id = 1", [], |row| row.get(0))?;

    #[allow(clippy::collapsible_if)]
    if let Some(current) = Phase::from_str(&current_str) {
        if !force && !current.can_transition_to(target) {
            let valid: Vec<&str> = current
                .valid_transitions()
                .iter()
                .map(|p| p.as_str())
                .collect();
            bail!(
                "invalid phase transition: {current_str} -> {name} (valid: {})",
                if valid.is_empty() {
                    "none".to_string()
                } else {
                    valid.join(", ")
                }
            );
        }
    }

    store.conn.execute("UPDATE phase SET name = ?1 WHERE id = 1", [name])?;
    json_out(&json!({"phase": name}))
}

fn cmd_phase_show() -> Result<()> {
    let store = Store::discover()?;
    let phase: String = store
        .conn
        .query_row("SELECT name FROM phase WHERE id = 1", [], |row| row.get(0))?;
    json_out(&json!({"phase": phase}))
}

/// Check if an operation is allowed in the current phase.
/// Returns Ok(()) if allowed, or an error with explanation.
pub fn check_phase(store: &Store, top_cmd: &str, sub_cmd: &str, force: bool) -> Result<()> {
    if force {
        return Ok(());
    }

    let phase: String = store
        .conn
        .query_row("SELECT name FROM phase WHERE id = 1", [], |row| row.get(0))?;

    let violation = match phase.as_str() {
        "orient" => Some("no writes allowed in ORIENT phase"),
        "plan" => {
            if top_cmd == "task" && matches!(sub_cmd, "claim" | "done" | "block") {
                Some("cannot claim/complete tasks in PLAN phase")
            } else {
                None
            }
        }
        "agree" => Some("no operations in AGREE phase -- present plan and wait for human approval"),
        "execute" => {
            if top_cmd == "task" && matches!(sub_cmd, "add" | "cancel") {
                Some("cannot add/cancel tasks in EXECUTE phase -- transition to REPLAN")
            } else if top_cmd == "spec" && sub_cmd == "add" {
                Some("cannot add specs in EXECUTE phase -- transition to REPLAN")
            } else {
                None
            }
        }
        "reflect" => {
            if top_cmd == "task" && sub_cmd == "claim" {
                Some("cannot claim tasks in REFLECT phase")
            } else {
                None
            }
        }
        "replan" => {
            if top_cmd == "task" && sub_cmd == "claim" {
                Some("cannot claim tasks in REPLAN phase")
            } else {
                None
            }
        }
        "report" => Some("no writes allowed in REPORT phase"),
        _ => None,
    };

    if let Some(msg) = violation {
        bail!("phase violation ({phase}): {msg}");
    }
    Ok(())
}

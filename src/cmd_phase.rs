//! Phase enforcement commands — ported to the claim substrate (v2).
//!
//! Phase becomes per-session: a `Phase` claim carries
//! `{session_id, name}`. Setting a phase appends a new claim that
//! supersedes the prior Phase claim for the same `session_id`;
//! showing reads the most recent Phase claim. Migrated v1 data lives
//! under `session_id = "global-migrated"`.

use anyhow::{anyhow, bail, Result};
use nomograph_claim::ClaimType;
use serde_json::{json, Value};

use crate::cli::PhaseCmd;
use crate::store::{json_out, SynthStore};
use crate::types::Phase;

/// Session id used for migrated v1 phase data (there was one global
/// `phase` table, not per-session).
const GLOBAL_SESSION_ID: &str = "global-migrated";

pub fn run(cmd: &PhaseCmd, force: bool) -> Result<()> {
    // The top-level `--session` flag is declared `global = true` with
    // `env = "SYNTHESIST_SESSION"`. Clap propagates it to env on parse
    // (see tests around the env binding); if the caller passed nothing
    // we fall through to the migrated default so legacy `phase show`
    // still works.
    let session = std::env::var("SYNTHESIST_SESSION").ok();
    match cmd {
        PhaseCmd::Set { name } => cmd_phase_set(name, session.as_deref(), force),
        PhaseCmd::Show => cmd_phase_show(session.as_deref()),
    }
}

fn resolve_session(explicit: Option<&str>) -> String {
    explicit
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| GLOBAL_SESSION_ID.to_string())
}

/// Append a new Phase claim after validating the from→to transition.
fn cmd_phase_set(name: &str, session: Option<&str>, force: bool) -> Result<()> {
    let target = Phase::from_str(name).ok_or_else(|| {
        anyhow!("unknown phase: {name} (valid: orient, plan, agree, execute, reflect, replan, report)")
    })?;

    let session_id = resolve_session(session);
    let mut store = SynthStore::discover()?;

    // Look up the current Phase claim for this session, if any.
    let prior = current_phase_claim(&store, &session_id)?;

    if let Some((_, current_str)) = prior.as_ref() {
        if let Some(current) = Phase::from_str(current_str) {
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
    }

    let props = json!({
        "session_id": session_id,
        "name": name,
    });
    let supersedes = prior.map(|(id, _)| id);
    store.append(ClaimType::Phase, props, supersedes)?;
    json_out(&json!({"phase": name, "session_id": session_id}))
}

fn cmd_phase_show(session: Option<&str>) -> Result<()> {
    let session_id = resolve_session(session);
    let store = SynthStore::discover()?;
    let phase = current_phase_claim(&store, &session_id)?
        .map(|(_, name)| name)
        .unwrap_or_else(|| "orient".to_string());
    json_out(&json!({"phase": phase, "session_id": session_id}))
}

/// Return `(claim_id, phase_name)` for the most recent Phase claim in
/// `session_id`, or `None` if no Phase claim exists.
fn current_phase_claim(store: &SynthStore, session_id: &str) -> Result<Option<(String, String)>> {
    let rows = store.query(
        "SELECT id, props FROM claims \
         WHERE claim_type = 'phase' \
           AND json_extract(props, '$.session_id') = ?1 \
         ORDER BY asserted_at DESC LIMIT 1",
        &[&session_id],
    )?;
    let row = match rows.into_iter().next() {
        Some(r) => r,
        None => return Ok(None),
    };
    let claim_id = row
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("phase row missing id"))?
        .to_string();
    let props_str = row
        .get("props")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("phase row missing props"))?;
    let props: Value = serde_json::from_str(props_str)?;
    let name = props
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("phase claim missing name"))?
        .to_string();
    Ok(Some((claim_id, name)))
}

/// Check if an operation is allowed in the current phase.
///
/// The transition matrix is unchanged from v1; only the read path
/// moved from the `phase` SQL table to a claim query. When no Phase
/// claim exists for the session, we default to `orient` (the phase
/// state machine's entry point) so fresh stores don't bail on every
/// write.
pub fn check_phase(store: &SynthStore, top_cmd: &str, sub_cmd: &str, force: bool) -> Result<()> {
    if force {
        return Ok(());
    }

    let session_id = std::env::var("SYNTHESIST_SESSION")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| GLOBAL_SESSION_ID.to_string());

    let phase = current_phase_claim(store, &session_id)?
        .map(|(_, name)| name)
        .unwrap_or_else(|| "orient".to_string());

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

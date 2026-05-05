//! `synthesist phase ...` CLI handlers.
//!
//! v2.1 moved the [`Phase`] enum, transition rules, and `check_phase`
//! enforcer into [`nomograph_workflow::phase`]. This file only owns
//! the CLI-facing `phase set` / `phase show` handlers; cross-tool
//! consumers (future `seer`) share the state machine automatically.

use anyhow::{Result, anyhow, bail};
use nomograph_claim::ClaimType;
use nomograph_workflow::Phase;
use nomograph_workflow::phase::current_phase_claim;
use serde_json::json;

use crate::cli::PhaseCmd;
use crate::store::{SynthStore, json_out};

/// Re-exported for `main.rs` so command dispatch can keep calling
/// `cmd_phase::check_phase(..)` without every caller learning the
/// workflow crate path.
pub use nomograph_workflow::phase::check_phase;

pub fn run(cmd: &PhaseCmd, session: &Option<String>, force: bool) -> Result<()> {
    // Clap's `env = "SYNTHESIST_SESSION"` on the top-level `--session`
    // flag is read-only: it populates `cli.session` when the env var
    // is set but does NOT write the env when `--session=<id>` comes in
    // as a flag. So we thread the parsed value through explicitly,
    // matching the pattern used everywhere else in the adapter.
    let session_ref = session.as_deref();
    match cmd {
        PhaseCmd::Set { name } => cmd_phase_set(name, session_ref, force),
        PhaseCmd::Show => cmd_phase_show(session_ref),
    }
}

fn resolve_session(explicit: Option<&str>) -> Result<String> {
    explicit
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or_else(|| anyhow!(
            "phase is per-session in v2; pass --session=<id> or set SYNTHESIST_SESSION.\n\
             \n  start one:    synthesist session start <id>\
             \n  show all:     synthesist status   (lists phase per live session)\
             \n  show one:     synthesist phase show --session=<id>"
        ))
}

fn cmd_phase_set(name: &str, session: Option<&str>, force: bool) -> Result<()> {
    let target = Phase::from_str(name).ok_or_else(|| {
        anyhow!(
            "unknown phase: {name} (valid: orient, plan, agree, execute, reflect, replan, report)"
        )
    })?;

    let session_id = resolve_session(session)?;
    let mut store = SynthStore::discover()?;

    let prior = current_phase_claim(&store, &session_id)?;
    if let Some((_, current_str)) = prior.as_ref()
        && let Some(current) = Phase::from_str(current_str)
        && !force
        && !current.can_transition_to(target)
    {
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

    let props = json!({
        "session_id": session_id,
        "name": name,
    });
    let supersedes = prior.map(|(id, _)| id);
    store.append(ClaimType::Phase, props, supersedes)?;
    json_out(&json!({"phase": name, "session_id": session_id}))
}

fn cmd_phase_show(session: Option<&str>) -> Result<()> {
    let session_id = resolve_session(session)?;
    let store = SynthStore::discover()?;
    let phase = current_phase_claim(&store, &session_id)?
        .map(|(_, name)| name)
        .unwrap_or_else(|| "orient".to_string());
    json_out(&json!({"phase": phase, "session_id": session_id}))
}

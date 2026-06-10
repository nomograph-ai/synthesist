//! `synthesist phase ...` CLI handlers (v3-native: typed gamma queries).
//!
//! Phase claims carry `{session_id, name}` and supersede earlier
//! phase claims for the same session. Queries go through the typed
//! gamma index helpers (no SPARQL).

use crate::claim_type::ClaimType;
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::cli::PhaseCmd;
use crate::store::{SynthStore, json_out};

/// Workflow phase. The 7-phase enum now lives in synthesist directly
/// (the workflow crate was dropped); callers depend only on this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Orient,
    Plan,
    Agree,
    Execute,
    Reflect,
    Replan,
    Report,
}

impl Phase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Orient => "orient",
            Self::Plan => "plan",
            Self::Agree => "agree",
            Self::Execute => "execute",
            Self::Reflect => "reflect",
            Self::Replan => "replan",
            Self::Report => "report",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "orient" => Some(Self::Orient),
            "plan" => Some(Self::Plan),
            "agree" => Some(Self::Agree),
            "execute" => Some(Self::Execute),
            "reflect" => Some(Self::Reflect),
            "replan" => Some(Self::Replan),
            "report" => Some(Self::Report),
            _ => None,
        }
    }

    pub fn valid_transitions(&self) -> &'static [Phase] {
        match self {
            Self::Orient => &[Self::Plan],
            Self::Plan => &[Self::Agree],
            Self::Agree => &[Self::Execute],
            Self::Execute => &[Self::Reflect, Self::Report],
            Self::Reflect => &[Self::Execute, Self::Replan, Self::Report],
            Self::Replan => &[Self::Agree],
            Self::Report => &[],
        }
    }

    pub fn can_transition_to(&self, target: Phase) -> bool {
        self.valid_transitions().contains(&target)
    }
}

pub fn run(cmd: &PhaseCmd, session: &Option<String>, force: bool) -> Result<()> {
    match cmd {
        PhaseCmd::Set { name } => cmd_phase_set(name, session, force),
        PhaseCmd::Show => cmd_phase_show(session.as_deref()),
    }
}

fn resolve_session(explicit: Option<&str>) -> Result<String> {
    explicit
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or_else(|| {
            anyhow!(
                "phase is per-session in v2; pass --session=<id> or set SYNTHESIST_SESSION.\n\
             \n  start one:    synthesist session start <id>\
             \n  show all:     synthesist status   (lists phase per live session)\
             \n  show one:     synthesist phase show --session=<id>"
            )
        })
}

fn cmd_phase_set(name: &str, session: &Option<String>, force: bool) -> Result<()> {
    let target = Phase::from_str(name).ok_or_else(|| {
        anyhow!(
            "unknown phase: {name} (valid: orient, plan, agree, execute, reflect, replan, report)"
        )
    })?;

    let session_id = resolve_session(session.as_deref())?;
    let mut store = SynthStore::discover_for(session)?;

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
    let phase = current_phase_name(&store, &session_id)?.unwrap_or_else(|| "orient".to_string());
    json_out(&json!({"phase": phase, "session_id": session_id}))
}

// The per-phase write gate moved OUT of the core into the phase-policy
// extension (nomograph-extension): the core no longer enforces process.
// A consumer opts in by configuring a policy extension at the dispatch
// site; bare synthesist is just data. The phase table now lives in the
// extension's `before-write` hook.

/// Return the current phase name for `session_id`.
///
/// Resolves the head of the Phase supersession chain for `session_id`
/// via [`current_phase_claim`], which delegates to `store.current_phase`
/// (the gamma H6 typed query), then reads the phase name off the claim.
pub fn current_phase_name(store: &SynthStore, session_id: &str) -> Result<Option<String>> {
    Ok(current_phase_claim(store, session_id)?.map(|(_, name)| name))
}

/// Return `(claim_iri, phase_name)` for the head of the Phase
/// supersession chain for `session_id`, or `None` if no phase claim
/// exists.
pub fn current_phase_claim(
    store: &SynthStore,
    session_id: &str,
) -> Result<Option<(String, String)>> {
    // H6: head of the Phase supersession chain for this session.
    let Some(claim_id) = store.current_phase(session_id)? else {
        return Ok(None);
    };
    let Some(doc) = store.doc(&claim_id)? else {
        return Ok(None);
    };
    let name = match doc
        .get(crate::wire_format::predicate_iri("name").as_str())
        .and_then(|v| v.as_str())
    {
        Some(s) => s.to_string(),
        None => return Ok(None),
    };
    // Strip the compact prefix to the bare hash so the caller can pass
    // it straight to `SynthStore::append` as `supersedes` (which
    // re-prefixes via `wire_format::claim_iri`).
    Ok(Some((crate::store::short_claim_id(&claim_id), name)))
}

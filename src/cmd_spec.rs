//! Spec commands (v2: claim-backed).
//!
//! Writes and reads `Spec` claims via the synthesist claim store. Every
//! `spec add` appends one [`nomograph_claim::ClaimType::Spec`] claim;
//! `spec update` appends a superseding claim that propagates unchanged
//! fields forward; `spec show` and `spec list` query the SQLite view
//! projection and dedupe-by-(tree, id) keeping the most recent
//! non-superseded head.
//!
//! The CLI surface is unchanged from v1: same subcommands, same flags,
//! same JSON output shape (the v1 `created` column is no longer
//! projected — the claim's `asserted_at` supersedes it, mirroring the
//! pattern in `cmd_discovery.rs`).

use std::collections::HashSet;

use anyhow::{Result, anyhow, bail};
use nomograph_claim::ClaimType;
use serde_json::{Map, Value, json};

use crate::cli::SpecCmd;
use crate::store::{SynthStore, json_out};

/// Dispatch a `synthesist spec <...>` subcommand.
pub fn run(cmd: &SpecCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        SpecCmd::Add {
            tree_spec,
            goal,
            constraints,
            decisions,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_spec_add(
                tree,
                spec,
                goal.as_deref(),
                constraints.as_deref(),
                decisions.as_deref(),
                session,
            )
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
        SpecCmd::List { tree, tree_flag } => {
            let resolved = tree
                .as_deref()
                .or(tree_flag.as_deref())
                .ok_or_else(|| anyhow::anyhow!(
                    "tree required: pass as positional `synthesist spec list <tree>` or as flag `synthesist spec list --tree <tree>`"
                ))?;
            cmd_spec_list(resolved, session)
        }
    }
}

/// Split `tree/spec` into `(tree, spec)` with a prescriptive error.
///
/// Inlined here to keep cmd_spec.rs self-contained; other v2 command
/// modules (cmd_discovery, cmd_task) expect this helper on `store` — a
/// shared helper lives on the backlog for the next port pass.
fn parse_tree_spec(ts: &str) -> Result<(&str, &str)> {
    let (tree, spec) = ts
        .split_once('/')
        .ok_or_else(|| anyhow!("expected tree/spec, got {ts}"))?;
    if tree.is_empty() || spec.is_empty() {
        bail!("expected tree/spec, got {ts}");
    }
    Ok((tree, spec))
}

/// Append a new `Spec` claim with status=active.
///
/// v1 auto-ensured the parent tree row for FK integrity; v2 has no FK
/// layer (claims are independent append-only facts), so that INSERT OR
/// IGNORE drops out. The schema requires a non-empty `goal`; if the
/// caller omits `--goal`, we fail fast with a prescriptive message
/// rather than write a claim that validation would reject downstream.
///
/// `topics` defaults to `[spec_id]` so validation passes; Andrew
/// reclassifies later.
fn cmd_spec_add(
    tree: &str,
    spec: &str,
    goal: Option<&str>,
    constraints: Option<&str>,
    decisions: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    let goal = goal
        .filter(|g| !g.is_empty())
        .ok_or_else(|| anyhow!("spec add requires non-empty --goal"))?;

    let mut props = Map::new();
    props.insert("tree".into(), Value::from(tree));
    props.insert("id".into(), Value::from(spec));
    props.insert("goal".into(), Value::from(goal));
    props.insert("status".into(), Value::from("active"));
    props.insert("topics".into(), Value::from(vec![spec.to_string()]));
    if let Some(v) = constraints {
        props.insert("constraints".into(), Value::from(v));
    }
    if let Some(v) = decisions {
        props.insert("decisions".into(), Value::from(v));
    }

    let mut store = SynthStore::discover_for(session)?;
    store.append(ClaimType::Spec, Value::Object(props), None)?;

    json_out(&json!({
        "tree": tree,
        "id": spec,
        "goal": goal,
        "status": "active",
    }))
}

/// Show the current `Spec` claim for `tree/id`, or error when absent.
fn cmd_spec_show(tree: &str, spec: &str, session: &Option<String>) -> Result<()> {
    let store = SynthStore::discover_for(session)?;
    let rows = query_spec_heads(&store, tree)?;
    match rows.into_iter().find(|p| spec_id(p) == spec) {
        Some(props) => json_out(&json!({
            "tree": tree,
            "id": spec,
            "goal": props.get("goal").cloned().unwrap_or(Value::Null),
            "constraints": props.get("constraints").cloned().unwrap_or(Value::Null),
            "decisions": props.get("decisions").cloned().unwrap_or(Value::Null),
            "status": props.get("status").cloned().unwrap_or(Value::Null),
            "outcome": props.get("outcome").cloned().unwrap_or(Value::Null),
        })),
        None => bail!("spec not found: {tree}/{spec}"),
    }
}

/// Append a superseding `Spec` claim that overlays the provided deltas
/// onto the prior claim's props.
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
    if goal.is_none()
        && constraints.is_none()
        && decisions.is_none()
        && status.is_none()
        && outcome.is_none()
    {
        bail!("no fields to update");
    }

    let mut store = SynthStore::discover_for(session)?;

    let prior = store.query(
        "SELECT id, props FROM claims \
         WHERE claim_type = 'spec' \
           AND json_extract(props, '$.tree') = ?1 \
           AND json_extract(props, '$.id')   = ?2 \
         ORDER BY asserted_at DESC \
         LIMIT 1",
        &[&tree, &spec],
    )?;
    let prior = prior
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("spec not found: {tree}/{spec}"))?;

    let prior_id = prior
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("claim row missing id"))?
        .to_string();
    let prior_props_str = prior
        .get("props")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("claim row missing props"))?;
    let prior_props: Value = serde_json::from_str(prior_props_str)?;
    let mut props: Map<String, Value> = prior_props
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("prior spec props not an object"))?;

    // Overlay deltas. Missing fields keep the prior value.
    if let Some(v) = goal {
        props.insert("goal".into(), Value::from(v));
    }
    if let Some(v) = constraints {
        props.insert("constraints".into(), Value::from(v));
    }
    if let Some(v) = decisions {
        props.insert("decisions".into(), Value::from(v));
    }
    if let Some(v) = status {
        props.insert("status".into(), Value::from(v));
    }
    if let Some(v) = outcome {
        props.insert("outcome".into(), Value::from(v));
    }

    // Preserve tree/id/topics invariants in case prior was malformed.
    props.insert("tree".into(), Value::from(tree));
    props.insert("id".into(), Value::from(spec));
    if !props.get("topics").is_some_and(|v| v.is_array()) {
        props.insert("topics".into(), Value::from(vec![spec.to_string()]));
    }

    store.append(ClaimType::Spec, Value::Object(props), Some(prior_id))?;
    json_out(&json!({"tree": tree, "id": spec, "updated": true}))
}

/// List every spec head in `tree`, ordered by spec id.
///
/// Heads are deduped per (tree, id) keeping the most recent non-
/// superseded asserted_at. The view's `supersedes` column lets us
/// exclude any claim whose id was superseded by a later one.
fn cmd_spec_list(tree: &str, session: &Option<String>) -> Result<()> {
    let store = SynthStore::discover_for(session)?;
    let heads = query_spec_heads(&store, tree)?;
    let mut specs: Vec<Value> = heads
        .into_iter()
        .map(|props| {
            json!({
                "id": props.get("id").cloned().unwrap_or(Value::Null),
                "goal": props.get("goal").cloned().unwrap_or(Value::Null),
                "status": props.get("status").cloned().unwrap_or(Value::Null),
                "outcome": props.get("outcome").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    specs.sort_by(|a, b| {
        let a = a.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let b = b.get("id").and_then(|v| v.as_str()).unwrap_or("");
        a.cmp(b)
    });
    json_out(&json!({"tree": tree, "specs": specs}))
}

/// Return the per-spec head `props` for every spec in `tree`.
///
/// A head is the most recent non-superseded claim for a given
/// (tree, id) pair. We walk asserted_at DESC and keep the first props
/// seen for each id, while tracking which claim ids have been
/// superseded so we skip them entirely.
fn query_spec_heads(store: &SynthStore, tree: &str) -> Result<Vec<Value>> {
    let rows = store.query(
        "SELECT id, props, supersedes \
         FROM claims \
         WHERE claim_type = 'spec' \
           AND json_extract(props, '$.tree') = ?1 \
         ORDER BY asserted_at DESC",
        &[&tree],
    )?;

    let superseded: HashSet<String> = rows
        .iter()
        .filter_map(|r| {
            r.get("supersedes")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();

    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Value> = Vec::new();
    for row in rows {
        let id = match row.get("id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if superseded.contains(&id) {
            continue;
        }
        let props_str = match row.get("props").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let props: Value = match serde_json::from_str(props_str) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let spec_key = spec_id(&props).to_string();
        if spec_key.is_empty() || !seen.insert(spec_key) {
            continue;
        }
        out.push(props);
    }
    Ok(out)
}

/// Extract `props.id` as a string slice, or `""` when missing/wrong-typed.
fn spec_id(props: &Value) -> &str {
    props.get("id").and_then(|v| v.as_str()).unwrap_or("")
}

//! Spec commands -- ported to the v3 redb-gamma substrate.
//!
//! Reads (list/show) run typed passes over the redb gamma index.
//! Writes (add/update) call `SynthStore::append`, which writes one v3
//! JSON-LD doc per call.

use crate::claim_type::ClaimType;
use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value, json};

use crate::cli::SpecCmd;
use crate::store::{SynthStore, bare_props, json_out, short_claim_id};
use crate::wire_format as wf;

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
            cmd_spec_show(tree, spec)
        }
        SpecCmd::Update {
            tree_spec,
            goal,
            constraints,
            decisions,
            status,
            outcome,
            agree_snapshot,
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
                agree_snapshot.as_deref(),
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
            cmd_spec_list(resolved)
        }
    }
}

fn parse_tree_spec(ts: &str) -> Result<(&str, &str)> {
    let (tree, spec) = ts
        .split_once('/')
        .ok_or_else(|| anyhow!("expected tree/spec, got {ts}"))?;
    if tree.is_empty() || spec.is_empty() {
        bail!("expected tree/spec, got {ts}");
    }
    Ok((tree, spec))
}

fn cmd_spec_add(
    tree: &str,
    spec: &str,
    goal: Option<&str>,
    constraints: Option<&str>,
    decisions: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    let goal = goal.filter(|g| !g.is_empty()).ok_or_else(|| {
        anyhow!(
            "spec add requires --goal <text>; \
             example: synthesist --session=<id> spec add {tree}/{spec} --goal \"<description>\""
        )
    })?;

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
        "ok": true,
        "tree": tree,
        "id": spec,
        "goal": goal,
        "status": "active",
    }))
}

fn cmd_spec_show(tree: &str, spec: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    let heads = live_spec_heads(&store, tree)?;
    match heads
        .into_iter()
        .find(|(_, p)| p.get("id").and_then(|v| v.as_str()) == Some(spec))
    {
        Some((_, props)) => json_out(&json!({
            "tree": tree,
            "id": spec,
            "goal": props.get("goal").cloned().unwrap_or(Value::Null),
            "constraints": props.get("constraints").cloned().unwrap_or(Value::Null),
            "decisions": props.get("decisions").cloned().unwrap_or(Value::Null),
            "status": props.get("status").cloned().unwrap_or(Value::Null),
            "outcome": props.get("outcome").cloned().unwrap_or(Value::Null),
        })),
        None => bail!(
            "spec not found: {tree}/{spec}. \
             List specs in this tree with `synthesist spec list {tree}`."
        ),
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
    agree_snapshot: Option<&[String]>,
    session: &Option<String>,
) -> Result<()> {
    if goal.is_none()
        && constraints.is_none()
        && decisions.is_none()
        && status.is_none()
        && outcome.is_none()
        && agree_snapshot.is_none()
    {
        bail!(
            "no fields to update; pass at least one of: \
             --goal, --constraints, --decisions, --status, --outcome, --agree-snapshot"
        );
    }

    let mut store = SynthStore::discover_for(session)?;
    let heads = live_spec_heads(&store, tree)?;
    let (prior_id, prior_props) = heads
        .into_iter()
        .find(|(_, p)| p.get("id").and_then(|v| v.as_str()) == Some(spec))
        .ok_or_else(|| {
            anyhow!(
                "spec not found: {tree}/{spec}. \
             List specs in this tree with `synthesist spec list {tree}`."
            )
        })?;

    let mut props: Map<String, Value> = prior_props
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("prior spec props not an object"))?;

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
    if let Some(snap) = agree_snapshot {
        let arr: Vec<Value> = snap
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| Value::from(s.as_str()))
            .collect();
        props.insert("agree_snapshot".into(), Value::Array(arr));
    }

    props.insert("tree".into(), Value::from(tree));
    props.insert("id".into(), Value::from(spec));
    if !props.get("topics").is_some_and(|v| v.is_array()) {
        props.insert("topics".into(), Value::from(vec![spec.to_string()]));
    }

    store.append(ClaimType::Spec, Value::Object(props), Some(prior_id))?;
    json_out(&json!({"ok": true, "tree": tree, "id": spec}))
}

fn cmd_spec_list(tree: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    let heads = live_spec_heads(&store, tree)?;
    let mut specs: Vec<Value> = heads
        .into_iter()
        .map(|(_, props)| {
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

/// Return `(prior_id, props)` for every live Spec head in `tree`.
fn live_spec_heads(store: &SynthStore, tree: &str) -> Result<Vec<(String, Value)>> {
    let mut out: Vec<(String, Value)> = Vec::new();
    for (id, doc) in store.live_docs(&wf::type_iri("spec"))? {
        let bare = bare_props(&doc);
        if bare.get("tree").and_then(|v| v.as_str()) != Some(tree) {
            continue;
        }
        let spec_id = match bare.get("id").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let mut props = Map::new();
        props.insert("tree".into(), Value::String(tree.to_string()));
        props.insert("id".into(), Value::String(spec_id));
        for key in ["goal", "constraints", "decisions", "status", "outcome"] {
            if let Some(v) = bare.get(key).and_then(|v| v.as_str())
                && !v.is_empty()
            {
                props.insert(key.into(), Value::String(v.to_string()));
            }
        }
        out.push((short_claim_id(&id), Value::Object(props)));
    }
    out.sort_by(|a, b| {
        let ai = a.1.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let bi = b.1.get("id").and_then(|v| v.as_str()).unwrap_or("");
        ai.cmp(bi)
    });
    Ok(out)
}

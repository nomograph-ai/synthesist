//! Campaign commands -- ported to the gamma typed query surface (C-2).
//!
//! Campaign is a cross-spec coordination primitive: each Campaign claim
//! per (tree, spec) is either `active`, `backlog`, or (after the close
//! port lands at the CLI level) `closed`. Reads project the live head
//! per (tree, spec) via the gamma H2 live-head anti-join; writes route
//! through `SynthStore::append`.
//!
//! NOTE: the v2 `CampaignCmd` enum exposes only `Add` and `List`. The
//! Stage 2 brief mentions a `campaign close <tree>/<spec>` subcommand,
//! which would require an additive change in `cli.rs`. That CLI surface
//! is intentionally out of scope for this commit.

use anyhow::Result;
use serde_json::{Map, Value, json};

use crate::cli::CampaignCmd;
use crate::store::{SynthStore, bare_props, json_out};
use crate::wire_format as wf;

/// Dispatch a `synthesist campaign <...>` subcommand.
pub fn run(cmd: &CampaignCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        CampaignCmd::Add {
            tree,
            spec_id,
            summary,
            backlog,
            title,
            blocked_by,
        } => {
            let kind = if *backlog { "backlog" } else { "active" };
            cmd_add(
                tree,
                spec_id,
                kind,
                title.as_deref(),
                summary,
                blocked_by,
                session,
            )
        }
        CampaignCmd::List { tree } => cmd_list(Some(tree.as_str())),
    }
}

/// Append a new `Campaign` claim (either `active` or `backlog`).
///
/// The `Campaign` props shape (per `crate::schema::campaign::validate`):
/// ```json
/// { "tree": "...", "spec": "...", "kind": "active"|"backlog",
///   "summary": "...", "title": "...", "blocked_by": ["..."] }
/// ```
fn cmd_add(
    tree: &str,
    spec_id: &str,
    kind: &str,
    title: Option<&str>,
    summary: &str,
    blocked_by: &[String],
    session: &Option<String>,
) -> Result<()> {
    if tree.is_empty() {
        anyhow::bail!(
            "campaign add requires a non-empty tree name; \
             pass it as the first positional argument: synthesist campaign add <tree> <spec_id>"
        );
    }
    if spec_id.is_empty() {
        anyhow::bail!(
            "campaign add requires a non-empty spec id; \
             pass it as the second positional argument: synthesist campaign add <tree> <spec_id>"
        );
    }

    let mut props = Map::new();
    props.insert("tree".into(), Value::from(tree));
    props.insert("spec".into(), Value::from(spec_id));
    props.insert("kind".into(), Value::from(kind));
    if !summary.is_empty() {
        props.insert("summary".into(), Value::from(summary));
    }
    if let Some(t) = title
        && !t.is_empty()
    {
        props.insert("title".into(), Value::from(t));
    }
    let deps: Vec<Value> = blocked_by
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| Value::from(s.as_str()))
        .collect();
    if !deps.is_empty() {
        props.insert("blocked_by".into(), Value::Array(deps));
    }

    let mut store = SynthStore::discover_for(session)?;
    store.append(
        crate::claim_type::ClaimType::Campaign,
        Value::Object(props),
        None,
    )?;

    json_out(&json!({"ok": true, "tree": tree, "spec_id": spec_id, "kind": kind}))
}

/// List the live Campaign head per (tree, spec) pair. Filter on `tree`
/// when supplied.
///
/// Output:
/// ```json
/// { "campaigns": [
///     { "tree": "...", "spec": "...", "kind": "...",
///       "summary": "...", "title": "...", "blocked_by": ["..."] },
///     ...
/// ] }
/// ```
fn cmd_list(tree_filter: Option<&str>) -> Result<()> {
    let store = SynthStore::discover()?;
    let mut rows: Vec<((String, String), Value)> = Vec::new();
    for (_, doc) in store.live_docs(&wf::type_iri("campaign"))? {
        let props = bare_props(&doc);
        let tree = match props.get("tree").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let spec = match props.get("spec").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if let Some(t) = tree_filter
            && tree != t
        {
            continue;
        }
        let str_opt = |k: &str| -> Option<String> {
            props
                .get(k)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        };
        // blockedBy is a member predicate -> bare_props yields an array.
        let blocked_by: Vec<Value> = match props.get("blocked_by") {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect(),
            Some(Value::String(s)) if !s.is_empty() => vec![Value::String(s.clone())],
            _ => Vec::new(),
        };

        let mut obj = Map::new();
        obj.insert("tree".into(), Value::String(tree.clone()));
        obj.insert("spec".into(), Value::String(spec.clone()));
        if let Some(v) = str_opt("kind") {
            obj.insert("kind".into(), Value::String(v));
        }
        if let Some(v) = str_opt("summary") {
            obj.insert("summary".into(), Value::String(v));
        }
        if let Some(v) = str_opt("title") {
            obj.insert("title".into(), Value::String(v));
        }
        obj.insert("blocked_by".into(), Value::Array(blocked_by));
        rows.push(((tree, spec), Value::Object(obj)));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let campaigns: Vec<Value> = rows.into_iter().map(|(_, v)| v).collect();
    json_out(&json!({ "campaigns": campaigns }))
}

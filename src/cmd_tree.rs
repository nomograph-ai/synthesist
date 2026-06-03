//! Tree commands -- ported to the v3 redb-gamma substrate.
//!
//! Reference port (Stage 1). The supersession chain walk is now a
//! typed gamma-index pass over live heads instead of a SQL
//! `supersedes IS NULL` + client-side dedup.

use crate::claim_type::ClaimType;
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::cli::TreeCmd;
use crate::store::{SynthStore, bare_props, json_out, short_claim_id};
use crate::wire_format as wf;

pub fn run(cmd: &TreeCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        TreeCmd::Add {
            name,
            description,
            status,
        } => cmd_tree_add(name, description, status, session),
        TreeCmd::List { include_closed } => cmd_tree_list(*include_closed),
        TreeCmd::Show { name } => cmd_tree_show(name),
        TreeCmd::Close { name, start_id } => cmd_tree_close(name, start_id.as_deref(), session),
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
        "ok": true,
        "name": name,
        "status": status,
        "description": description,
    }))
}

fn cmd_tree_list(include_closed: bool) -> Result<()> {
    let store = SynthStore::discover()?;
    let trees = live_tree_heads(&store, include_closed)?;
    json_out(&json!({ "trees": trees }))
}

fn cmd_tree_show(name: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    let trees = live_tree_heads(&store, true)?;
    let row = trees
        .iter()
        .find(|t| t.get("name").and_then(|v| v.as_str()) == Some(name))
        .ok_or_else(|| anyhow!("tree not found: {name}. Try `synthesist tree list`."))?;

    // Spec count and session count via SPARQL.
    let spec_count = count_for_tree(&store, "synthesist:Spec", name)?;
    let session_count = count_for_tree(&store, "synthesist:Session", name)?;

    json_out(&json!({
        "name": row.get("name").cloned().unwrap_or(json!(name)),
        "description": row.get("description").cloned().unwrap_or(Value::Null),
        "spec_count": spec_count,
        "session_count": session_count,
    }))
}

/// `tree close <name>` -- find the live `Tree` head with the given
/// name and append a superseding `Tree` claim with `status = "closed"`.
///
/// When `--start-id` is supplied it disambiguates between trees that
/// share a name (any unambiguous hex prefix of the original opener's
/// claim hash). When omitted and multiple live trees match, bail with
/// a prescriptive error listing the candidates.
fn cmd_tree_close(name: &str, start_id: Option<&str>, session: &Option<String>) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;

    // Find the live Tree head(s) matching `name`. The list path filters
    // by `name` and surfaces the same `start_id` shape we accept here.
    let mut candidates: Vec<(String, String, String)> = Vec::new();
    for (id, doc) in store.live_docs(&wf::type_iri("tree"))? {
        let props = bare_props(&doc);
        if props.get("name").and_then(|v| v.as_str()) != Some(name) {
            continue;
        }
        let desc = props
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let status = props
            .get("status")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("active")
            .to_string();
        candidates.push((id, desc, status));
    }

    if candidates.is_empty() {
        bail!(
            "tree not found: {name}. \
             Run `synthesist tree list --include-closed` to see all trees, or \
             `synthesist tree add {name}` to create it."
        );
    }

    // Filter to those still active (skip already-closed trees) before
    // disambiguating, so `close` on an already-closed name complains
    // about no candidates rather than silently re-closing.
    let active: Vec<(String, String, String)> = candidates
        .iter()
        .filter(|(_, _, s)| s != "closed")
        .cloned()
        .collect();
    if active.is_empty() {
        bail!(
            "tree '{name}' is already closed; \
             list with `synthesist tree list --include-closed` to confirm"
        );
    }

    let (iri, desc, _status) = match start_id {
        Some(prefix) if !prefix.is_empty() => {
            let matched: Vec<(String, String, String)> = active
                .iter()
                .filter(|(iri, _, _)| short_claim_id(iri).starts_with(prefix))
                .cloned()
                .collect();
            match matched.len() {
                0 => bail!(
                    "no active tree '{name}' matches --start-id '{prefix}'; \
                     run `synthesist tree list` to see candidates"
                ),
                1 => matched.into_iter().next().unwrap(),
                _ => bail!(
                    "--start-id '{prefix}' is ambiguous among {} active trees named '{name}'; \
                     supply a longer prefix",
                    matched.len()
                ),
            }
        }
        _ => {
            if active.len() > 1 {
                let ids: Vec<String> = active
                    .iter()
                    .map(|(iri, _, _)| short_claim_id(iri))
                    .collect();
                bail!(
                    "ambiguous: multiple active trees named '{name}'; \
                     disambiguate with `synthesist tree close {name} --start-id <prefix>` \
                     (candidates: {})",
                    ids.join(", ")
                );
            }
            active.into_iter().next().unwrap()
        }
    };

    let prior_id = short_claim_id(&iri);
    let mut props = serde_json::Map::new();
    props.insert("name".into(), Value::String(name.to_string()));
    if !desc.is_empty() {
        props.insert("description".into(), Value::String(desc));
    }
    props.insert("status".into(), Value::String("closed".to_string()));

    store.append(
        ClaimType::Tree,
        Value::Object(props),
        Some(prior_id.clone()),
    )?;
    json_out(&json!({
        "closed": true,
        "name": name,
        "start_id": prior_id,
    }))
}

// ---------------------------------------------------------------------------
// SPARQL helpers
// ---------------------------------------------------------------------------

/// Live Tree heads (no later claim supersedes them). When
/// `include_closed` is false, filter out any whose status is "closed".
fn live_tree_heads(store: &SynthStore, include_closed: bool) -> Result<Vec<Value>> {
    let mut out: Vec<Value> = Vec::new();
    for (id, doc) in store.live_docs(&wf::type_iri("tree"))? {
        let props = bare_props(&doc);
        let name = match props.get("name").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let desc = props.get("description").cloned().unwrap_or(Value::Null);
        let status = props
            .get("status")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("active")
            .to_string();
        if !include_closed && status == "closed" {
            continue;
        }
        out.push(json!({
            "name": name,
            "status": status,
            "description": desc,
            "start_id": short_claim_id(&id),
        }));
    }
    // live_docs is gamma-id sorted; re-sort by name for the v2 contract.
    out.sort_by(|a, b| {
        a.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
    });
    Ok(out)
}

/// Count all claim versions of `type_value` scoped to `tree`.
///
/// Mirrors the retired SPARQL
/// `SELECT (COUNT(DISTINCT ?c)) WHERE { ?c rdf:type {type}; synthesist:tree "{tree}" }`,
/// which had no `FILTER NOT EXISTS` and so counted every version
/// (superseded revisions included), not just live heads. The count is
/// deliberately not live-filtered: `tree show`'s `spec_count` /
/// `session_count` report total claim activity on the tree.
fn count_for_tree(store: &SynthStore, type_value: &str, tree: &str) -> Result<i64> {
    let n = store.count_by_type_and_value(type_value, &wf::predicate_iri("tree"), tree)?;
    Ok(n as i64)
}

//! Unified task mutation helper.
//!
//! Every existing task-state command (`claim`, `done`, `block`, `wait`,
//! `cancel`, `update`, `reset`) follows the same pattern: load the
//! current task head, mutate `props`, append a superseding claim. This
//! module folds that pattern into one helper so each command is a
//! small, focused mutation closure rather than a re-implementation of
//! the load-mutate-append boilerplate.
//!
//! Validation runs at the synthesist API boundary inside
//! [`crate::store::SynthStore::append`], so callers don't need to
//! validate explicitly. If a mutation produces props that don't
//! conform to the Task schema, append fails with a structured error.

use anyhow::{Context, Result, anyhow};
use nomograph_claim::ClaimType;
use serde_json::{Map, Value};

use crate::store::SynthStore;

/// Load the current `(tree, spec, task_id)` head, run `mutation`
/// against its props, then append a superseding Task claim.
///
/// Returns the post-mutation props as a JSON value (suitable for
/// passing to `json_out`), or an error if the task isn't found,
/// `mutation` rejects, or schema validation fails on append.
#[allow(dead_code)]
pub fn mutate<F>(
    store: &mut SynthStore,
    tree: &str,
    spec: &str,
    task_id: &str,
    mutation: F,
) -> Result<Value>
where
    F: FnOnce(&mut Map<String, Value>) -> Result<()>,
{
    let (prior_id, mut props) = load_current(store, tree, spec, task_id)?
        .with_context(|| format!("task {tree}/{spec}/{task_id} not found"))?;
    let map = props
        .as_object_mut()
        .ok_or_else(|| anyhow!("task props not a JSON object"))?;
    mutation(map).with_context(|| format!("mutate task {tree}/{spec}/{task_id}"))?;
    store.append(ClaimType::Task, props.clone(), Some(prior_id))?;
    Ok(props)
}

/// Read-only loader: returns `(prior_claim_id, current_props)` for the
/// most-recent non-superseded Task claim matching `(tree, spec, id)`.
/// Returns `Ok(None)` when no such task exists.
#[allow(dead_code)]
pub fn load_current(
    store: &SynthStore,
    tree: &str,
    spec: &str,
    id: &str,
) -> Result<Option<(String, Value)>> {
    let rows = store.query(
        "SELECT id, props FROM claims \
         WHERE claim_type = 'task' \
           AND json_extract(props, '$.tree') = ?1 \
           AND json_extract(props, '$.spec') = ?2 \
           AND json_extract(props, '$.id') = ?3 \
         ORDER BY asserted_at DESC LIMIT 1",
        &[&tree, &spec, &id],
    )?;
    if let Some(row) = rows.into_iter().next() {
        let claim_id = row
            .get("id")
            .and_then(|v| v.as_str())
            .context("row missing id")?
            .to_string();
        let props_str = row
            .get("props")
            .and_then(|v| v.as_str())
            .context("row missing props")?
            .to_string();
        let props: Value = serde_json::from_str(&props_str).context("parse props")?;
        Ok(Some((claim_id, props)))
    } else {
        Ok(None)
    }
}

/// Load every current (non-superseded) task head for a `(tree, spec)`
/// as a `Vec<Value>`. Used by callers that need to operate over the
/// full DAG (`crate::task_dag::TaskDag`, `task ready`, dep validation).
pub fn load_all_current(store: &SynthStore, tree: &str, spec: &str) -> Result<Vec<Value>> {
    let rows = store.query(
        "SELECT id, props, supersedes FROM claims \
         WHERE claim_type = 'task' \
           AND json_extract(props, '$.tree') = ?1 \
           AND json_extract(props, '$.spec') = ?2 \
         ORDER BY asserted_at DESC",
        &[&tree, &spec],
    )?;
    let superseded: std::collections::HashSet<String> = rows
        .iter()
        .filter_map(|r| r.get("supersedes"))
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    let mut seen_ids: std::collections::HashSet<String> = Default::default();
    let mut out = Vec::new();
    for row in rows {
        let claim_id = row
            .get("id")
            .and_then(|v| v.as_str())
            .context("row id")?
            .to_string();
        if superseded.contains(&claim_id) {
            continue;
        }
        let props_str = row
            .get("props")
            .and_then(|v| v.as_str())
            .context("row props")?
            .to_string();
        let props: Value = serde_json::from_str(&props_str).context("parse props")?;
        let task_id = props
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if seen_ids.insert(task_id) {
            out.push(props);
        }
    }
    Ok(out)
}

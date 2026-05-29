//! Task DAG commands — ported to the claim substrate.
//!
//! Every status transition is a supersession: find the current Task
//! claim for `(tree, spec, id)`, build a new one with the updated
//! status + propagated fields, append with `supersedes: Some(prior)`.

use std::process::Command as ShellCommand;

use anyhow::{Context, Result, bail};
use nomograph_claim::{ClaimId, ClaimType};
use serde_json::{Value, json};

use crate::cli::TaskCmd;
use crate::store::{SynthStore, json_out, parse_tree_spec};

pub fn run(cmd: &TaskCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        TaskCmd::Add {
            tree_spec,
            summary,
            id,
            depends_on,
            gate,
            files,
            description,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_add(
                &tree,
                &spec,
                summary,
                id.as_deref(),
                depends_on,
                gate.as_deref(),
                files,
                description.as_deref(),
                session,
            )
        }
        TaskCmd::List {
            tree_spec,
            human: _,
            active,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_list(&tree, &spec, *active)
        }
        TaskCmd::Show { tree_spec, task_id } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_show(&tree, &spec, task_id)
        }
        TaskCmd::Update {
            tree_spec,
            task_id,
            summary,
            description,
            files,
            depends_on,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_update(
                &tree,
                &spec,
                task_id,
                summary.as_deref(),
                description.as_deref(),
                files.as_ref(),
                depends_on.as_ref(),
                session,
            )
        }
        TaskCmd::Claim { tree_spec, task_id } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_claim(&tree, &spec, task_id, session)
        }
        TaskCmd::Done {
            tree_spec,
            task_id,
            skip_verify,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_done(&tree, &spec, task_id, *skip_verify, session)
        }
        TaskCmd::Reset {
            tree_spec,
            task_id,
            session: _reset_session,
            reason,
        } => {
            if let (Some(ts), Some(tid)) = (tree_spec.as_deref(), task_id.as_deref()) {
                let (tree, spec) = parse_tree_spec(ts)?;
                cmd_task_reset(&tree, &spec, tid, reason.as_deref(), session)
            } else {
                bail!("task reset requires <tree/spec> <task_id>")
            }
        }
        TaskCmd::Block { tree_spec, task_id } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_status_transition(&tree, &spec, task_id, "blocked", None, None, session)
        }
        TaskCmd::Wait {
            tree_spec,
            task_id,
            reason,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_status_transition(
                &tree,
                &spec,
                task_id,
                "waiting",
                Some(("wait_reason", reason.as_str())),
                None,
                session,
            )
        }
        TaskCmd::Cancel {
            tree_spec,
            task_id,
            reason,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            let reason_pair = reason.as_deref().map(|r| ("failure_note", r));
            cmd_task_status_transition(
                &tree,
                &spec,
                task_id,
                "cancelled",
                reason_pair,
                None,
                session,
            )
        }
        TaskCmd::Ready { tree_spec } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_ready(&tree, &spec)
        }
        TaskCmd::Acceptance {
            tree_spec,
            task_id,
            criterion,
            verify,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_acceptance(&tree, &spec, task_id, criterion, verify, session)
        }
    }
}

/// Return `(claim_id, props)` for the currently-live Task claim for
/// `(tree, spec, id)`, or `None` if no task exists.
fn current_task(
    store: &SynthStore,
    tree: &str,
    spec: &str,
    id: &str,
) -> Result<Option<(ClaimId, Value)>> {
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

/// Dedup + filter list of Task claims for `(tree, spec)` by id.
fn list_current_tasks(store: &SynthStore, tree: &str, spec: &str) -> Result<Vec<Value>> {
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

#[allow(clippy::too_many_arguments)]
fn cmd_task_add(
    tree: &str,
    spec: &str,
    summary: &str,
    id: Option<&str>,
    depends_on: &[String],
    gate: Option<&str>,
    files: &[String],
    description: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let task_id = match id {
        Some(s) => s.to_string(),
        None => {
            // auto-generate: next tN after max existing
            let rows = list_current_tasks(&store, tree, spec)?;
            let max_n = rows
                .iter()
                .filter_map(|r| r.get("id").and_then(|v| v.as_str()))
                .filter_map(|s| s.strip_prefix('t').and_then(|n| n.parse::<u64>().ok()))
                .max()
                .unwrap_or(0);
            format!("t{}", max_n + 1)
        }
    };
    if current_task(&store, tree, spec, &task_id)?.is_some() {
        bail!("task {tree}/{spec}/{task_id} already exists");
    }
    let mut props = json!({
        "tree": tree,
        "spec": spec,
        "id": task_id,
        "summary": summary,
        "status": "pending",
        "depends_on": depends_on,
        "files": files,
    });
    if let Some(desc) = description {
        props["description"] = json!(desc);
    }
    if let Some(g) = gate {
        props["gate"] = json!(g);
    }
    store.append(ClaimType::Task, props.clone(), None)?;
    json_out(&props)
}

fn cmd_task_list(tree: &str, spec: &str, active: bool) -> Result<()> {
    let store = SynthStore::discover()?;
    let tasks = list_current_tasks(&store, tree, spec)?;
    let filtered: Vec<Value> = if active {
        tasks
            .into_iter()
            .filter(|t| {
                let s = t.get("status").and_then(|v| v.as_str()).unwrap_or("");
                matches!(s, "pending" | "in_progress" | "blocked" | "waiting")
            })
            .collect()
    } else {
        tasks
    };
    json_out(&json!({ "tasks": filtered }))
}

fn cmd_task_show(tree: &str, spec: &str, task_id: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    match current_task(&store, tree, spec, task_id)? {
        Some((_id, props)) => json_out(&props),
        None => bail!("task {tree}/{spec}/{task_id} not found"),
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_task_update(
    tree: &str,
    spec: &str,
    task_id: &str,
    summary: Option<&str>,
    description: Option<&str>,
    files: Option<&Vec<String>>,
    depends_on: Option<&Vec<String>>,
    session: &Option<String>,
) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let (prior_id, mut props) = current_task(&store, tree, spec, task_id)?
        .with_context(|| format!("task {tree}/{spec}/{task_id} not found"))?;
    if let Some(s) = summary {
        props["summary"] = json!(s);
    }
    if let Some(d) = description {
        props["description"] = json!(d);
    }
    if let Some(f) = files {
        props["files"] = json!(f);
    }
    let mut warnings: Vec<String> = Vec::new();
    if let Some(deps_raw) = depends_on {
        // value_delimiter = ',' yields [""] for an empty value; treat
        // that as "clear deps" rather than a one-element [""] list.
        let deps: Vec<String> = deps_raw
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let snapshot = crate::task_mutate::load_all_current(&store, tree, spec)?;
        let dag = crate::task_dag::TaskDag::from_snapshot(&snapshot);
        let validation = dag
            .validate_proposed_deps(task_id, &deps, tree, spec)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        for dep in &validation.cancelled_deps {
            warnings.push(format!(
                "depending on cancelled task {dep}; the new dep will be a dead node in the DAG"
            ));
        }
        props["depends_on"] = json!(deps);
    }
    store.append(ClaimType::Task, props.clone(), Some(prior_id))?;
    crate::output::emit(crate::output::Output::new(props).warns(warnings))
}

fn cmd_task_claim(tree: &str, spec: &str, task_id: &str, session: &Option<String>) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let (prior_id, mut props) = current_task(&store, tree, spec, task_id)?
        .with_context(|| format!("task {tree}/{spec}/{task_id} not found"))?;
    let status = props
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("pending");
    if status != "pending" {
        bail!("task {tree}/{spec}/{task_id} status is {status}; cannot claim (must be pending)");
    }
    props["status"] = json!("in_progress");
    let owner = session
        .clone()
        .unwrap_or_else(|| std::env::var("USER").unwrap_or_else(|_| "unknown".into()));
    props["owner"] = json!(owner);
    store.append(ClaimType::Task, props.clone(), Some(prior_id))?;
    json_out(&props)
}

fn cmd_task_done(
    tree: &str,
    spec: &str,
    task_id: &str,
    skip_verify: bool,
    session: &Option<String>,
) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let (prior_id, mut props) = current_task(&store, tree, spec, task_id)?
        .with_context(|| format!("task {tree}/{spec}/{task_id} not found"))?;
    if !skip_verify
        && let Some(acceptance) = props.get("acceptance").and_then(|v| v.as_array()).cloned()
    {
        for crit in &acceptance {
            let verify_cmd = crit
                .get("verify_cmd")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if verify_cmd.is_empty() {
                continue;
            }
            let status = ShellCommand::new("sh")
                .arg("-c")
                .arg(verify_cmd)
                .status()
                .with_context(|| format!("run acceptance: {verify_cmd}"))?;
            if !status.success() {
                bail!(
                    "acceptance check failed: {}; pass --skip-verify to override",
                    verify_cmd
                );
            }
        }
    }
    props["status"] = json!("done");
    store.append(ClaimType::Task, props.clone(), Some(prior_id))?;
    json_out(&props)
}

fn cmd_task_reset(
    tree: &str,
    spec: &str,
    task_id: &str,
    reason: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let (prior_id, mut props) = current_task(&store, tree, spec, task_id)?
        .with_context(|| format!("task {tree}/{spec}/{task_id} not found"))?;
    props["status"] = json!("pending");
    props["owner"] = Value::Null;
    if let Some(r) = reason {
        props["reset_reason"] = json!(r);
    }
    store.append(ClaimType::Task, props.clone(), Some(prior_id))?;
    json_out(&props)
}

fn cmd_task_status_transition(
    tree: &str,
    spec: &str,
    task_id: &str,
    new_status: &str,
    extra_field: Option<(&str, &str)>,
    _clear: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let (prior_id, mut props) = current_task(&store, tree, spec, task_id)?
        .with_context(|| format!("task {tree}/{spec}/{task_id} not found"))?;
    props["status"] = json!(new_status);
    if let Some((k, v)) = extra_field {
        props[k] = json!(v);
    }
    store.append(ClaimType::Task, props.clone(), Some(prior_id))?;
    json_out(&props)
}

fn cmd_task_ready(tree: &str, spec: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    let tasks = list_current_tasks(&store, tree, spec)?;
    // Build id -> status map
    let status_by_id: std::collections::HashMap<String, String> = tasks
        .iter()
        .filter_map(|t| {
            let id = t.get("id").and_then(|v| v.as_str())?.to_string();
            let s = t.get("status").and_then(|v| v.as_str())?.to_string();
            Some((id, s))
        })
        .collect();
    let mut ready: Vec<Value> = tasks
        .iter()
        .filter(|t| {
            let s = t.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if s != "pending" {
                return false;
            }
            let deps = t
                .get("depends_on")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            deps.iter()
                .filter_map(|d| d.as_str())
                .all(|d| status_by_id.get(d).map(|s| s.as_str()) == Some("done"))
        })
        .cloned()
        .collect();

    // Annotate each ready task with plan_at_risk: true when its parent spec
    // is flagged by the plan-at-risk overlay. The overlay invocation is
    // non-fatal: if the view is unavailable or the overlay errors, we skip
    // annotation silently and return the task list as-is.
    if let Some(at_risk_set) = build_at_risk_set() {
        for task in &mut ready {
            let task_spec = task.get("spec").and_then(|v| v.as_str()).unwrap_or("");
            if at_risk_set.contains(task_spec) {
                task["plan_at_risk"] = json!(true);
            }
        }
    }

    json_out(&json!({ "ready": ready }))
}

/// Build a set of raw spec ids (e.g. "deploy", "cms") that are currently
/// plan-at-risk, by running the plan-at-risk overlay against the graph view.
///
/// Returns `None` if the view cannot be opened or the overlay fails.
/// The caller treats `None` as "no annotation available" and omits the flag.
fn build_at_risk_set() -> Option<std::collections::HashSet<String>> {
    // Try to open the graph view the same way cmd_query does: on-disk first,
    // fall back to an in-memory rebuild from the claims log union.
    let view = open_graph_view_best_effort()?;

    // Look up the plan-at-risk overlay via the registry so we don't need
    // to import the private submodule directly.
    let overlay = crate::overlay::find("plan-at-risk")?;
    let hits = overlay.run(&view).ok()?;
    Some(at_risk_set_from_hits(&hits))
}

/// Extract raw spec ids from a slice of overlay results.
///
/// Each `OverlayResult.detail` is expected to carry a `"spec_id"` string
/// field (the `synth:id` literal of the at-risk spec). Results whose
/// `spec_id` is absent or empty are silently ignored.
///
/// Extracted as a pure helper so it can be unit tested independently of
/// the graph view and overlay infrastructure.
fn at_risk_set_from_hits(hits: &[crate::overlay::OverlayResult]) -> std::collections::HashSet<String> {
    let mut at_risk = std::collections::HashSet::new();
    for hit in hits {
        let raw_id = hit
            .detail
            .get("spec_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !raw_id.is_empty() {
            at_risk.insert(raw_id);
        }
    }
    at_risk
}

/// Open the graph view using the same on-disk-then-in-memory fallback that
/// `cmd_query` uses. Returns `None` on any error so callers can degrade
/// gracefully.
fn open_graph_view_best_effort() -> Option<nomograph_claim::graph_view::GraphView> {
    use nomograph_claim::graph_view::{GraphView, rebuild};

    // Locate the claims directory by walking up from cwd.
    let start = std::env::current_dir().ok()?;
    let claims_dir = {
        let mut cur = start.as_path();
        loop {
            let candidate = cur.join("claims");
            if candidate.is_dir() {
                break Some(candidate);
            }
            match cur.parent() {
                Some(p) => cur = p,
                None => break None,
            }
        }
    }?;

    let view_dir = claims_dir.join("_view.oxigraph");

    // Prefer on-disk view; fall back to in-memory rebuild.
    match GraphView::open(&view_dir) {
        Ok(v) => Some(v),
        Err(_) => {
            let v = GraphView::open_in_memory().ok()?;
            rebuild(&v, &claims_dir).ok()?;
            Some(v)
        }
    }
}

fn cmd_task_acceptance(
    tree: &str,
    spec: &str,
    task_id: &str,
    criterion: &str,
    verify: &str,
    session: &Option<String>,
) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let (prior_id, mut props) = current_task(&store, tree, spec, task_id)?
        .with_context(|| format!("task {tree}/{spec}/{task_id} not found"))?;
    let mut acceptance = props
        .get("acceptance")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    acceptance.push(json!({ "criterion": criterion, "verify_cmd": verify }));
    props["acceptance"] = Value::Array(acceptance);
    store.append(ClaimType::Task, props.clone(), Some(prior_id))?;
    json_out(&props)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::OverlayResult;
    use serde_json::json;

    // ------------------------------------------------------------------
    // T8.2 Acceptance criterion 1:
    //   at_risk_set_from_hits returns the spec id when detail.spec_id is set.
    // ------------------------------------------------------------------
    #[test]
    fn at_risk_set_includes_spec_id_from_hit_detail() {
        let hit = OverlayResult::with_detail(
            "https://nomograph.org/synth/claim/abc123",
            "synth:planAtRisk",
            "https://nomograph.org/synth/claim/xyz",
            json!({
                "old_claim": "https://nomograph.org/synth/claim/old",
                "stakeholder": "asserter:user:local:agd",
                "new_at": "2026-05-10T09:00:00.000Z",
                "spec_id": "deploy",
            }),
        );

        let set = at_risk_set_from_hits(&[hit]);
        assert!(
            set.contains("deploy"),
            "expected 'deploy' in at-risk set, got: {:?}",
            set
        );
    }

    // ------------------------------------------------------------------
    // T8.2 Acceptance criterion 2:
    //   at_risk_set_from_hits omits entries where spec_id is absent
    //   or empty (non-risk specs produce no entry in the set).
    // ------------------------------------------------------------------
    #[test]
    fn at_risk_set_omits_entry_when_spec_id_absent() {
        // A hit with no spec_id in detail (absent field).
        let hit_no_id = OverlayResult::with_detail(
            "https://nomograph.org/synth/claim/abc",
            "synth:planAtRisk",
            "https://nomograph.org/synth/claim/new",
            json!({
                "old_claim": "https://nomograph.org/synth/claim/old",
                "new_at": "2026-05-10T09:00:00.000Z",
            }),
        );

        // A hit with an empty spec_id.
        let hit_empty_id = OverlayResult::with_detail(
            "https://nomograph.org/synth/claim/def",
            "synth:planAtRisk",
            "https://nomograph.org/synth/claim/new2",
            json!({
                "old_claim": "https://nomograph.org/synth/claim/old2",
                "new_at": "2026-05-11T09:00:00.000Z",
                "spec_id": "",
            }),
        );

        let set = at_risk_set_from_hits(&[hit_no_id, hit_empty_id]);
        assert!(
            set.is_empty(),
            "expected empty at-risk set for hits without spec_id, got: {:?}",
            set
        );
    }

    // ------------------------------------------------------------------
    // Multiple hits for the same spec id deduplicate correctly.
    // ------------------------------------------------------------------
    #[test]
    fn at_risk_set_deduplicates_same_spec_id() {
        let make_hit = |spec_id: &str| {
            OverlayResult::with_detail(
                "https://nomograph.org/synth/claim/some-spec",
                "synth:planAtRisk",
                "https://nomograph.org/synth/claim/new",
                json!({ "spec_id": spec_id }),
            )
        };
        // Two hits for "cms", one for "deploy".
        let hits = vec![make_hit("cms"), make_hit("cms"), make_hit("deploy")];
        let set = at_risk_set_from_hits(&hits);
        assert_eq!(set.len(), 2, "expected 2 unique ids, got: {:?}", set);
        assert!(set.contains("cms"));
        assert!(set.contains("deploy"));
    }

    // ------------------------------------------------------------------
    // Empty hit slice returns empty set.
    // ------------------------------------------------------------------
    #[test]
    fn at_risk_set_from_empty_hits_is_empty() {
        let set = at_risk_set_from_hits(&[]);
        assert!(set.is_empty());
    }
}

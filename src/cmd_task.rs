//! Task DAG commands -- ported to the v3 SPARQL substrate.
//!
//! Reference port (Stage 1). `cmd_task_ready` is the load-bearing one
//! and the SPARQL pattern subsequent task ports will mimic.

use std::process::Command as ShellCommand;

use anyhow::{Context, Result, anyhow, bail};
use nomograph_claim::ClaimType;
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
        TaskCmd::List { tree_spec, .. } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_list(&tree, &spec)
        }
        TaskCmd::Show { tree_spec, task_id } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_show(&tree, &spec, task_id)
        }
        TaskCmd::Update { .. } => {
            bail!("task update: TODO PATH-B (write-side update not yet ported)")
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
        TaskCmd::Reset { .. } => bail!("task reset: TODO PATH-B"),
        TaskCmd::Block { tree_spec, task_id } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_status_transition(&tree, &spec, task_id, "blocked", None, session)
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
            cmd_task_status_transition(&tree, &spec, task_id, "cancelled", reason_pair, session)
        }
        TaskCmd::Ready { tree_spec } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_ready(&tree, &spec)
        }
        TaskCmd::Acceptance { .. } => bail!("task acceptance: TODO PATH-B"),
    }
}

/// Live Task heads for `(tree, spec)`. Returns `(prior_claim_id, props)`
/// for each. Shared by every task command in this module.
fn live_tasks(store: &SynthStore, tree: &str, spec: &str) -> Result<Vec<(String, Value)>> {
    let q = format!(
        r#"
        SELECT ?c ?id ?status ?summary ?description ?gate
               (GROUP_CONCAT(?dep; SEPARATOR="\u001F") AS ?deps)
               (GROUP_CONCAT(?file; SEPARATOR="\u001F") AS ?files)
        WHERE {{
          GRAPH ?g {{
            ?c rdf:type synthesist:Task ;
               synthesist:tree   "{tree}" ;
               synthesist:spec   "{spec}" ;
               synthesist:id     ?id ;
               synthesist:status ?status .
            OPTIONAL {{ ?c synthesist:summary     ?summary }}
            OPTIONAL {{ ?c synthesist:description ?description }}
            OPTIONAL {{ ?c synthesist:gate        ?gate }}
            OPTIONAL {{ ?c synthesist:dependsOn   ?dep }}
            OPTIONAL {{ ?c synthesist:files       ?file }}
            FILTER NOT EXISTS {{
              GRAPH ?g2 {{ ?later synthesist:supersedes ?c }}
            }}
          }}
        }}
        GROUP BY ?c ?id ?status ?summary ?description ?gate
        ORDER BY ?id
        "#
    );
    let r = store.sparql(&q)?;
    let mut out: Vec<(String, Value)> = Vec::new();
    for row in &r.rows {
        use nomograph_claim::graph_view::Term;
        let claim_iri = match row.first() {
            Some(Term::Iri(s)) => s.clone(),
            _ => continue,
        };
        let prior_id = short_claim_id(&claim_iri);
        let str_at = |i: usize| -> Option<String> {
            match row.get(i) {
                Some(Term::Literal { value, .. }) if !value.is_empty() => Some(value.clone()),
                _ => None,
            }
        };
        let id = match str_at(1) {
            Some(s) => s,
            None => continue,
        };
        let status = str_at(2).unwrap_or_default();
        let summary = str_at(3);
        let description = str_at(4);
        let gate = str_at(5);
        let deps_concat = str_at(6).unwrap_or_default();
        let files_concat = str_at(7).unwrap_or_default();

        let deps: Vec<Value> = if deps_concat.is_empty() {
            Vec::new()
        } else {
            deps_concat
                .split('\u{001F}')
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect()
        };
        let files: Vec<Value> = if files_concat.is_empty() {
            Vec::new()
        } else {
            files_concat
                .split('\u{001F}')
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect()
        };

        let mut props = serde_json::Map::new();
        props.insert("tree".into(), json!(tree));
        props.insert("spec".into(), json!(spec));
        props.insert("id".into(), json!(id));
        props.insert("status".into(), json!(status));
        if let Some(s) = summary {
            props.insert("summary".into(), json!(s));
        }
        if let Some(s) = description {
            props.insert("description".into(), json!(s));
        }
        if let Some(s) = gate {
            props.insert("gate".into(), json!(s));
        }
        props.insert("depends_on".into(), Value::Array(deps));
        props.insert("files".into(), Value::Array(files));
        out.push((prior_id, Value::Object(props)));
    }
    Ok(out)
}

fn find_task(
    store: &SynthStore,
    tree: &str,
    spec: &str,
    task_id: &str,
) -> Result<Option<(String, Value)>> {
    Ok(live_tasks(store, tree, spec)?
        .into_iter()
        .find(|(_, p)| p.get("id").and_then(|v| v.as_str()) == Some(task_id)))
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
    let existing = live_tasks(&store, tree, spec)?;
    let task_id = match id {
        Some(s) => s.to_string(),
        None => {
            let max_n = existing
                .iter()
                .filter_map(|(_, p)| p.get("id").and_then(|v| v.as_str()))
                .filter_map(|s| s.strip_prefix('t').and_then(|n| n.parse::<u64>().ok()))
                .max()
                .unwrap_or(0);
            format!("t{}", max_n + 1)
        }
    };
    if existing
        .iter()
        .any(|(_, p)| p.get("id").and_then(|v| v.as_str()) == Some(task_id.as_str()))
    {
        bail!(
            "task {tree}/{spec}/{task_id} already exists; \
             use `synthesist task show {tree}/{spec} {task_id}` to inspect it"
        );
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

fn cmd_task_list(tree: &str, spec: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    let tasks: Vec<Value> = live_tasks(&store, tree, spec)?
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    json_out(&json!({ "tasks": tasks }))
}

fn cmd_task_show(tree: &str, spec: &str, task_id: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    match find_task(&store, tree, spec, task_id)? {
        Some((_id, props)) => json_out(&props),
        None => bail!(
            "task {tree}/{spec}/{task_id} not found; \
             list tasks with `synthesist task list {tree}/{spec}`"
        ),
    }
}

fn cmd_task_claim(tree: &str, spec: &str, task_id: &str, session: &Option<String>) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let (prior_id, mut props) = find_task(&store, tree, spec, task_id)?
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
    let (prior_id, mut props) = find_task(&store, tree, spec, task_id)?
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

fn cmd_task_status_transition(
    tree: &str,
    spec: &str,
    task_id: &str,
    new_status: &str,
    extra_field: Option<(&str, &str)>,
    session: &Option<String>,
) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let (prior_id, mut props) = find_task(&store, tree, spec, task_id)?
        .with_context(|| format!("task {tree}/{spec}/{task_id} not found"))?;
    props["status"] = json!(new_status);
    if let Some((k, v)) = extra_field {
        props[k] = json!(v);
    }
    store.append(ClaimType::Task, props.clone(), Some(prior_id))?;
    json_out(&props)
}

/// Reference port: `synthesist task ready <tree>/<spec>`. Returns the
/// tasks whose status is pending and whose every depends_on entry
/// resolves to a live task with status=done in the same (tree, spec).
fn cmd_task_ready(tree: &str, spec: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    let tasks: Vec<Value> = live_tasks(&store, tree, spec)?
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    let status_by_id: std::collections::HashMap<String, String> = tasks
        .iter()
        .filter_map(|t| {
            let id = t.get("id").and_then(|v| v.as_str())?.to_string();
            let s = t.get("status").and_then(|v| v.as_str())?.to_string();
            Some((id, s))
        })
        .collect();
    let mut ready: Vec<Value> = tasks
        .into_iter()
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
        .collect();

    // Annotate each ready task with plan_at_risk: true when its parent
    // spec is flagged by the plan-at-risk overlay.
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

fn build_at_risk_set() -> Option<std::collections::HashSet<String>> {
    let view = open_graph_view_best_effort()?;
    let overlay = crate::overlay::find("plan-at-risk")?;
    let hits = overlay.run(&view).ok()?;
    Some(at_risk_set_from_hits(&hits))
}

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

fn open_graph_view_best_effort() -> Option<nomograph_claim::graph_view::GraphView> {
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
    nomograph_claim::graph_view::GraphView::open_or_in_memory(&view_dir, &claims_dir).ok()
}

fn short_claim_id(iri: &str) -> String {
    iri.strip_prefix("https://nomograph.org/synthesist/claim/")
        .or_else(|| iri.strip_prefix("synthesist:claim/"))
        .unwrap_or(iri)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::OverlayResult;
    use serde_json::json;

    #[test]
    fn at_risk_set_includes_spec_id_from_hit_detail() {
        let hit = OverlayResult::with_detail(
            "https://nomograph.org/synthesist/claim/abc123",
            "synthesist:planAtRisk",
            "https://nomograph.org/synthesist/claim/xyz",
            json!({
                "old_claim": "https://nomograph.org/synthesist/claim/old",
                "stakeholder": "asserter:user:local:agd",
                "new_at": "2026-05-10T09:00:00.000Z",
                "spec_id": "deploy",
            }),
        );

        let set = at_risk_set_from_hits(&[hit]);
        assert!(set.contains("deploy"));
    }

    #[test]
    fn at_risk_set_omits_entry_when_spec_id_absent() {
        let hit_no_id = OverlayResult::with_detail(
            "https://nomograph.org/synthesist/claim/abc",
            "synthesist:planAtRisk",
            "https://nomograph.org/synthesist/claim/new",
            json!({
                "old_claim": "https://nomograph.org/synthesist/claim/old",
                "new_at": "2026-05-10T09:00:00.000Z",
            }),
        );
        let hit_empty_id = OverlayResult::with_detail(
            "https://nomograph.org/synthesist/claim/def",
            "synthesist:planAtRisk",
            "https://nomograph.org/synthesist/claim/new2",
            json!({
                "old_claim": "https://nomograph.org/synthesist/claim/old2",
                "new_at": "2026-05-11T09:00:00.000Z",
                "spec_id": "",
            }),
        );
        let set = at_risk_set_from_hits(&[hit_no_id, hit_empty_id]);
        assert!(set.is_empty());
    }

    #[test]
    fn at_risk_set_deduplicates_same_spec_id() {
        let make_hit = |spec_id: &str| {
            OverlayResult::with_detail(
                "https://nomograph.org/synthesist/claim/some-spec",
                "synthesist:planAtRisk",
                "https://nomograph.org/synthesist/claim/new",
                json!({ "spec_id": spec_id }),
            )
        };
        let hits = vec![make_hit("cms"), make_hit("cms"), make_hit("deploy")];
        let set = at_risk_set_from_hits(&hits);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn at_risk_set_from_empty_hits_is_empty() {
        assert!(at_risk_set_from_hits(&[]).is_empty());
    }
}

//! `synthesist serve` — local HTTP browser for the claim graph.
//!
//! Multi-person ergonomics: Josh and other reviewers open the page,
//! see the current estate, drill in via progressive disclosure. The
//! page is server-rendered HTML with `<details>` for collapse. Every
//! request re-queries the claim view, so refreshes show the latest
//! state without persistent server state.
//!
//! Routes:
//!   GET /             -- full dashboard (trees, sessions, summary)
//!   GET /api/state    -- same data as JSON (agent-readable)
//!   GET /api/graph    -- network graph data as JSON (nodes + edges
//!                        for the d3-force network view)
//!   GET /events       -- SSE stream that ticks on every claims/changes/
//!                        filesystem event (push-based refresh)
//!
//! No timed polling. The client subscribes to /events and only
//! re-fetches when the server signals a change.

use anyhow::{Context, Result};
use axum::{
    Router,
    response::{Html, Json, Sse, sse::Event},
    routing::get,
};
use futures_util::stream::Stream;
use notify::{Event as NotifyEvent, EventKind, RecursiveMode, Watcher};
use serde_json::Value;
use std::convert::Infallible;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

use crate::store::SynthStore;

const DEFAULT_PORT: u16 = 5179;
/// Coalesce filesystem events that arrive within this window into a
/// single SSE tick. Multiple claims often land within a few ms of
/// each other (a `task done` writes 2-3 claims in quick succession);
/// firing one event covers all of them.
const COALESCE_MS: u64 = 250;

pub fn run(port: Option<u16>, bind_all: bool) -> Result<()> {
    // Bridge sync caller into the async runtime. main.rs is sync; this
    // is the only async entry point we need.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    rt.block_on(serve_async(port, bind_all))
}

async fn serve_async(port: Option<u16>, bind_all: bool) -> Result<()> {
    let port = port.unwrap_or(DEFAULT_PORT);
    let host = if bind_all { "0.0.0.0" } else { "127.0.0.1" };
    let addr: std::net::SocketAddr = format!("{host}:{port}")
        .parse()
        .with_context(|| format!("parse bind addr {host}:{port}"))?;

    // Resolve claims_dir for the watcher. We re-discover it through
    // the store so the same env-var precedence applies as everywhere
    // else in synthesist. Store::root() returns the claims/ dir.
    let store = SynthStore::discover().context("discover synthesist data dir")?;
    let claims_changes = store.root().join("changes");
    drop(store);

    let (tx, _) = broadcast::channel::<()>(16);
    spawn_fs_watcher(claims_changes.clone(), tx.clone())?;

    let app = Router::new()
        .route("/", get(handle_dashboard))
        .route("/api/state", get(handle_state_json))
        .route("/api/graph", get(handle_graph_json))
        .route("/events", get(handle_events))
        .with_state(tx);

    eprintln!("synthesist serve listening on http://{addr}");
    eprintln!("  GET /          -- dashboard");
    eprintln!("  GET /api/state -- state as JSON");
    eprintln!("  GET /api/graph -- network graph as JSON");
    eprintln!("  GET /events    -- SSE (push on fs change)");
    eprintln!("watching {} for changes", claims_changes.display());
    eprintln!("press ctrl-c to stop");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            eprintln!("\nstopping serve");
        })
        .await
        .context("axum serve")?;
    Ok(())
}

fn spawn_fs_watcher(target: PathBuf, tx: broadcast::Sender<()>) -> Result<()> {
    if !target.is_dir() {
        eprintln!(
            "warning: claims/changes/ not found at {}; serve will run but won't push live updates",
            target.display()
        );
        return Ok(());
    }
    // Channel from notify thread → coalescing task.
    let (raw_tx, mut raw_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut watcher = notify::recommended_watcher(move |res: Result<NotifyEvent, _>| {
        if let Ok(ev) = res
            && matches!(
                ev.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            )
        {
            let _ = raw_tx.send(());
        }
    })
    .context("create fs watcher")?;
    watcher
        .watch(&target, RecursiveMode::NonRecursive)
        .with_context(|| format!("watch {}", target.display()))?;

    // Coalesce bursts of events into a single tick. Hold the watcher
    // alive in the spawned task — dropping it stops the watch.
    tokio::spawn(async move {
        let _watcher = watcher;
        loop {
            // Block for the first event in the burst.
            if raw_rx.recv().await.is_none() {
                break;
            }
            // Drain anything that arrives within COALESCE_MS so a
            // multi-claim write produces one notification.
            let coalesce = tokio::time::sleep(Duration::from_millis(COALESCE_MS));
            tokio::pin!(coalesce);
            loop {
                tokio::select! {
                    _ = &mut coalesce => break,
                    msg = raw_rx.recv() => if msg.is_none() { return; },
                }
            }
            let _ = tx.send(());
        }
    });
    Ok(())
}

async fn handle_dashboard(
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let view = q
        .get("view")
        .cloned()
        .unwrap_or_else(|| "trees".to_string());
    match tokio::task::spawn_blocking(collect_state).await {
        Ok(Ok(state)) => Html(render_dashboard_with_view(&state, &view)).into_response(),
        Ok(Err(e)) => render_error(&format!("error: {e}")),
        Err(e) => render_error(&format!("join error: {e}")),
    }
}

async fn handle_state_json() -> axum::response::Response {
    match tokio::task::spawn_blocking(collect_state).await {
        Ok(Ok(state)) => Json(state).into_response(),
        Ok(Err(e)) => render_error(&format!("error: {e}")),
        Err(e) => render_error(&format!("join error: {e}")),
    }
}

async fn handle_graph_json() -> axum::response::Response {
    match tokio::task::spawn_blocking(collect_state).await {
        Ok(Ok(state)) => Json(build_graph(&state)).into_response(),
        Ok(Err(e)) => render_error(&format!("error: {e}")),
        Err(e) => render_error(&format!("join error: {e}")),
    }
}

fn render_error(msg: &str) -> axum::response::Response {
    use axum::http::StatusCode;
    (StatusCode::INTERNAL_SERVER_ERROR, msg.to_string()).into_response()
}

async fn handle_events(
    axum::extract::State(tx): axum::extract::State<broadcast::Sender<()>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|res| {
        match res {
            Ok(()) => Some(Ok(Event::default().event("change").data("changed"))),
            // Lagged: client missed events because the broadcast
            // buffer was full. Send a single "refresh anyway" tick;
            // we don't try to enumerate what was missed.
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_)) => {
                Some(Ok(Event::default().event("change").data("lagged")))
            }
        }
    });
    // Initial event so the client renders fresh on connect.
    let initial = futures_util::stream::once(async {
        Ok::<_, Infallible>(Event::default().event("change").data("initial"))
    });
    Sse::new(initial.chain(stream)).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

use axum::response::IntoResponse as _;

#[derive(Debug, serde::Serialize)]
struct State {
    data_dir: String,
    version: String,
    generated_at: String,
    recent: Vec<RecentClaim>,
    trees: Vec<TreeView>,
    sessions: Vec<SessionView>,
}

#[derive(Debug, serde::Serialize)]
struct RecentClaim {
    asserted_at: i64,
    asserted_by: String,
    claim_type: String,
    summary: String,
}

const RECENT_LIMIT: i64 = 20;

#[derive(Debug, serde::Serialize)]
struct GraphView {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

#[derive(Debug, serde::Serialize)]
struct GraphNode {
    /// Stable id, scoped by kind (tree:keaton, spec:keaton/foo, session:s1).
    id: String,
    label: String,
    /// One of: tree, spec, session.
    kind: String,
    /// Tree the node belongs to (for color clustering). For trees,
    /// it's the tree itself. For sessions, the session's tree if any.
    tree: Option<String>,
    /// Status (active/done/closed/...) where applicable.
    status: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct GraphEdge {
    source: String,
    target: String,
    /// One of: contains, asserted.
    kind: String,
}

/// Build a graph projection of the estate. Nodes: trees, specs, sessions.
/// Edges: tree-contains-spec; session-asserted-on-spec/tree.
///
/// Sessions without a tree/spec scope are EXCLUDED — they have no
/// edges, would be disconnected components, and the force layout
/// would scatter them far from the clusters. The point of the
/// network view is to surface connections; isolated nodes hurt
/// more than they help.
fn build_graph(state: &State) -> GraphView {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    for tree in &state.trees {
        nodes.push(GraphNode {
            id: format!("tree:{}", tree.name),
            label: tree.name.clone(),
            kind: "tree".into(),
            tree: Some(tree.name.clone()),
            status: None,
        });
        for spec in &tree.specs {
            let spec_id = format!("spec:{}/{}", tree.name, spec.id);
            nodes.push(GraphNode {
                id: spec_id.clone(),
                label: spec.id.clone(),
                kind: "spec".into(),
                tree: Some(tree.name.clone()),
                status: Some(spec.status.clone()),
            });
            edges.push(GraphEdge {
                source: format!("tree:{}", tree.name),
                target: spec_id,
                kind: "contains".into(),
            });
        }
    }

    for sess in &state.sessions {
        // Only include sessions with a known scope; otherwise they
        // would be disconnected nodes scattered far from clusters.
        if sess.tree.is_none() && sess.spec.is_none() {
            continue;
        }
        let sid = format!("session:{}", sess.id);
        nodes.push(GraphNode {
            id: sid.clone(),
            label: sess.id.clone(),
            kind: "session".into(),
            tree: sess.tree.clone(),
            status: Some(sess.status.clone()),
        });
        match (&sess.tree, &sess.spec) {
            (Some(t), Some(sp)) => {
                edges.push(GraphEdge {
                    source: sid,
                    target: format!("spec:{}/{}", t, sp),
                    kind: "asserted".into(),
                });
            }
            (Some(t), None) => {
                edges.push(GraphEdge {
                    source: sid,
                    target: format!("tree:{}", t),
                    kind: "asserted".into(),
                });
            }
            _ => {}
        }
    }

    GraphView { nodes, edges }
}

#[derive(Debug, serde::Serialize)]
struct TreeView {
    name: String,
    description: String,
    specs: Vec<SpecView>,
    session_count: usize,
}

#[derive(Debug, serde::Serialize)]
struct SpecView {
    id: String,
    goal: String,
    status: String,
    tasks: Vec<TaskView>,
    discoveries: Vec<DiscoveryView>,
}

#[derive(Debug, serde::Serialize)]
struct TaskView {
    id: String,
    summary: String,
    status: String,
    owner: Option<String>,
    depends_on: Vec<String>,
    gate: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct DiscoveryView {
    id: String,
    finding: String,
    impact: Option<String>,
    date: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct SessionView {
    id: String,
    tree: Option<String>,
    spec: Option<String>,
    summary: Option<String>,
    status: String,
    started_at: Option<i64>,
    claim_count: usize,
    /// Most-recent first. Capped at SESSION_CLAIM_LIMIT to keep
    /// the rendered page bounded; the count above is exact.
    claims: Vec<SessionClaim>,
}

#[derive(Debug, serde::Serialize)]
struct SessionClaim {
    asserted_at: i64,
    claim_type: String,
    summary: String,
}

const SESSION_CLAIM_LIMIT: i64 = 50;

fn collect_state() -> Result<State> {
    let store = SynthStore::discover().context("discover synthesist data dir")?;

    let trees_rows = store.query(
        "SELECT props FROM claims WHERE claim_type = 'tree' ORDER BY asserted_at",
        &[],
    )?;
    let mut trees: Vec<TreeView> = Vec::new();
    let mut tree_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for row in trees_rows {
        let props = parse_props(&row);
        let name = props
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() || tree_names.contains(&name) {
            continue;
        }
        tree_names.insert(name.clone());
        let description = props
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let specs = collect_specs(&store, &name)?;
        let session_count = count_sessions_for_tree(&store, &name)?;
        trees.push(TreeView {
            name,
            description,
            specs,
            session_count,
        });
    }

    let sessions = collect_sessions(&store)?;

    let data_dir = std::env::var("SYNTHESIST_DIR")
        .ok()
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_else(|| "(discovered)".to_string());

    let recent = collect_recent_claims(&store)?;

    let _ = store; // Keep store alive until we read all data above.
    Ok(State {
        data_dir,
        version: env!("CARGO_PKG_VERSION").to_string(),
        generated_at: now_human(),
        recent,
        trees,
        sessions,
    })
}

fn collect_recent_claims(store: &SynthStore) -> Result<Vec<RecentClaim>> {
    let limit = RECENT_LIMIT;
    let rows = store.query(
        "SELECT asserted_at, asserted_by, claim_type, props FROM claims \
         ORDER BY asserted_at DESC LIMIT ?1",
        &[&limit],
    )?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let asserted_at = row.get("asserted_at").and_then(|v| v.as_i64()).unwrap_or(0);
            let asserted_by = row
                .get("asserted_by")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let claim_type = row
                .get("claim_type")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            let props = parse_props(&row);
            let summary = summarize_claim_props(&claim_type, &props);
            RecentClaim {
                asserted_at,
                asserted_by,
                claim_type,
                summary,
            }
        })
        .collect())
}

/// Extract the session id from an asserter string of the form
/// `user:local:<USER>:<session-id>`. Returns None if the asserter
/// doesn't match the expected shape.
fn session_from_asserter(asserter: &str) -> Option<&str> {
    asserter.rsplit(':').next().filter(|s| !s.is_empty())
}

fn collect_specs(store: &SynthStore, tree: &str) -> Result<Vec<SpecView>> {
    let rows = store.query(
        "SELECT id, props, supersedes FROM claims \
         WHERE claim_type = 'spec' AND json_extract(props, '$.tree') = ?1 \
         ORDER BY asserted_at DESC",
        &[&tree],
    )?;
    let superseded: std::collections::HashSet<String> = rows
        .iter()
        .filter_map(|r| {
            r.get("supersedes")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();
    let mut by_spec: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    for row in rows {
        let id = row
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if superseded.contains(&id) {
            continue;
        }
        let props = parse_props(&row);
        let spec_id = props
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if spec_id.is_empty() || by_spec.contains_key(&spec_id) {
            continue;
        }
        by_spec.insert(spec_id, Value::Object(props));
    }

    let mut specs = Vec::new();
    for (spec_id, props) in by_spec {
        let goal = props
            .get("goal")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let status = props
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let tasks = collect_tasks(store, tree, &spec_id)?;
        let discoveries = collect_discoveries(store, tree, &spec_id)?;
        specs.push(SpecView {
            id: spec_id,
            goal,
            status,
            tasks,
            discoveries,
        });
    }
    Ok(specs)
}

fn collect_tasks(store: &SynthStore, tree: &str, spec: &str) -> Result<Vec<TaskView>> {
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
        .filter_map(|r| {
            r.get("supersedes")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();
    let mut by_task: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    for row in rows {
        let id = row
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if superseded.contains(&id) {
            continue;
        }
        let props = parse_props(&row);
        let task_id = props
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if task_id.is_empty() || by_task.contains_key(&task_id) {
            continue;
        }
        by_task.insert(task_id, Value::Object(props));
    }
    let mut tasks: Vec<TaskView> = by_task
        .into_iter()
        .map(|(id, props)| TaskView {
            id,
            summary: props
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            status: props
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending")
                .to_string(),
            owner: props
                .get("owner")
                .and_then(|v| v.as_str())
                .map(String::from),
            depends_on: props
                .get("depends_on")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            gate: props.get("gate").and_then(|v| v.as_str()).map(String::from),
        })
        .collect();
    tasks.sort_by(|a, b| natural_id_order(&a.id, &b.id));
    Ok(tasks)
}

fn collect_discoveries(store: &SynthStore, tree: &str, spec: &str) -> Result<Vec<DiscoveryView>> {
    let rows = store.query(
        "SELECT id, props FROM claims \
         WHERE claim_type = 'discovery' \
           AND json_extract(props, '$.tree') = ?1 \
           AND json_extract(props, '$.spec') = ?2 \
         ORDER BY asserted_at DESC",
        &[&tree, &spec],
    )?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let id = row
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let props = parse_props(&row);
            DiscoveryView {
                id,
                finding: props
                    .get("finding")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                impact: props
                    .get("impact")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                date: props.get("date").and_then(|v| v.as_str()).map(String::from),
            }
        })
        .collect())
}

fn count_sessions_for_tree(store: &SynthStore, tree: &str) -> Result<usize> {
    let rows = store.query(
        "SELECT COUNT(*) AS n FROM claims \
         WHERE claim_type = 'session' AND json_extract(props, '$.tree') = ?1",
        &[&tree],
    )?;
    Ok(rows
        .first()
        .and_then(|r| r.get("n"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as usize)
}

fn collect_sessions(store: &SynthStore) -> Result<Vec<SessionView>> {
    let rows = store.query(
        "SELECT id, props, supersedes, asserted_at FROM claims \
         WHERE claim_type = 'session' \
         ORDER BY asserted_at DESC",
        &[],
    )?;
    let superseded: std::collections::HashSet<String> = rows
        .iter()
        .filter_map(|r| {
            r.get("supersedes")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();
    // Closed signal: a session is closed when its session_id has a
    // claim that supersedes another. cmd_session_close re-writes
    // identical props with `supersedes` pointing at the opener — no
    // status field is added — so the only signal is the supersession.
    let mut by_id: std::collections::BTreeMap<String, (Value, i64, bool)> =
        std::collections::BTreeMap::new();
    for row in rows {
        let id = row
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if superseded.contains(&id) {
            continue;
        }
        let props = parse_props(&row);
        let session_id = props
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if session_id.is_empty() || by_id.contains_key(&session_id) {
            continue;
        }
        let started = row.get("asserted_at").and_then(|v| v.as_i64()).unwrap_or(0);
        let is_closer = row
            .get("supersedes")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());
        by_id.insert(session_id, (Value::Object(props), started, is_closer));
    }
    let mut out: Vec<SessionView> = Vec::new();
    for (sid, (props, started, is_closer)) in by_id {
        let status = if is_closer { "closed" } else { "active" }.to_string();
        let claim_count = count_claims_by_session(store, &sid)?;
        let claims = collect_session_claims(store, &sid)?;
        out.push(SessionView {
            id: sid,
            tree: props.get("tree").and_then(|v| v.as_str()).map(String::from),
            spec: props.get("spec").and_then(|v| v.as_str()).map(String::from),
            summary: props
                .get("summary")
                .and_then(|v| v.as_str())
                .map(String::from),
            status,
            started_at: Some(started),
            claim_count,
            claims,
        });
    }
    // Sort: active first, then by started_at descending.
    out.sort_by(|a, b| {
        let active_order = (a.status != "active") as u8;
        let active_b = (b.status != "active") as u8;
        active_order
            .cmp(&active_b)
            .then(b.started_at.unwrap_or(0).cmp(&a.started_at.unwrap_or(0)))
    });
    Ok(out)
}

fn count_claims_by_session(store: &SynthStore, session: &str) -> Result<usize> {
    let pattern = format!("%:{session}");
    let rows = store.query(
        "SELECT COUNT(*) AS n FROM claims WHERE asserted_by LIKE ?1",
        &[&pattern.as_str()],
    )?;
    Ok(rows
        .first()
        .and_then(|r| r.get("n"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as usize)
}

fn collect_session_claims(store: &SynthStore, session: &str) -> Result<Vec<SessionClaim>> {
    let pattern = format!("%:{session}");
    let limit = SESSION_CLAIM_LIMIT;
    let rows = store.query(
        "SELECT asserted_at, claim_type, props FROM claims \
         WHERE asserted_by LIKE ?1 \
         ORDER BY asserted_at DESC LIMIT ?2",
        &[&pattern.as_str(), &limit],
    )?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let asserted_at = row.get("asserted_at").and_then(|v| v.as_i64()).unwrap_or(0);
            let claim_type = row
                .get("claim_type")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            let props = parse_props(&row);
            let summary = summarize_claim_props(&claim_type, &props);
            SessionClaim {
                asserted_at,
                claim_type,
                summary,
            }
        })
        .collect())
}

/// One-line description of a claim, leaning on whichever props
/// matter most for that claim_type. Keeps the session drill-down
/// scannable without dumping raw JSON.
fn summarize_claim_props(claim_type: &str, props: &serde_json::Map<String, Value>) -> String {
    let s = |k: &str| {
        props
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    match claim_type {
        "task" => {
            let id = s("id");
            let tree = s("tree");
            let spec = s("spec");
            let status = s("status");
            let summary = s("summary");
            let path = if !tree.is_empty() && !spec.is_empty() {
                format!("{tree}/{spec}/{id}")
            } else {
                id
            };
            format!("{path} [{status}] {summary}")
        }
        "spec" => {
            let id = s("id");
            let tree = s("tree");
            let goal = s("goal");
            let status = s("status");
            format!("{tree}/{id} [{status}] {goal}")
        }
        "tree" => format!("{} — {}", s("name"), s("description")),
        "discovery" => {
            let tree = s("tree");
            let spec = s("spec");
            let finding = s("finding");
            format!("{tree}/{spec}: {finding}")
        }
        "session" => {
            let id = s("id");
            let summary = s("summary");
            format!("{id}: {summary}")
        }
        "phase" => {
            let phase = s("phase");
            let session = s("session");
            format!("phase={phase} session={session}")
        }
        "campaign" => {
            let id = s("id");
            let title = s("title");
            format!("{id}: {title}")
        }
        _ => {
            // Fallback: key=value of first 3 string fields.
            let mut parts = Vec::new();
            for (k, v) in props.iter().take(3) {
                if let Some(vs) = v.as_str() {
                    parts.push(format!("{k}={vs}"));
                }
            }
            parts.join(" · ")
        }
    }
}

fn parse_props(row: &Value) -> serde_json::Map<String, Value> {
    row.get("props")
        .and_then(|v| v.as_str())
        .and_then(|s| serde_json::from_str(s).ok())
        .and_then(|v: Value| v.as_object().cloned())
        .unwrap_or_default()
}

fn natural_id_order(a: &str, b: &str) -> std::cmp::Ordering {
    // t1, t2, ... t10 -> numeric sort by trailing number.
    let an: Option<u64> = a
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok();
    let bn: Option<u64> = b
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok();
    match (an, bn) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => a.cmp(b),
    }
}

fn now_human() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

/// Render an asserted_at unix-ms timestamp as a relative duration
/// from now: `5s`, `12m`, `3h`, `2d`. Capped at days for older
/// entries; precision is intentionally coarse.
fn relative_time(asserted_at_ms: i64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let mut delta = (now_ms - asserted_at_ms) / 1000; // seconds
    if delta < 0 {
        delta = 0;
    }
    if delta < 60 {
        return format!("{delta}s");
    }
    let minutes = delta / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 48 {
        return format!("{hours}h");
    }
    let days = hours / 24;
    format!("{days}d")
}

fn render_dashboard_with_view(state: &State, view: &str) -> String {
    let mut s = String::with_capacity(64 * 1024);
    s.push_str("<!doctype html>\n<html lang=\"en\"><head>");
    s.push_str("<meta charset=\"utf-8\">");
    s.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
    s.push_str("<title>synthesist · ");
    s.push_str(&html_escape(&state.data_dir));
    s.push_str("</title>");
    s.push_str(STYLE);
    s.push_str("</head><body>");

    // Header — nomograph mark, page title, meta + chrome
    s.push_str("<header>");
    s.push_str(NOM_MARK);
    s.push_str("<h1>synthesist</h1>");
    s.push_str(&format!(
        "<div class=\"meta\"><code>{}</code> · v{} <span id=\"live-status\" class=\"live-on\">· connecting</span> <button id=\"live-toggle\" class=\"chrome-btn\" type=\"button\">pause</button> <button id=\"refresh-now\" class=\"chrome-btn\" type=\"button\">refresh</button></div>",
        html_escape(&state.data_dir),
        html_escape(&state.version),
    ));
    s.push_str("</header>");

    // View tabs
    s.push_str("<nav class=\"view-tabs\">");
    for (key, label) in &[("trees", "Trees"), ("network", "Network")] {
        let active = if view == *key { " active" } else { "" };
        s.push_str(&format!(
            "<a href=\"?view={}\" class=\"view-tab{}\">{}</a>",
            key, active, label
        ));
    }
    s.push_str("</nav>");

    if view == "network" {
        render_network_view(&mut s);
        s.push_str(SCRIPT);
        s.push_str("</body></html>");
        return s;
    }
    // Default: trees view (existing dashboard sections)
    render_trees_view(&mut s, state);
    s.push_str(SCRIPT);
    s.push_str("</body></html>");
    s
}

fn render_trees_view(s: &mut String, state: &State) {
    // session_id -> tree (for tagging recent activity rows + now-band rendering)
    let session_tree: std::collections::HashMap<&str, &str> = state
        .sessions
        .iter()
        .filter_map(|s| s.tree.as_deref().map(|t| (s.id.as_str(), t)))
        .collect();

    // "Now" band — most-recently-active sessions with their last claim.
    // Standalone humans landing on the page see what's in flight at a glance,
    // without expanding any tree.
    render_now_band(s, state);

    // Recent activity (cross-cutting)
    if !state.recent.is_empty() {
        s.push_str(&format!(
            "<section class=\"recent\"><details open id=\"section:recent\"><summary><span class=\"section-title\">recent</span> <span class=\"count\">{} claims</span></summary>",
            state.recent.len()
        ));
        for r in &state.recent {
            let session = session_from_asserter(&r.asserted_by).unwrap_or("");
            let tree = session_tree.get(session).copied().unwrap_or("");
            let when = relative_time(r.asserted_at);
            let tree_tag = if tree.is_empty() {
                String::new()
            } else {
                format!(
                    " <span class=\"recent-tree\">{}</span>",
                    html_escape(tree)
                )
            };
            s.push_str(&format!(
                "<div class=\"recent-row\"><span class=\"recent-when\">{}</span> <span class=\"claim-type\">{}</span>{} <span class=\"recent-session muted\">@{}</span> <span class=\"claim-summary\">{}</span></div>",
                html_escape(&when),
                html_escape(&r.claim_type),
                tree_tag,
                html_escape(session),
                html_escape(&r.summary),
            ));
        }
        s.push_str("</details></section>");
    }

    // Trees
    s.push_str(&format!(
        "<section class=\"trees\"><details open id=\"section:trees\"><summary><span class=\"section-title\">trees</span> <span class=\"count\">{}</span></summary>",
        state.trees.len()
    ));
    for tree in &state.trees {
        s.push_str(&render_tree(tree));
    }
    s.push_str("</details></section>");

    // Sessions
    let active_n = state
        .sessions
        .iter()
        .filter(|s| s.status == "active")
        .count();
    let closed_n = state.sessions.len() - active_n;
    s.push_str(&format!(
        "<section class=\"sessions\"><details open id=\"section:sessions\"><summary><span class=\"section-title\">sessions</span> <span class=\"count\">{} <span class=\"muted\">/ {} active · {} closed</span></span></summary>",
        state.sessions.len(),
        active_n,
        closed_n,
    ));
    for sess in &state.sessions {
        s.push_str(&render_session(sess));
    }
    s.push_str("</details></section>");

    s.push_str("<footer>generated unix:");
    s.push_str(&html_escape(&state.generated_at));
    s.push_str(" · <a href=\"/api/state\">/api/state</a> for JSON</footer>");
}

/// "Now" band — top of dashboard, shows the N most-recently-active
/// sessions with their scope and last claim. Optimized for a human
/// landing on the page cold and wanting to see what's in flight.
fn render_now_band(s: &mut String, state: &State) {
    const NOW_LIMIT: usize = 6;
    let mut active: Vec<&SessionView> = state
        .sessions
        .iter()
        .filter(|s| s.status == "active" && !s.claims.is_empty())
        .collect();
    active.sort_by(|a, b| {
        let a_at = a.claims.first().map(|c| c.asserted_at).unwrap_or(0);
        let b_at = b.claims.first().map(|c| c.asserted_at).unwrap_or(0);
        b_at.cmp(&a_at)
    });
    if active.is_empty() {
        return;
    }
    let total_active = active.len();
    let shown = active.len().min(NOW_LIMIT);
    let overflow = total_active - shown;
    s.push_str(&format!(
        "<section class=\"now\"><div class=\"now-head\"><span class=\"section-title\">in flight</span> <span class=\"count\">{} active</span></div><div class=\"now-rows\">",
        total_active,
    ));
    for sess in active.iter().take(NOW_LIMIT) {
        let scope = match (&sess.tree, &sess.spec) {
            (Some(t), Some(sp)) => format!("{}/{}", t, sp),
            (Some(t), None) => t.clone(),
            _ => "—".to_string(),
        };
        let (when, last_summary, last_type) = match sess.claims.first() {
            Some(c) => (
                relative_time(c.asserted_at),
                c.summary.clone(),
                c.claim_type.clone(),
            ),
            None => ("—".to_string(), String::new(), String::new()),
        };
        s.push_str(&format!(
            "<a class=\"now-row\" href=\"#session:{}\"><span class=\"now-when\">{}</span> <span class=\"now-id\">{}</span> <span class=\"now-scope muted\">{}</span> <span class=\"claim-type\">{}</span> <span class=\"now-summary\">{}</span></a>",
            html_escape(&sess.id),
            html_escape(&when),
            html_escape(&sess.id),
            html_escape(&scope),
            html_escape(&last_type),
            html_escape(&last_summary),
        ));
    }
    if overflow > 0 {
        s.push_str(&format!(
            "<div class=\"now-overflow muted\">+{overflow} more active sessions below</div>"
        ));
    }
    s.push_str("</div></section>");
}

fn render_network_view(s: &mut String) {
    s.push_str(
        r#"<section class="network"><div class="network-shell">
        <div id="graph-status" class="muted">loading graph…</div>
        <svg id="graph-canvas" viewBox="0 0 800 600" preserveAspectRatio="xMidYMid meet"></svg>
        <div class="legend">
          <span class="legend-item"><span class="legend-dot dot-tree"></span>tree</span>
          <span class="legend-item"><span class="legend-dot dot-spec"></span>spec</span>
          <span class="legend-item"><span class="legend-dot dot-session"></span>session</span>
          <span class="legend-item"><span class="legend-edge edge-contains"></span>contains</span>
          <span class="legend-item"><span class="legend-edge edge-asserted"></span>asserted</span>
        </div>
        <div id="graph-detail" class="graph-detail"></div>
      </div></section>"#,
    );
    s.push_str("<footer><a href=\"/api/graph\">/api/graph</a> for JSON</footer>");
}

/// Bucketed task counts for a slice of tasks. Statuses are normalized
/// into five buckets that humans actually care about when scanning a
/// spec or tree summary: done, in-flight (claimed/in_progress), ready
/// (pending, no human gate), gated (pending, waiting on human), blocked.
/// Tasks in unknown states (deferred/cancelled/superseded) are counted
/// in `other` and only shown if non-zero.
#[derive(Default, Debug, Clone, Copy)]
struct TaskCounts {
    done: usize,
    in_flight: usize,
    ready: usize,
    gated: usize,
    blocked: usize,
    other: usize,
}

impl TaskCounts {
    fn add(&mut self, other: TaskCounts) {
        self.done += other.done;
        self.in_flight += other.in_flight;
        self.ready += other.ready;
        self.gated += other.gated;
        self.blocked += other.blocked;
        self.other += other.other;
    }
    fn total(self) -> usize {
        self.done + self.in_flight + self.ready + self.gated + self.blocked + self.other
    }
    fn render_pills(self) -> String {
        if self.total() == 0 {
            return "<span class=\"muted\">· 0 tasks</span>".to_string();
        }
        let mut out = String::new();
        let push = |out: &mut String, n: usize, cls: &str, label: &str| {
            if n > 0 {
                out.push_str(&format!(
                    " <span class=\"pill pill-{cls}\">{n} {label}</span>"
                ));
            }
        };
        push(&mut out, self.done, "done", "done");
        push(&mut out, self.in_flight, "in-flight", "in-flight");
        push(&mut out, self.ready, "ready", "ready");
        push(&mut out, self.gated, "gated", "gated");
        push(&mut out, self.blocked, "blocked", "blocked");
        push(&mut out, self.other, "other", "other");
        out
    }
}

fn count_tasks(tasks: &[TaskView]) -> TaskCounts {
    let mut c = TaskCounts::default();
    for t in tasks {
        match t.status.as_str() {
            "done" | "completed" => c.done += 1,
            "in_progress" | "claimed" | "active" => c.in_flight += 1,
            "blocked" => c.blocked += 1,
            "pending" => {
                if t.gate.as_deref() == Some("human") {
                    c.gated += 1;
                } else {
                    c.ready += 1;
                }
            }
            _ => c.other += 1,
        }
    }
    c
}

fn count_tasks_in_tree(t: &TreeView) -> TaskCounts {
    let mut c = TaskCounts::default();
    for sp in &t.specs {
        c.add(count_tasks(&sp.tasks));
    }
    c
}

fn render_tree(t: &TreeView) -> String {
    let mut s = String::new();
    let counts = count_tasks_in_tree(t);
    s.push_str(&format!(
        "<details class=\"tree\" id=\"tree:{}\"><summary><span class=\"name\">{}</span> <span class=\"muted\">· {} specs · {} sessions</span>{}</summary>",
        html_escape(&t.name),
        html_escape(&t.name),
        t.specs.len(),
        t.session_count,
        counts.render_pills(),
    ));
    if !t.description.is_empty() {
        s.push_str(&format!(
            "<p class=\"desc\">{}</p>",
            html_escape(&t.description)
        ));
    }
    if !t.specs.is_empty() {
        s.push_str("<div class=\"indent\">");
        for spec in &t.specs {
            s.push_str(&render_spec(spec));
        }
        s.push_str("</div>");
    }
    s.push_str("</details>");
    s
}

fn render_spec(sp: &SpecView) -> String {
    let mut s = String::new();
    let counts = count_tasks(&sp.tasks);
    s.push_str(&format!(
        "<details class=\"spec\" id=\"spec:{}\"><summary><span class=\"name\">{}</span> <span class=\"status status-{}\">{}</span>{}</summary>",
        html_escape(&sp.id),
        html_escape(&sp.id),
        html_escape(&sp.status),
        html_escape(&sp.status),
        counts.render_pills(),
    ));
    if !sp.goal.is_empty() {
        s.push_str(&format!("<p class=\"desc\">{}</p>", html_escape(&sp.goal)));
    }
    if !sp.tasks.is_empty() {
        s.push_str(&format!(
            "<div class=\"indent\"><details id=\"spec-tasks:{}\"><summary><span class=\"section-sub\">tasks</span> <span class=\"count\">",
            html_escape(&sp.id)
        ));
        s.push_str(&sp.tasks.len().to_string());
        s.push_str("</span></summary>");
        for t in &sp.tasks {
            s.push_str(&render_task(t));
        }
        s.push_str("</details></div>");
    }
    if !sp.discoveries.is_empty() {
        s.push_str(&format!(
            "<div class=\"indent\"><details id=\"spec-discoveries:{}\"><summary><span class=\"section-sub\">discoveries</span> <span class=\"count\">",
            html_escape(&sp.id)
        ));
        s.push_str(&sp.discoveries.len().to_string());
        s.push_str("</span></summary>");
        for d in &sp.discoveries {
            s.push_str(&render_discovery(d));
        }
        s.push_str("</details></div>");
    }
    s.push_str("</details>");
    s
}

fn render_task(t: &TaskView) -> String {
    let gate = t
        .gate
        .as_deref()
        .map(|g| format!(" <span class=\"gate\">⛔ {}</span>", html_escape(g)))
        .unwrap_or_default();
    let deps = if t.depends_on.is_empty() {
        String::new()
    } else {
        format!(
            " <span class=\"deps muted\">← {}</span>",
            html_escape(&t.depends_on.join(", "))
        )
    };
    let owner = t
        .owner
        .as_deref()
        .map(|o| format!(" <span class=\"muted owner\">@{}</span>", html_escape(o)))
        .unwrap_or_default();
    format!(
        "<div class=\"task\"><span class=\"task-id\">{}</span> <span class=\"status status-{}\">{}</span>{} <span class=\"task-summary\">{}</span>{}{}</div>",
        html_escape(&t.id),
        html_escape(&t.status),
        html_escape(&t.status),
        gate,
        html_escape(&t.summary),
        deps,
        owner,
    )
}

fn render_discovery(d: &DiscoveryView) -> String {
    let impact = d
        .impact
        .as_deref()
        .map(|i| format!(" <span class=\"impact\">{}</span>", html_escape(i)))
        .unwrap_or_default();
    let date = d
        .date
        .as_deref()
        .map(|x| format!(" <span class=\"muted\">{}</span>", html_escape(x)))
        .unwrap_or_default();
    format!(
        "<div class=\"discovery\"><span class=\"discovery-id\">{}</span>{}{}<p class=\"discovery-body\">{}</p></div>",
        html_escape(&d.id),
        impact,
        date,
        html_escape(&d.finding),
    )
}

fn render_session(sess: &SessionView) -> String {
    let scope = match (&sess.tree, &sess.spec) {
        (Some(t), Some(s)) => format!(
            " <span class=\"muted\">· {}/{}</span>",
            html_escape(t),
            html_escape(s)
        ),
        (Some(t), None) => format!(" <span class=\"muted\">· {}</span>", html_escape(t)),
        _ => String::new(),
    };
    let summary = sess
        .summary
        .as_deref()
        .map(|s| format!("<p class=\"desc\">{}</p>", html_escape(s)))
        .unwrap_or_default();

    let mut claims_html = String::new();
    if !sess.claims.is_empty() {
        claims_html.push_str(&format!(
            "<div class=\"indent\"><details id=\"session-claims:{}\"><summary><span class=\"section-sub\">claims</span> <span class=\"count\">",
            html_escape(&sess.id)
        ));
        let limited = sess.claim_count > sess.claims.len();
        if limited {
            claims_html.push_str(&format!(
                "{} of {} (most recent)",
                sess.claims.len(),
                sess.claim_count
            ));
        } else {
            claims_html.push_str(&sess.claims.len().to_string());
        }
        claims_html.push_str("</span></summary>");
        for c in &sess.claims {
            claims_html.push_str(&format!(
                "<div class=\"claim\"><span class=\"claim-type\">{}</span> <span class=\"claim-summary\">{}</span></div>",
                html_escape(&c.claim_type),
                html_escape(&c.summary),
            ));
        }
        claims_html.push_str("</details></div>");
    }

    format!(
        "<details class=\"session\" id=\"session:{}\"><summary><span class=\"name\">{}</span> <span class=\"status status-{}\">{}</span>{} <span class=\"muted\">· {} claims</span></summary>{}{}</details>",
        html_escape(&sess.id),
        html_escape(&sess.id),
        html_escape(&sess.status),
        html_escape(&sess.status),
        scope,
        sess.claim_count,
        summary,
        claims_html,
    )
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

const STYLE: &str = r#"
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=DM+Sans:wght@400;500;600&family=DM+Serif+Display&family=JetBrains+Mono:wght@400;500;600&display=swap" rel="stylesheet">
<style>
  :root {
    /* Nomograph palette — gitlab.com/nomograph/design/tokens/colors.css */
    --paper-50:  #faf9f7;
    --paper-100: #f2f0eb;
    --paper-200: #e2e0da;
    --paper-300: #c8c5bc;
    --paper-400: #9c9890;
    --paper-500: #706d66;
    --ink-300: #5c5c5c;
    --ink-400: #3d3d3d;
    --ink-500: #1a1a1a;
    --ink-600: #111111;
    --ink-700: #080808;
    --steel-100: #dde4e9;
    --steel-200: #b8c6cf;
    --steel-300: #7d9aaa;
    --steel-400: #4a6072;
    --steel-500: #364858;
    --steel-600: #243040;

    /* Semantic mappings */
    --bg: var(--paper-50);
    --bg-soft: var(--paper-100);
    --bg-tint: var(--paper-200);
    --border: var(--paper-200);
    --border-strong: var(--paper-300);
    --fg: var(--ink-500);
    --fg-secondary: var(--ink-400);
    --fg-muted: var(--paper-500);

    --accent: var(--steel-400);
    --accent-soft: var(--steel-200);
    --accent-mute: color-mix(in srgb, var(--steel-300) 8%, transparent);
    --accent-mute-strong: color-mix(in srgb, var(--steel-300) 18%, transparent);

    /* Status semantics — layered on top of paper, not pure RGB */
    --status-active: var(--steel-400);
    --status-in_progress: #b95a18;
    --status-done: #427c50;
    --status-completed: #427c50;
    --status-blocked: #b03a3a;
    --status-pending: var(--paper-400);
    --status-deferred: var(--paper-400);
    --status-closed: var(--paper-400);
    --status-cancelled: var(--paper-400);
    --status-superseded: var(--paper-400);
    --status-abandoned: #b03a3a;
    --gate: #b03a3a;

    /* Section accents — give each top-level section a distinct hue */
    --accent-now: #427c50;
    --accent-recent: #8a6f3a;
    --accent-trees: var(--steel-400);
    --accent-sessions: #5e5b8a;

    --serif: "DM Serif Display", Georgia, "Times New Roman", serif;
    --sans:  "DM Sans", -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    --mono:  "JetBrains Mono", ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
  }
  @media (prefers-color-scheme: dark) {
    :root {
      --bg: var(--ink-600);
      --bg-soft: #1a1f24;
      --bg-tint: #232830;
      --border: #2a3038;
      --border-strong: #3a4250;
      --fg: var(--paper-100);
      --fg-secondary: var(--paper-200);
      --fg-muted: var(--paper-400);
      --accent: var(--steel-300);
      --accent-soft: var(--steel-500);
      --accent-mute: color-mix(in srgb, var(--steel-300) 10%, transparent);
      --accent-mute-strong: color-mix(in srgb, var(--steel-300) 22%, transparent);
      --accent-now: #6fb38a;
      --accent-recent: #c4a26b;
      --accent-trees: var(--steel-300);
      --accent-sessions: #9b97c9;
    }
  }
  * { box-sizing: border-box; }
  body {
    font-family: var(--sans);
    background: var(--bg);
    color: var(--fg);
    margin: 0;
    padding: 1.5rem 2rem 3rem;
    max-width: 110ch;
    margin-left: auto;
    margin-right: auto;
    font-size: 14px;
    line-height: 1.5;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
  }

  /* ───────── Header ─────────────────────────────────────────── */
  header {
    margin-bottom: 1.5rem;
    padding-bottom: 1rem;
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: baseline;
    gap: 0.85rem;
    flex-wrap: wrap;
  }
  .nom-mark { display: inline-block; width: 30px; height: 30px; flex: 0 0 30px; align-self: center; color: var(--accent); }
  .nom-mark svg { width: 100%; height: 100%; display: block; }
  h1 {
    margin: 0;
    font-family: var(--serif);
    font-weight: 400;
    font-size: 1.6rem;
    letter-spacing: 0.01em;
    color: var(--fg);
  }
  .meta {
    font-family: var(--mono);
    font-size: 0.78rem;
    color: var(--fg-muted);
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    flex-wrap: wrap;
  }
  .meta code {
    background: var(--bg-soft);
    padding: 0.12em 0.4em;
    border-radius: 3px;
    color: var(--fg-secondary);
  }

  /* ───────── Sections (callout pattern) ─────────────────────── */
  section {
    margin: 1.25rem 0;
    border-left: 3px solid var(--section-accent, var(--accent));
    background: var(--section-tint, var(--accent-mute));
    padding: 0.85rem 0.85rem 0.85rem 1rem;
    border-radius: 0 4px 4px 0;
  }
  section.now      { --section-accent: var(--accent-now);      --section-tint: color-mix(in srgb, var(--accent-now) 8%, transparent); }
  section.recent   { --section-accent: var(--accent-recent);   --section-tint: color-mix(in srgb, var(--accent-recent) 6%, transparent); }
  section.trees    { --section-accent: var(--accent-trees);    --section-tint: color-mix(in srgb, var(--accent-trees) 6%, transparent); }
  section.sessions { --section-accent: var(--accent-sessions); --section-tint: color-mix(in srgb, var(--accent-sessions) 6%, transparent); }

  /* ───────── Now band ───────────────────────────────────────── */
  .now-head { display: flex; align-items: baseline; gap: 0.5rem; margin-bottom: 0.5rem; }
  .now-rows { display: flex; flex-direction: column; gap: 0.2rem; }
  .now-row {
    display: grid;
    grid-template-columns: 3.2rem 11rem minmax(8rem, 16rem) auto 1fr;
    gap: 0.55rem;
    align-items: baseline;
    padding: 0.25rem 0.4rem;
    border-radius: 3px;
    font-family: var(--mono);
    font-size: 0.82rem;
    text-decoration: none;
    color: var(--fg);
    line-height: 1.4;
  }
  .now-row:hover { background: color-mix(in srgb, var(--accent-now) 10%, transparent); }
  .now-when { color: var(--fg-muted); }
  .now-id { color: var(--fg); font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .now-scope { color: var(--fg-muted); font-size: 0.78rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .now-summary { color: var(--fg-secondary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .now-overflow { font-family: var(--mono); font-size: 0.78rem; padding: 0.3rem 0.4rem 0; }

  /* ───────── Status pills ───────────────────────────────────── */
  .pill {
    display: inline-block;
    padding: 0.05em 0.45em;
    margin-left: 0.25em;
    border-radius: 9px;
    font-family: var(--mono);
    font-size: 0.7rem;
    font-weight: 500;
    line-height: 1.4;
    border: 1px solid color-mix(in srgb, currentColor 35%, transparent);
    background: color-mix(in srgb, currentColor 10%, transparent);
  }
  .pill-done      { color: var(--status-done); }
  .pill-in-flight { color: var(--status-in_progress); }
  .pill-ready     { color: var(--accent); }
  .pill-gated     { color: #8a6f3a; }
  .pill-blocked   { color: var(--status-blocked); }
  .pill-other     { color: var(--paper-400); }
  @media (prefers-color-scheme: dark) {
    .pill-gated   { color: #c4a26b; }
  }

  /* Recent-row tree tag */
  .recent-tree {
    display: inline-block;
    padding: 0 0.4em;
    border-radius: 3px;
    font-family: var(--mono);
    font-size: 0.7rem;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--fg-secondary);
  }

  /* ───────── Disclosure ─────────────────────────────────────── */
  details { margin: 0.15rem 0; }
  details > summary {
    cursor: pointer;
    padding: 0.2rem 0;
    list-style: none;
    user-select: none;
  }
  details > summary::-webkit-details-marker { display: none; }
  details > summary::before {
    content: "▸";
    color: var(--paper-400);
    display: inline-block;
    width: 1em;
    margin-right: 0.15em;
    transition: transform 0.1s ease;
  }
  details[open] > summary::before { content: "▾"; color: var(--accent); }

  /* ───────── Text styles ────────────────────────────────────── */
  .section-title {
    font-family: var(--serif);
    font-weight: 400;
    font-size: 1.05rem;
    color: var(--fg);
    letter-spacing: 0.01em;
  }
  .section-sub {
    font-family: var(--mono);
    font-size: 0.78rem;
    color: var(--fg-secondary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .count {
    font-family: var(--mono);
    font-size: 0.75rem;
    color: var(--fg-muted);
  }
  .muted {
    color: var(--fg-muted);
    font-family: var(--mono);
    font-size: 0.78rem;
  }
  .name {
    font-family: var(--mono);
    font-weight: 500;
    color: var(--fg);
  }
  .desc {
    color: var(--fg-secondary);
    font-size: 0.85rem;
    margin: 0.3rem 0 0.5rem 1.5em;
    font-family: var(--sans);
    line-height: 1.45;
    max-width: 75ch;
  }
  .indent {
    padding-left: 1.5em;
    border-left: 1px dashed var(--border-strong);
    margin-left: 0.4em;
  }

  /* Trees > tree > spec > tasks/discoveries */
  .tree    > summary { font-size: 0.95rem; padding: 0.35rem 0; }
  .tree    > summary .name { font-weight: 600; font-size: 0.95rem; }
  .spec    > summary { font-size: 0.88rem; padding: 0.25rem 0; }
  .session > summary { font-size: 0.9rem; padding: 0.3rem 0; }

  /* ───────── Status pills ──────────────────────────────────── */
  .status {
    font-family: var(--mono);
    font-size: 0.65rem;
    padding: 0.15em 0.55em;
    border-radius: 10px;
    text-transform: lowercase;
    font-weight: 500;
    letter-spacing: 0.03em;
    color: var(--paper-50);
    margin: 0 0.2em;
    vertical-align: 1px;
  }
  .status-active     { background: var(--status-active); }
  .status-closed     { background: var(--paper-300); color: var(--fg-secondary); }
  .status-in_progress { background: var(--status-in_progress); }
  .status-done       { background: var(--status-done); }
  .status-completed  { background: var(--status-completed); }
  .status-pending    {
    background: transparent;
    color: var(--fg-muted);
    border: 1px solid var(--border-strong);
  }
  .status-blocked    { background: var(--status-blocked); }
  .status-deferred   { background: var(--paper-300); color: var(--fg-secondary); }
  .status-cancelled  { background: var(--paper-300); color: var(--fg-secondary); }
  .status-superseded { background: var(--paper-300); color: var(--fg-secondary); }
  .status-abandoned  { background: var(--status-abandoned); }
  .gate { color: var(--gate); font-family: var(--mono); font-size: 0.7rem; }

  /* ───────── Task / discovery / claim rows ─────────────────── */
  .task {
    padding: 0.18rem 0;
    padding-left: 1.5em;
    font-family: var(--sans);
    font-size: 0.85rem;
    line-height: 1.4;
  }
  .task-id { font-family: var(--mono); color: var(--fg-muted); margin-right: 0.4em; font-size: 0.75rem; }
  .task-summary { color: var(--fg); }
  .deps, .owner { font-size: 0.7rem; }

  .discovery {
    padding: 0.4rem 0.6rem;
    margin: 0.3rem 0 0.3rem 1.5em;
    border-left: 2px solid var(--accent-soft);
    background: var(--accent-mute);
    border-radius: 0 3px 3px 0;
  }
  .discovery-id { font-family: var(--mono); color: var(--fg-muted); font-size: 0.7rem; }
  .discovery-body {
    margin: 0.2rem 0 0;
    font-size: 0.85rem;
    font-family: var(--serif);
    font-style: italic;
    color: var(--fg-secondary);
    line-height: 1.45;
  }
  .impact {
    font-family: var(--mono);
    font-size: 0.65rem;
    padding: 0.1em 0.45em;
    border-radius: 3px;
    background: var(--bg-tint);
    color: var(--fg-secondary);
    margin-left: 0.4em;
  }

  .claim {
    padding: 0.16rem 0;
    padding-left: 1.5em;
    font-size: 0.8rem;
    line-height: 1.4;
  }
  .claim-type {
    font-family: var(--mono);
    color: var(--fg-muted);
    display: inline-block;
    min-width: 7ch;
    font-size: 0.72rem;
  }
  .claim-summary { font-family: var(--sans); color: var(--fg-secondary); }

  /* ───────── Recent activity rows ──────────────────────────── */
  .recent-row {
    padding: 0.22rem 0;
    font-size: 0.8rem;
    padding-left: 1em;
    line-height: 1.4;
    display: flex;
    gap: 0.5rem;
    align-items: baseline;
  }
  .recent-row + .recent-row {
    border-top: 1px dashed color-mix(in srgb, var(--accent-recent) 18%, transparent);
  }
  .recent-when {
    font-family: var(--mono);
    color: var(--accent-recent);
    font-weight: 500;
    min-width: 3.5ch;
    text-align: right;
    font-size: 0.72rem;
    flex: 0 0 auto;
  }
  .recent-session {
    font-size: 0.72rem;
    flex: 0 0 auto;
  }

  /* ───────── Footer ────────────────────────────────────────── */
  footer {
    margin-top: 2.5rem;
    padding-top: 0.85rem;
    border-top: 1px solid var(--border);
    font-family: var(--mono);
    font-size: 0.7rem;
    color: var(--fg-muted);
  }
  footer a { color: var(--accent); text-decoration: none; }
  footer a:hover { text-decoration: underline; }

  /* ───────── Chrome (header buttons) ───────────────────────── */
  .chrome-btn {
    font-family: var(--mono);
    font-size: 0.7rem;
    padding: 0.2em 0.7em;
    margin-left: 0.3em;
    border-radius: 4px;
    border: 1px solid var(--border-strong);
    background: var(--bg-soft);
    color: var(--fg-secondary);
    cursor: pointer;
    transition: background 0.1s, border-color 0.1s, color 0.1s;
  }
  .chrome-btn:hover {
    background: var(--accent-mute);
    border-color: var(--accent-soft);
    color: var(--fg);
  }
  .live-on  { color: var(--status-done); font-weight: 500; }
  .live-off { color: var(--fg-muted); }

  /* ───────── View tabs ─────────────────────────────────────── */
  .view-tabs {
    display: flex;
    gap: 0.4rem;
    margin: -0.5rem 0 1rem;
    border-bottom: 1px solid var(--border);
    padding-bottom: 0;
  }
  .view-tab {
    font-family: var(--mono);
    font-size: 0.78rem;
    text-transform: lowercase;
    letter-spacing: 0.04em;
    padding: 0.45rem 0.85rem;
    border: 1px solid transparent;
    border-bottom: none;
    border-radius: 4px 4px 0 0;
    text-decoration: none;
    color: var(--fg-muted);
    margin-bottom: -1px;
    background: transparent;
  }
  .view-tab:hover { color: var(--fg); background: var(--accent-mute); }
  .view-tab.active {
    color: var(--fg);
    border-color: var(--border);
    border-bottom: 1px solid var(--bg);
    background: var(--bg);
    font-weight: 500;
  }

  /* ───────── Network view ──────────────────────────────────── */
  section.network { padding: 0.5rem; border-left: 3px solid var(--accent-trees); background: var(--accent-mute); }
  .network-shell { position: relative; }
  #graph-canvas {
    width: 100%;
    height: 70vh;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    cursor: grab;
  }
  #graph-canvas:active { cursor: grabbing; }
  #graph-status {
    position: absolute;
    top: 0.5rem;
    left: 0.6rem;
    font-family: var(--mono);
    font-size: 0.72rem;
    color: var(--fg-muted);
    pointer-events: none;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
    margin-top: 0.5rem;
    font-family: var(--mono);
    font-size: 0.72rem;
    color: var(--fg-muted);
  }
  .legend-item { display: inline-flex; align-items: center; gap: 0.35em; }
  .legend-dot { display: inline-block; width: 10px; height: 10px; border-radius: 50%; }
  .legend-edge { display: inline-block; width: 18px; height: 0; border-top: 2px solid; }
  .dot-tree { background: var(--accent-trees); width: 14px; height: 14px; }
  .dot-spec { background: var(--steel-300); }
  .dot-session { background: var(--accent-sessions); }
  .edge-contains { border-top-style: solid;  border-top-color: var(--paper-400); }
  .edge-asserted { border-top-style: dashed; border-top-color: var(--accent-sessions); }
  .graph-detail {
    margin-top: 0.75rem;
    padding: 0.6rem 0.8rem;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: 0.85rem;
    min-height: 2.5rem;
    color: var(--fg-secondary);
  }
  .graph-detail.empty { color: var(--fg-muted); font-style: italic; }
  /* SVG node + edge classes */
  .gnode { cursor: pointer; }
  .gnode-tree    { fill: var(--accent-trees); stroke: var(--bg); stroke-width: 2; }
  .gnode-spec    { fill: var(--steel-300); stroke: var(--bg); stroke-width: 1; }
  .gnode-session { fill: var(--accent-sessions); stroke: var(--bg); stroke-width: 1; }
  .gedge { fill: none; stroke: var(--paper-400); stroke-width: 1; opacity: 0.6; }
  .gedge-asserted { stroke: var(--accent-sessions); stroke-dasharray: 4 3; opacity: 0.55; }
  .glabel {
    font-family: var(--mono);
    font-size: 9px;
    fill: var(--fg-secondary);
    pointer-events: none;
  }
  .glabel-tree { font-size: 11px; font-weight: 600; fill: var(--fg); }
  .gnode.dimmed, .gedge.dimmed, .glabel.dimmed { opacity: 0.12; }
  .gnode.highlight { stroke: var(--ink-500); stroke-width: 2; }
</style>"#;

/// Inline SVG of the nomograph mark: three dashed scales plus the curve.
/// Single-color, currentColor. Source: gitlab.com/nomograph/design/mark/mark.svg.
/// Stroke-width is bumped to 3.5 (vs the canonical 2.5) so the dashed scales
/// stay legible at the small header display size — at 28px display from a
/// 64-unit viewBox the canonical stroke renders sub-pixel and the scales
/// disappear, leaving only the curve.
const NOM_MARK: &str = r#"<span class="nom-mark" aria-hidden="true"><svg viewBox="0 0 64 64" xmlns="http://www.w3.org/2000/svg"><line x1="14" y1="6" x2="14" y2="58" stroke="currentColor" stroke-width="3.5" stroke-linecap="round" stroke-dasharray="6,6"/><line x1="32" y1="6" x2="32" y2="58" stroke="currentColor" stroke-width="3.5" stroke-linecap="round" stroke-dasharray="3,4"/><line x1="50" y1="6" x2="50" y2="58" stroke="currentColor" stroke-width="3.5" stroke-linecap="round" stroke-dasharray="4.5,5"/><path d="M 14 6 C 14 10, 14 22, 22 28 S 50 38, 50 58" fill="none" stroke="currentColor" stroke-width="3.5" stroke-linecap="round"/></svg></span>"#;

/// Client-side behavior for serve:
///   1. Persists `<details>` open state across refreshes to
///      localStorage, keyed by the stable id on every `<details>`.
///   2. Push-based live refresh: subscribes to /events (Server-Sent
///      Events). Server pushes a `change` event when claims/changes/
///      sees a filesystem event, coalesced across bursts. No timed
///      polling. Page only re-fetches when something actually
///      changed.
///   3. Pause/resume (closes/opens the EventSource) + manual refresh.
const SCRIPT: &str = r#"<script>
(function () {
  const KEY = 'synthesist-serve:open-details';
  const LIVE_KEY = 'synthesist-serve:live';

  function loadOpen() {
    try { return new Set(JSON.parse(localStorage.getItem(KEY) || '[]')); }
    catch (_) { return new Set(); }
  }
  function saveOpen(set) {
    try { localStorage.setItem(KEY, JSON.stringify(Array.from(set))); }
    catch (_) { /* localStorage disabled; soldier on */ }
  }
  function applyOpen(root) {
    const open = loadOpen();
    root.querySelectorAll('details[id]').forEach(d => {
      if (open.has('OPEN:' + d.id)) d.open = true;
      if (open.has('CLOSED:' + d.id)) d.open = false;
    });
  }
  function attachToggle(root) {
    root.querySelectorAll('details[id]').forEach(d => {
      d.addEventListener('toggle', () => {
        const cur = loadOpen();
        cur.delete('OPEN:' + d.id);
        cur.delete('CLOSED:' + d.id);
        cur.add((d.open ? 'OPEN:' : 'CLOSED:') + d.id);
        saveOpen(cur);
      });
    });
  }

  let liveOn = localStorage.getItem(LIVE_KEY) !== '0';
  let es = null;
  let inflight = false;

  async function refresh() {
    if (inflight) return;
    inflight = true;
    try {
      // Preserve query string (?view=...) across SSE-triggered refreshes.
      const url = window.location.pathname + window.location.search;
      const res = await fetch(url, { cache: 'no-store' });
      if (!res.ok) return;
      const html = await res.text();
      const next = new DOMParser().parseFromString(html, 'text/html');
      document.body.innerHTML = next.body.innerHTML;
      applyOpen(document);
      attachToggle(document);
      wireChrome();
      // If we're on the network view, the body swap nuked the SVG.
      // Re-run network init.
      if (document.getElementById('graph-canvas')) initNetwork();
      flashStatus('updated');
    } catch (_) { /* network blip; ignore */ }
    finally { inflight = false; }
  }
  function flashStatus(text) {
    const status = document.getElementById('live-status');
    if (!status) return;
    const prev = status.textContent;
    status.textContent = '· ' + text;
    setTimeout(() => { if (status) status.textContent = prev; }, 700);
  }
  function openStream() {
    if (es) { es.close(); es = null; }
    es = new EventSource('/events');
    es.addEventListener('open', () => updateStatus());
    es.addEventListener('change', () => refresh());
    es.addEventListener('error', () => updateStatus('reconnecting'));
  }
  function closeStream() {
    if (es) { es.close(); es = null; }
  }
  function setLive(on) {
    liveOn = on;
    localStorage.setItem(LIVE_KEY, on ? '1' : '0');
    if (on) openStream(); else closeStream();
    updateStatus(on ? 'live' : 'paused');
  }
  function updateStatus(override) {
    const status = document.getElementById('live-status');
    const toggle = document.getElementById('live-toggle');
    const text = override !== undefined ? override : (liveOn ? 'live' : 'paused');
    if (status) {
      status.textContent = '· ' + text;
      status.className = (text === 'live' || text === 'updated') ? 'live-on' : 'live-off';
    }
    if (toggle) toggle.textContent = liveOn ? 'pause' : 'resume';
  }
  function wireChrome() {
    const toggle = document.getElementById('live-toggle');
    if (toggle) toggle.onclick = () => setLive(!liveOn);
    const now = document.getElementById('refresh-now');
    if (now) now.onclick = () => refresh();
    updateStatus();
  }

  applyOpen(document);
  attachToggle(document);
  wireChrome();
  if (liveOn) openStream();
})();

// Network view setup, exposed so refresh() can re-run it after body swap.
let __d3forceModule = null;
async function initNetwork() {
  const canvas = document.getElementById('graph-canvas');
  if (!canvas) return;
  const status = document.getElementById('graph-status');
  const detail = document.getElementById('graph-detail');
  detail.classList.add('empty');
  detail.textContent = 'click a node to inspect';

  let d3 = __d3forceModule;
  if (!d3) {
    try {
      d3 = await import('https://cdn.jsdelivr.net/npm/d3-force@3/+esm');
      __d3forceModule = d3;
    } catch (e) {
      status.textContent = 'failed to load d3-force from CDN: ' + e.message;
      return;
    }
  }

  let res;
  try { res = await fetch('/api/graph'); }
  catch (e) { status.textContent = 'graph fetch failed: ' + e.message; return; }
  if (!res.ok) { status.textContent = 'graph fetch HTTP ' + res.status; return; }
  const graph = await res.json();
  status.textContent = graph.nodes.length + ' nodes · ' + graph.edges.length + ' edges';

  const W = 800, H = 600;
  const NS = 'http://www.w3.org/2000/svg';
  while (canvas.firstChild) canvas.removeChild(canvas.firstChild);

  // Node radius by kind.
  function radius(n) {
    if (n.kind === 'tree') return 14;
    if (n.kind === 'session') return 6;
    return 5;
  }

  // Build mutable copies for d3-force.
  const nodes = graph.nodes.map(n => ({ ...n }));
  const edges = graph.edges.map(e => ({ ...e }));

  const sim = d3.forceSimulation(nodes)
    .force('link', d3.forceLink(edges).id(n => n.id).distance(e => e.kind === 'contains' ? 50 : 80).strength(0.4))
    .force('charge', d3.forceManyBody().strength(-180))
    .force('center', d3.forceCenter(W/2, H/2))
    .force('collide', d3.forceCollide().radius(n => radius(n) + 4))
    .stop();

  // Run synchronously for a fixed number of ticks so layout is stable
  // before paint. Avoids the wiggle that d3 tickwise rendering shows.
  for (let i = 0; i < 400; i++) sim.tick();

  // Compute bounds and re-fit viewBox so the layout fills the canvas.
  let xmin = Infinity, xmax = -Infinity, ymin = Infinity, ymax = -Infinity;
  nodes.forEach(n => {
    xmin = Math.min(xmin, n.x); xmax = Math.max(xmax, n.x);
    ymin = Math.min(ymin, n.y); ymax = Math.max(ymax, n.y);
  });
  const pad = 40;
  const vbW = Math.max(200, (xmax - xmin) + pad * 2);
  const vbH = Math.max(200, (ymax - ymin) + pad * 2);
  canvas.setAttribute('viewBox', `${xmin - pad} ${ymin - pad} ${vbW} ${vbH}`);

  // Build SVG content. Edges first so nodes paint over them.
  const edgeLayer = document.createElementNS(NS, 'g');
  const nodeLayer = document.createElementNS(NS, 'g');
  const labelLayer = document.createElementNS(NS, 'g');
  canvas.appendChild(edgeLayer);
  canvas.appendChild(nodeLayer);
  canvas.appendChild(labelLayer);

  const edgeEls = edges.map(e => {
    const line = document.createElementNS(NS, 'line');
    line.classList.add('gedge');
    if (e.kind === 'asserted') line.classList.add('gedge-asserted');
    line.setAttribute('x1', e.source.x); line.setAttribute('y1', e.source.y);
    line.setAttribute('x2', e.target.x); line.setAttribute('y2', e.target.y);
    line.dataset.source = e.source.id; line.dataset.target = e.target.id;
    edgeLayer.appendChild(line);
    return line;
  });

  const nodeEls = nodes.map(n => {
    const circle = document.createElementNS(NS, 'circle');
    circle.classList.add('gnode', 'gnode-' + n.kind);
    circle.setAttribute('r', radius(n));
    circle.setAttribute('cx', n.x);
    circle.setAttribute('cy', n.y);
    circle.dataset.id = n.id;
    circle.addEventListener('mouseenter', () => focus(n.id));
    circle.addEventListener('mouseleave', () => unfocus());
    circle.addEventListener('click', () => select(n));
    nodeLayer.appendChild(circle);
    if (n.kind === 'tree' || n.kind === 'session') {
      const label = document.createElementNS(NS, 'text');
      label.classList.add('glabel');
      if (n.kind === 'tree') label.classList.add('glabel-tree');
      label.setAttribute('x', n.x + radius(n) + 3);
      label.setAttribute('y', n.y + 3);
      label.textContent = n.label;
      label.dataset.id = n.id;
      labelLayer.appendChild(label);
    }
    return circle;
  });

  function neighbors(id) {
    const set = new Set([id]);
    edges.forEach(e => {
      if (e.source.id === id) set.add(e.target.id);
      if (e.target.id === id) set.add(e.source.id);
    });
    return set;
  }
  function focus(id) {
    const ns = neighbors(id);
    nodeEls.forEach(c => c.classList.toggle('dimmed', !ns.has(c.dataset.id)));
    edgeEls.forEach(l => l.classList.toggle('dimmed', !(l.dataset.source === id || l.dataset.target === id)));
    labelLayer.querySelectorAll('text').forEach(t => t.classList.toggle('dimmed', !ns.has(t.dataset.id)));
  }
  function unfocus() {
    nodeEls.forEach(c => c.classList.remove('dimmed'));
    edgeEls.forEach(l => l.classList.remove('dimmed'));
    labelLayer.querySelectorAll('text').forEach(t => t.classList.remove('dimmed'));
  }
  function select(n) {
    detail.classList.remove('empty');
    const lines = [
      n.kind + ': ' + n.label,
      // Skip the "tree:" line when the node IS a tree (would just
      // restate the kind line).
      (n.tree && n.kind !== 'tree') ? 'tree: ' + n.tree : null,
      n.status ? 'status: ' + n.status : null,
    ].filter(Boolean);
    // Show plus a "open in trees view" link.
    const lns = lines.map(l => `<div>${l}</div>`).join('');
    let link = '';
    if (n.kind === 'tree') link = `<a href="/?view=trees#tree:${encodeURIComponent(n.label)}">open in trees</a>`;
    if (n.kind === 'spec') link = `<a href="/?view=trees#spec:${encodeURIComponent(n.label)}">open in trees</a>`;
    if (n.kind === 'session') link = `<a href="/?view=trees#session:${encodeURIComponent(n.label)}">open in trees</a>`;
    detail.innerHTML = lns + (link ? `<div style="margin-top:0.4em">${link}</div>` : '');
  }
}
// Run on initial load.
initNetwork();
</script>"#;

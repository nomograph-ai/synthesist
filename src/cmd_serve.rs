//! `synthesist serve` — local HTTP browser for the claim graph.
//!
//! Multi-person ergonomics: Josh and other reviewers open the page,
//! see the current estate, drill in via progressive disclosure. The
//! page is server-rendered HTML with `<details>` for collapse. Every
//! request re-queries the claim view, so refreshes show the latest
//! state without persistent server state.
//!
//! Routes:
//!   GET /             — full dashboard (trees, sessions, summary)
//!   GET /api/state    — same data as JSON (agent-readable)
//!
//! Auto-refresh via meta tag (5s). No JS required for the v1 view.

use anyhow::{Context, Result};
use rouille::{Response, router};
use serde_json::{Value, json};

use crate::store::SynthStore;

const DEFAULT_PORT: u16 = 5179;
const REFRESH_SECONDS: u32 = 5;

pub fn run(port: Option<u16>, bind_all: bool) -> Result<()> {
    let port = port.unwrap_or(DEFAULT_PORT);
    let host = if bind_all { "0.0.0.0" } else { "127.0.0.1" };
    let addr = format!("{host}:{port}");

    eprintln!("synthesist serve listening on http://{addr}");
    eprintln!("  GET /          — dashboard");
    eprintln!("  GET /api/state — JSON");
    eprintln!("press ctrl-c to stop");

    rouille::start_server(addr, move |request| {
        router!(request,
            (GET) (/) => { handle_dashboard() },
            (GET) (/api/state) => { handle_state_json() },
            _ => Response::empty_404(),
        )
    });
}

fn handle_dashboard() -> Response {
    match collect_state() {
        Ok(state) => Response::html(render_dashboard(&state)),
        Err(e) => Response::text(format!("error: {e}")).with_status_code(500),
    }
}

fn handle_state_json() -> Response {
    match collect_state() {
        Ok(state) => Response::json(&state),
        Err(e) => Response::text(format!("error: {e}")).with_status_code(500),
    }
}

#[derive(Debug, serde::Serialize)]
struct State {
    data_dir: String,
    version: String,
    generated_at: String,
    trees: Vec<TreeView>,
    sessions: Vec<SessionView>,
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
        .or_else(|| std::env::current_dir().ok().map(|p| p.display().to_string()))
        .unwrap_or_else(|| "(discovered)".to_string());

    let _ = store; // Keep store alive until we read all data above.
    Ok(State {
        data_dir,
        version: env!("CARGO_PKG_VERSION").to_string(),
        generated_at: now_human(),
        trees,
        sessions,
    })
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
    let mut by_spec: std::collections::BTreeMap<String, Value> =
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
    let mut by_task: std::collections::BTreeMap<String, Value> =
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
            gate: props
                .get("gate")
                .and_then(|v| v.as_str())
                .map(String::from),
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
                date: props
                    .get("date")
                    .and_then(|v| v.as_str())
                    .map(String::from),
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
        let started = row
            .get("asserted_at")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
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
            let asserted_at = row
                .get("asserted_at")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
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
    let s = |k: &str| props.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
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
    let an: Option<u64> = a.trim_start_matches(|c: char| !c.is_ascii_digit()).parse().ok();
    let bn: Option<u64> = b.trim_start_matches(|c: char| !c.is_ascii_digit()).parse().ok();
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

fn render_dashboard(state: &State) -> String {
    let mut s = String::with_capacity(64 * 1024);
    s.push_str("<!doctype html>\n<html lang=\"en\"><head>");
    s.push_str("<meta charset=\"utf-8\">");
    s.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
    s.push_str("<title>synthesist · ");
    s.push_str(&html_escape(&state.data_dir));
    s.push_str("</title>");
    s.push_str(STYLE);
    s.push_str("</head><body>");

    // Header
    s.push_str("<header><h1>synthesist</h1>");
    s.push_str(&format!(
        "<div class=\"meta\"><code>{}</code> · v{} <span id=\"live-status\" class=\"live-on\">· live</span> <button id=\"live-toggle\" class=\"chrome-btn\" type=\"button\">pause</button> <button id=\"refresh-now\" class=\"chrome-btn\" type=\"button\">refresh</button></div>",
        html_escape(&state.data_dir),
        html_escape(&state.version),
    ));
    s.push_str("</header>");

    // Trees
    s.push_str(&format!(
        "<section><details open id=\"section:trees\"><summary><span class=\"section-title\">trees</span> <span class=\"count\">{}</span></summary>",
        state.trees.len()
    ));
    for tree in &state.trees {
        s.push_str(&render_tree(tree));
    }
    s.push_str("</details></section>");

    // Sessions
    let active_n = state.sessions.iter().filter(|s| s.status == "active").count();
    let closed_n = state.sessions.len() - active_n;
    s.push_str(&format!(
        "<section><details open id=\"section:sessions\"><summary><span class=\"section-title\">sessions</span> <span class=\"count\">{} <span class=\"muted\">/ {} active · {} closed</span></span></summary>",
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
    s.push_str(SCRIPT);
    s.push_str("</body></html>");
    s
}

fn render_tree(t: &TreeView) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "<details class=\"tree\" id=\"tree:{}\"><summary><span class=\"name\">{}</span> <span class=\"muted\">· {} specs · {} sessions</span></summary>",
        html_escape(&t.name),
        html_escape(&t.name),
        t.specs.len(),
        t.session_count
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
    s.push_str(&format!(
        "<details class=\"spec\" id=\"spec:{}\"><summary><span class=\"name\">{}</span> <span class=\"status status-{}\">{}</span> <span class=\"muted\">· {} tasks</span></summary>",
        html_escape(&sp.id),
        html_escape(&sp.id),
        html_escape(&sp.status),
        html_escape(&sp.status),
        sp.tasks.len()
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
        (Some(t), Some(s)) => format!(" <span class=\"muted\">· {}/{}</span>", html_escape(t), html_escape(s)),
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

const STYLE: &str = r#"<style>
  :root {
    --fg: #1a1a1a;
    --fg-muted: #6a737d;
    --bg: #ffffff;
    --bg-soft: #f6f8fa;
    --border: #e5e7eb;
    --accent: #7d9aaa;
    --status-active: #1a73e8;
    --status-closed: #6a737d;
    --status-in_progress: #e36209;
    --status-done: #2ea44f;
    --status-blocked: #d73a49;
    --status-deferred: #6a737d;
    --status-pending: #6a737d;
    --status-completed: #2ea44f;
    --status-cancelled: #6a737d;
    --status-superseded: #6a737d;
    --status-abandoned: #d73a49;
    --gate: #d73a49;
    --mono: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
    --sans: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  }
  @media (prefers-color-scheme: dark) {
    :root {
      --fg: #e6edf3;
      --fg-muted: #8b949e;
      --bg: #0d1117;
      --bg-soft: #161b22;
      --border: #30363d;
    }
  }
  * { box-sizing: border-box; }
  body {
    font-family: var(--sans);
    background: var(--bg);
    color: var(--fg);
    margin: 0;
    padding: 1.5rem;
    max-width: 100ch;
    margin-left: auto;
    margin-right: auto;
    font-size: 14px;
    line-height: 1.5;
  }
  header { margin-bottom: 1.5rem; padding-bottom: 0.75rem; border-bottom: 1px solid var(--border); }
  h1 { margin: 0 0 0.25rem 0; font-size: 1.25rem; font-weight: 600; font-family: var(--mono); letter-spacing: -0.01em; }
  .meta { font-family: var(--mono); font-size: 0.8rem; color: var(--fg-muted); }
  .meta code { background: var(--bg-soft); padding: 0.1em 0.35em; border-radius: 3px; }
  section { margin: 1rem 0; }
  details { margin: 0.15rem 0; }
  details > summary {
    cursor: pointer;
    padding: 0.25rem 0;
    list-style: none;
    user-select: none;
  }
  details > summary::-webkit-details-marker { display: none; }
  details > summary::before {
    content: "▸ ";
    color: var(--fg-muted);
    display: inline-block;
    width: 1em;
    transition: transform 0.1s;
  }
  details[open] > summary::before { content: "▾ "; }
  .section-title { font-family: var(--mono); font-weight: 600; font-size: 0.95rem; }
  .section-sub { font-family: var(--mono); font-size: 0.85rem; color: var(--fg-muted); }
  .count { font-family: var(--mono); font-size: 0.8rem; color: var(--fg-muted); }
  .muted { color: var(--fg-muted); font-family: var(--mono); font-size: 0.8rem; }
  .name { font-family: var(--mono); font-weight: 500; }
  .desc { color: var(--fg-muted); font-size: 0.875rem; margin: 0.25rem 0 0.5rem 1.5em; }
  .indent { padding-left: 1.5em; border-left: 1px solid var(--border); margin-left: 0.5em; }
  .tree summary { font-size: 0.95rem; }
  .spec summary { font-size: 0.9rem; }
  .session summary { font-size: 0.9rem; }
  .status {
    font-family: var(--mono);
    font-size: 0.7rem;
    padding: 0.1em 0.45em;
    border-radius: 3px;
    text-transform: lowercase;
    font-weight: 500;
    color: var(--bg);
    margin: 0 0.15em;
  }
  .status-active { background: var(--status-active); }
  .status-closed { background: var(--status-closed); }
  .status-in_progress { background: var(--status-in_progress); }
  .status-done { background: var(--status-done); }
  .status-completed { background: var(--status-completed); }
  .status-pending { background: var(--status-pending); color: var(--fg); border: 1px solid var(--border); }
  .status-blocked { background: var(--status-blocked); }
  .status-deferred { background: var(--status-deferred); }
  .status-cancelled { background: var(--status-cancelled); }
  .status-superseded { background: var(--status-superseded); }
  .status-abandoned { background: var(--status-abandoned); }
  .gate { color: var(--gate); font-family: var(--mono); font-size: 0.75rem; }
  .task { padding: 0.15rem 0; padding-left: 1.5em; font-family: var(--sans); font-size: 0.875rem; }
  .task-id { font-family: var(--mono); color: var(--fg-muted); margin-right: 0.25em; }
  .task-summary { color: var(--fg); }
  .deps, .owner { font-size: 0.75rem; }
  .discovery { padding: 0.4rem 0; padding-left: 1.5em; }
  .discovery-id { font-family: var(--mono); color: var(--fg-muted); font-size: 0.75rem; }
  .discovery-body { margin: 0.25rem 0 0; font-size: 0.875rem; }
  .impact { font-family: var(--mono); font-size: 0.7rem; padding: 0.1em 0.4em; border-radius: 3px; background: var(--bg-soft); color: var(--fg-muted); }
  .claim { padding: 0.15rem 0; padding-left: 1.5em; font-size: 0.8rem; }
  .claim-type { font-family: var(--mono); color: var(--fg-muted); display: inline-block; min-width: 6ch; }
  .claim-summary { font-family: var(--sans); color: var(--fg); }
  footer { margin-top: 2rem; padding-top: 0.75rem; border-top: 1px solid var(--border); font-family: var(--mono); font-size: 0.75rem; color: var(--fg-muted); }
  footer a { color: var(--accent); text-decoration: none; }
  footer a:hover { text-decoration: underline; }
  .chrome-btn {
    font-family: var(--mono);
    font-size: 0.75rem;
    padding: 0.15em 0.55em;
    margin-left: 0.4em;
    border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--bg-soft);
    color: var(--fg);
    cursor: pointer;
  }
  .chrome-btn:hover { background: var(--border); }
  .live-on { color: var(--status-done); }
  .live-off { color: var(--fg-muted); }
</style>"#;

/// Client-side behavior for serve:
///   1. Persists `<details>` open state across reloads to
///      localStorage, keyed by the stable id we put on every
///      `<details>`. Without this any refresh nukes every expansion.
///   2. Smooth live refresh: every 5s, fetch `/`, swap the body
///      innerHTML using DOMParser. No navigation, no flicker. State
///      restoration runs after each swap.
///   3. Pause/resume + manual refresh buttons in the header.
const SCRIPT: &str = r#"<script>
(function () {
  const KEY = 'synthesist-serve:open-details';
  const LIVE_KEY = 'synthesist-serve:live';
  const INTERVAL_MS = 5000;

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
  let timer = null;

  async function refresh() {
    try {
      const res = await fetch(window.location.pathname, { cache: 'no-store' });
      if (!res.ok) return;
      const html = await res.text();
      const next = new DOMParser().parseFromString(html, 'text/html');
      // Swap only the body; keep the <head> stable.
      document.body.innerHTML = next.body.innerHTML;
      applyOpen(document);
      attachToggle(document);
      wireChrome();
    } catch (_) { /* network blip; skip this tick */ }
  }
  function setLive(on) {
    liveOn = on;
    localStorage.setItem(LIVE_KEY, on ? '1' : '0');
    if (timer) clearInterval(timer);
    timer = on ? setInterval(refresh, INTERVAL_MS) : null;
    const status = document.getElementById('live-status');
    const toggle = document.getElementById('live-toggle');
    if (status) {
      status.textContent = on ? '· live' : '· paused';
      status.className = on ? 'live-on' : 'live-off';
    }
    if (toggle) toggle.textContent = on ? 'pause' : 'resume';
  }
  function wireChrome() {
    const toggle = document.getElementById('live-toggle');
    if (toggle) toggle.onclick = () => setLive(!liveOn);
    const now = document.getElementById('refresh-now');
    if (now) now.onclick = () => refresh();
    // Reflect current state into freshly-swapped chrome.
    const status = document.getElementById('live-status');
    if (status) {
      status.textContent = liveOn ? '· live' : '· paused';
      status.className = liveOn ? 'live-on' : 'live-off';
    }
    if (toggle) toggle.textContent = liveOn ? 'pause' : 'resume';
  }

  applyOpen(document);
  attachToggle(document);
  wireChrome();
  if (liveOn) timer = setInterval(refresh, INTERVAL_MS);
})();
</script>"#;

#[allow(dead_code)]
fn _quiet_unused_warnings() -> Value {
    json!({})
}

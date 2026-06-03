//! Session commands (Path B Stage 1: v3-native).
//!
//! `session start` writes one v3 Session claim with session-scoped
//! asserter. `session close` writes a superseding Session claim.
//! Reads (`list`, `status`) walk the redb gamma index.

use crate::claim_type::ClaimType;
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::cli::SessionCmd;
use crate::store::{SynthStore, bare_props, json_out, short_claim_id};
use crate::wire_format as wf;

pub fn run(cmd: &SessionCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        SessionCmd::Start {
            id,
            tree,
            spec,
            summary,
        } => cmd_session_start(id, tree.as_deref(), spec.as_deref(), summary.as_deref()),
        SessionCmd::List => cmd_session_list(),
        SessionCmd::Status { id } => cmd_session_status(id),
        SessionCmd::Merge { .. } => {
            bail!("session merge removed in v2; merges are automatic (git pull; CRDT merge).")
        }
        SessionCmd::Discard { .. } => {
            bail!("session discard removed in v2; use `synthesist session close <id>` instead.")
        }
        SessionCmd::Close { id, start_id } => cmd_session_close(id, start_id.as_deref(), session),
    }
}

fn asserter_base() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "unknown".into());
    format!("user:local:{user}")
}

fn cmd_session_start(
    id: &str,
    tree: Option<&str>,
    spec: Option<&str>,
    summary: Option<&str>,
) -> Result<()> {
    if id.is_empty() {
        bail!("session id must be non-empty");
    }
    let base = asserter_base();
    let session_asserter = format!("{}:{}", base, id);
    let mut props = serde_json::Map::new();
    props.insert("id".to_string(), Value::String(id.to_string()));
    if let Some(t) = tree {
        props.insert("tree".to_string(), Value::String(t.to_string()));
    }
    if let Some(s) = spec {
        props.insert("spec".to_string(), Value::String(s.to_string()));
    }
    if let Some(s) = summary {
        props.insert("summary".to_string(), Value::String(s.to_string()));
    }

    let mut store = SynthStore::discover()?.with_asserter(session_asserter.clone());
    store
        .append(ClaimType::Session, Value::Object(props), None)
        .map_err(|e| anyhow!("session start failed: {e}"))?;

    json_out(&json!({
        "id": id,
        "asserter": session_asserter,
        "started_at": Value::Null,
    }))
}

fn cmd_session_list() -> Result<()> {
    let store = SynthStore::discover()?;
    // Live Session openers (H4 dual anti-join: not superseded AND not
    // itself a superseder, separating openers from closers that share an
    // id).
    let mut out: Vec<Value> = Vec::new();
    for opener in store.live_session_openers()? {
        let Some(doc) = store.doc(&opener)? else {
            continue;
        };
        let props = bare_props(&doc);
        let id = match props.get("id").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let opt = |k: &str| -> Option<String> {
            props
                .get(k)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        };
        out.push(json!({
            "id": id,
            "tree": opt("tree"),
            "spec": opt("spec"),
            "summary": opt("summary"),
            "asserter_base": format!("{}:{}", asserter_base(), id),
            "start_id": short_claim_id(&opener),
        }));
    }
    out.sort_by(|a, b| {
        a.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("id").and_then(|v| v.as_str()).unwrap_or(""))
    });
    json_out(&json!({ "sessions": out }))
}

fn cmd_session_status(id: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    // The live opener carrying this display id (H4). When live, the
    // opener is both not-superseded and not-itself-a-superseder. When
    // the session is closed, no live opener exists, so we fall back to a
    // scan over all Session heads to recover the opener's props for the
    // closed-status report.
    let live = store.session_is_live(id)?;
    let opener_doc = if let Some(opener) = store.session_opener_by_id(id)? {
        store.doc(&opener)?
    } else {
        // Closed (or never-opened): find any Session claim carrying this
        // id that does not itself supersede a prior (the original opener).
        find_opener_doc(&store, id)?
    };
    let doc = opener_doc.ok_or_else(|| {
        anyhow!(
            "session '{id}' not found. \
             Run `synthesist session list` to see known sessions, \
             or `synthesist session start <id>` to open a new one."
        )
    })?;
    let props = bare_props(&doc);
    let str_prop = |k: &str| -> Option<String> {
        props
            .get(k)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };
    let tree = str_prop("tree");
    let spec = str_prop("spec");
    let summary = str_prop("summary");
    let started_at = doc
        .get(wf::GENERATED_AT_PRED)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let status = if live { "active" } else { "closed" };

    let mut props = serde_json::Map::new();
    props.insert("id".into(), Value::String(id.to_string()));
    if let Some(t) = tree {
        props.insert("tree".into(), Value::String(t));
    }
    if let Some(s) = spec {
        props.insert("spec".into(), Value::String(s));
    }
    if let Some(s) = summary {
        props.insert("summary".into(), Value::String(s));
    }

    json_out(&json!({
        "id": id,
        "status": status,
        "started_at": started_at.map(Value::String).unwrap_or(Value::Null),
        "props": Value::Object(props),
    }))
}

/// Find the original opener doc for a (possibly closed) session id.
///
/// The opener is the Session claim carrying `synthesist:id == id` that
/// does NOT itself supersede a prior claim. Walks the raw log union
/// because closed sessions have no live head for gamma's H4 to return.
fn find_opener_doc(store: &SynthStore, id: &str) -> Result<Option<Value>> {
    let session_type = wf::type_iri("session");
    let id_pred = wf::predicate_iri("id");
    for doc in store.iter_claims()? {
        if doc.get("@type").and_then(|v| v.as_str()) != Some(session_type.as_str()) {
            continue;
        }
        if doc.get(&id_pred).and_then(|v| v.as_str()) != Some(id) {
            continue;
        }
        // The opener does not carry a supersedes edge.
        if doc.get(wf::SUPERSEDES_PRED).is_some() {
            continue;
        }
        return Ok(Some(doc));
    }
    Ok(None)
}

fn cmd_session_close(id: &str, start_id: Option<&str>, session: &Option<String>) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;

    // Collect all live openers for this display id. The v2 contract
    // tolerates name collisions across sessions; `--start-id` picks the
    // intended target. With one live opener we proceed; with more we
    // require disambiguation (or, when no prefix is supplied, fall back
    // to the most recently asserted opener per `prov:generatedAtTime`).
    //
    // Order DESC by generatedAtTime pushes the freshest opener to the top
    // so the implicit "single live session" path keeps the v2 behaviour.
    struct Candidate {
        iri: String,
        ts: String,
        tree: Option<String>,
        spec: Option<String>,
        summary: Option<String>,
    }

    let mut candidates: Vec<Candidate> = Vec::new();
    for opener in store.live_session_openers()? {
        let Some(doc) = store.doc(&opener)? else {
            continue;
        };
        let props = bare_props(&doc);
        if props.get("id").and_then(|v| v.as_str()) != Some(id) {
            continue;
        }
        let opt = |k: &str| -> Option<String> {
            props
                .get(k)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        };
        let ts = doc
            .get(wf::GENERATED_AT_PRED)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        candidates.push(Candidate {
            iri: opener,
            ts,
            tree: opt("tree"),
            spec: opt("spec"),
            summary: opt("summary"),
        });
    }
    // Lexical compare == chronological for the canonical timestamp form.
    candidates.sort_by(|a, b| b.ts.cmp(&a.ts).then(a.iri.cmp(&b.iri)));

    if candidates.is_empty() {
        bail!(
            "session '{id}' not found or already closed. \
             Run `synthesist session list` to see live sessions."
        );
    }

    let chosen = match start_id {
        Some(prefix) if !prefix.is_empty() => {
            let matched: Vec<&Candidate> = candidates
                .iter()
                .filter(|c| short_claim_id(&c.iri).starts_with(prefix))
                .collect();
            match matched.len() {
                0 => {
                    let ids: Vec<String> =
                        candidates.iter().map(|c| short_claim_id(&c.iri)).collect();
                    bail!(
                        "no live session '{id}' matches --start-id '{prefix}' \
                         (candidates: {})",
                        ids.join(", ")
                    );
                }
                1 => matched.into_iter().next().unwrap(),
                _ => {
                    let ids: Vec<String> = matched.iter().map(|c| short_claim_id(&c.iri)).collect();
                    bail!(
                        "--start-id '{prefix}' is ambiguous among {} live sessions named '{id}' \
                         (candidates: {}); supply a longer prefix",
                        ids.len(),
                        ids.join(", ")
                    );
                }
            }
        }
        _ => {
            // No prefix supplied. With multiple live openers we take the
            // most recently asserted one (candidates are already ordered
            // newest-first by generation time when read off the gamma
            // index); that keeps the single-session happy path stable
            // while still terminating cleanly on name collisions without
            // forcing the caller to pick.
            candidates.first().unwrap()
        }
    };

    let prior_id = short_claim_id(&chosen.iri);
    let mut props = serde_json::Map::new();
    props.insert("id".into(), Value::String(id.to_string()));
    if let Some(t) = chosen.tree.clone() {
        props.insert("tree".into(), Value::String(t));
    }
    if let Some(s) = chosen.spec.clone() {
        props.insert("spec".into(), Value::String(s));
    }
    if let Some(s) = chosen.summary.clone() {
        props.insert("summary".into(), Value::String(s));
    }

    store.append(
        ClaimType::Session,
        Value::Object(props),
        Some(prior_id.clone()),
    )?;
    json_out(&json!({ "closed": true, "id": id, "start_id": prior_id }))
}

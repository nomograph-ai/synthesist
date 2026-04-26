//! Session commands — ported to the claim substrate (v2).
//!
//! v1 used file-copied SQLite databases with a three-way EXCEPT merge.
//! v2 represents a session as a tagged span of writes: one `Session`
//! claim opens the span, a superseding `Session` claim closes it. The
//! CRDT handles merges automatically, so `session merge` and
//! `session discard` are gone.
//!
//! Supported: `start`, `close`, `list`, `status`. The CLI still carries
//! `Merge` and `Discard` variants for backwards compatibility; both
//! bail with a prescriptive message telling the caller what to do
//! instead.

use anyhow::{Result, anyhow, bail};
use nomograph_claim::{ClaimType, Session};
use serde_json::{Value, json};

use crate::cli::SessionCmd;
use crate::store::{SynthStore, json_out};

pub fn run(cmd: &SessionCmd) -> Result<()> {
    match cmd {
        SessionCmd::Start {
            id,
            tree,
            spec,
            summary,
        } => cmd_session_start(id, tree.as_deref(), spec.as_deref(), summary.as_deref()),
        SessionCmd::List => cmd_session_list(),
        SessionCmd::Status { id } => cmd_session_status(id),
        SessionCmd::Merge { .. } => bail!(
            "session merge removed in v2; merges are automatic (git pull; CRDT merge). \
             Run `synthesist conflicts` to list diamond conflicts; resolve by \
             appending a claim that supersedes both rivals."
        ),
        SessionCmd::Discard { .. } => bail!(
            "session discard removed in v2; use `synthesist session close <id>` to \
             supersede the opener non-destructively, or just stop referencing the \
             session. Run `synthesist conflicts` if supersessions diverged."
        ),
        SessionCmd::Close { id, start_id } => cmd_session_close(id, start_id.as_deref()),
    }
}

/// Derive the asserter base for a new session. Mirrors the
/// `user:local:<USER>` convention used everywhere else in the
/// synthesist adapter.
fn asserter_base() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "unknown".into());
    format!("user:local:{user}")
}

/// `session start <id>` — append an opening `Session` claim, print
/// `{id, asserter, started_at}`.
fn cmd_session_start(
    id: &str,
    tree: Option<&str>,
    spec: Option<&str>,
    summary: Option<&str>,
) -> Result<()> {
    if id.is_empty() {
        bail!("session id must be non-empty");
    }

    let mut store = SynthStore::discover()?;
    let base = asserter_base();
    let handle = Session::start(store.inner(), id, &base, tree, spec, summary)
        .map_err(|e| anyhow!("session start failed: {e}"))?;

    // Refresh the view so subsequent reads see the new claim.
    store.sync_view()?;

    // Locate the opening claim to surface its asserted_at.
    let rows = store.query(
        "SELECT asserted_at FROM claims \
         WHERE claim_type = 'session' \
           AND json_extract(props, '$.id') = ?1 \
           AND supersedes IS NULL \
         ORDER BY asserted_at DESC LIMIT 1",
        &[&id],
    )?;
    let started_at = rows
        .into_iter()
        .next()
        .and_then(|r| r.get("asserted_at").cloned())
        .unwrap_or(Value::Null);

    json_out(&json!({
        "id": handle.id(),
        "asserter": handle.asserter(),
        "started_at": started_at,
    }))
}

/// `session list` — every live session opener.
fn cmd_session_list() -> Result<()> {
    let mut store = SynthStore::discover()?;
    let sessions =
        Session::list_live(store.inner()).map_err(|e| anyhow!("session list failed: {e}"))?;
    let out: Vec<Value> = sessions
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "tree": s.tree,
                "spec": s.spec,
                "summary": s.summary,
                "asserter_base": s.asserter_base,
                "start_id": s.start_id,
            })
        })
        .collect();
    json_out(&json!({ "sessions": out }))
}

/// `session status <id>` — print the props of the Session claim for
/// `<id>` (opener; closers are Session claims with a `supersedes` set).
fn cmd_session_status(id: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    let rows = store.query(
        "SELECT props, asserted_at, supersedes FROM claims \
         WHERE claim_type = 'session' \
           AND json_extract(props, '$.id') = ?1 \
         ORDER BY asserted_at DESC",
        &[&id],
    )?;

    // Find the most recent opener (claim with no `supersedes`) for this id.
    let opener = rows
        .iter()
        .find(|r| r.get("supersedes").map(|v| v.is_null()).unwrap_or(true));
    let opener = opener.ok_or_else(|| anyhow!("session '{id}' not found"))?;

    let props_str = opener
        .get("props")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("session claim missing props"))?;
    let props: Value = serde_json::from_str(props_str)?;

    // Determine live-vs-closed: a closer is a Session claim with
    // `supersedes` set to the opener's id. Because the opener query
    // already limits to id=?1, presence of any closer in `rows` means
    // closed.
    let closed = rows
        .iter()
        .any(|r| r.get("supersedes").and_then(|v| v.as_str()).is_some());
    let status = if closed { "closed" } else { "active" };

    let started_at = opener.get("asserted_at").cloned().unwrap_or(Value::Null);

    json_out(&json!({
        "id": id,
        "status": status,
        "started_at": started_at,
        "props": props,
    }))
}

/// `session close <id>` — append a superseding `Session` claim marking
/// `status = "closed"`. Non-destructive: prior claims stay in the log.
///
/// When `start_id` is given, select the live opener whose claim hash
/// (the `id` column on the `claims` table) starts with that prefix.
/// This disambiguates when several sessions share the same display
/// `id` — the original v1 single-id assumption that v2 doesn't enforce
/// at write time.
fn cmd_session_close(id: &str, start_id: Option<&str>) -> Result<()> {
    let mut store = SynthStore::discover()?;
    // Pull every live opener (claim_type=session, supersedes IS NULL,
    // and whose claim id is not itself superseded by a later session
    // claim). We re-derive "live" client-side from the rows because the
    // `supersedes IS NULL` filter alone is not enough — that yields all
    // openers, including ones already closed by a later closer.
    let all_rows = store.query(
        "SELECT id, props, supersedes FROM claims \
         WHERE claim_type = 'session' \
         ORDER BY asserted_at",
        &[],
    )?;

    let mut superseded_ids = std::collections::HashSet::new();
    for row in &all_rows {
        if let Some(prior) = row.get("supersedes").and_then(|v| v.as_str()) {
            superseded_ids.insert(prior.to_string());
        }
    }

    // Live openers are rows with `supersedes IS NULL` and `id NOT IN superseded_ids`.
    let mut live_with_matching_display_id: Vec<(String, Value)> = Vec::new();
    for row in &all_rows {
        let supersedes_set = row.get("supersedes").and_then(|v| v.as_str()).is_some();
        if supersedes_set {
            continue;
        }
        let claim_id = row
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("session claim missing id"))?
            .to_string();
        if superseded_ids.contains(&claim_id) {
            continue;
        }
        let props_str = row
            .get("props")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("session claim missing props"))?;
        let props: Value = serde_json::from_str(props_str)?;
        let display_id = props.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if display_id == id {
            live_with_matching_display_id.push((claim_id, props));
        }
    }

    // Pick the target opener.
    let (prior_id, props) = match start_id {
        Some(needle) => {
            let needle = needle.trim();
            if needle.is_empty() {
                bail!("--start-id must be a non-empty hex prefix or full hash");
            }
            let matches: Vec<&(String, Value)> = live_with_matching_display_id
                .iter()
                .filter(|(claim_id, _)| claim_id.starts_with(needle))
                .collect();
            match matches.len() {
                0 => bail!(
                    "no live session with id '{id}' has start_id starting with '{needle}'. \
                     Candidates: [{}]",
                    live_with_matching_display_id
                        .iter()
                        .map(|(c, _)| c.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                1 => matches[0].clone(),
                _ => bail!(
                    "--start-id '{needle}' is ambiguous; matches: [{}]",
                    matches
                        .iter()
                        .map(|(c, _)| c.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        }
        None => {
            // Without --start-id, behavior is unchanged from the
            // original `session close`: pick the most recently asserted
            // live opener for `id`. The `all_rows` query is ordered
            // oldest-first, so the last collected match is newest.
            live_with_matching_display_id
                .last()
                .cloned()
                .ok_or_else(|| anyhow!("session '{id}' not found"))?
        }
    };

    store.append(ClaimType::Session, props, Some(prior_id.clone()))?;
    json_out(&json!({ "closed": true, "id": id, "start_id": prior_id }))
}

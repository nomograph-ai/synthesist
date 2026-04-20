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
             Use `synthesist conflicts` if supersessions diverged."
        ),
        SessionCmd::Discard { .. } => bail!(
            "session discard removed in v2; use `synthesist conflicts resolve --pick` \
             or just stop referencing the session."
        ),
        SessionCmd::Close { id } => cmd_session_close(id),
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

/// Reserved for when the CLI gains a `close` subcommand. Appends a
/// superseding Session claim with the opener's props.
///
/// `session close <id>` — append a superseding `Session` claim marking
/// `status = "closed"`. Non-destructive: prior claims stay in the log.
fn cmd_session_close(id: &str) -> Result<()> {
    let mut store = SynthStore::discover()?;
    let rows = store.query(
        "SELECT id, props FROM claims \
         WHERE claim_type = 'session' \
           AND json_extract(props, '$.id') = ?1 \
           AND supersedes IS NULL \
         ORDER BY asserted_at DESC LIMIT 1",
        &[&id],
    )?;
    let row = rows
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("session '{id}' not found"))?;

    let prior_id = row
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("session claim missing id"))?
        .to_string();
    let props_str = row
        .get("props")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("session claim missing props"))?;
    let props: Value = serde_json::from_str(props_str)?;

    store.append(ClaimType::Session, props, Some(prior_id))?;
    json_out(&json!({ "closed": true, "id": id }))
}

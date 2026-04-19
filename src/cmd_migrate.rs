//! One-shot synthesist v1 SQLite → v2 nomograph-claim migration.
//!
//! Reads a `.synth/main.db` (v1 schema from `synthesist/src/migrations/0001_initial.sql`)
//! and appends one claim per row into a fresh `claims/` directory via
//! `nomograph_claim::Store`. Preserves `created_at` timestamps as
//! `asserted_at` / `valid_from` on the resulting claims.
//!
//! Idempotence marker: a `Discovery` claim with `tree = "__migration__"`
//! and `spec = "v1-to-v2"` is written on completion; subsequent runs
//! detect it and refuse to re-migrate unless `overwrite = true`.

use std::path::Path;

use chrono::{DateTime, TimeZone, Utc};
use nomograph_claim::{Claim, ClaimId, ClaimType, Store};
use rusqlite::{params, Connection, OpenFlags, Row};
use serde_json::{json, Value};
use thiserror::Error;

/// Asserter used for all migrated claims.
pub const MIGRATION_ASSERTER: &str = "user:migration-v1-v2";

/// Marker claim props identifying a completed migration.
fn marker_props() -> Value {
    json!({
        "tree": "__migration__",
        "spec": "v1-to-v2",
        "id": "marker",
        "date": Utc::now().to_rfc3339(),
        "finding": "v1-to-v2 migration complete",
    })
}

/// Summary of a migration run.
#[derive(Debug, Default, Clone)]
pub struct MigrationSummary {
    pub trees: usize,
    pub specs: usize,
    pub tasks: usize,
    pub discoveries: usize,
    pub campaigns: usize,
    pub sessions: usize,
    pub stakeholders: usize,
    pub dispositions: usize,
    pub signals: usize,
    pub phase: usize,
    pub skipped: Vec<String>,
}

impl MigrationSummary {
    pub fn total(&self) -> usize {
        self.trees
            + self.specs
            + self.tasks
            + self.discoveries
            + self.campaigns
            + self.sessions
            + self.stakeholders
            + self.dispositions
            + self.signals
            + self.phase
    }
}

#[derive(Debug, Error)]
pub enum MigrateError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("claim: {0}")]
    Claim(#[from] nomograph_claim::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("already migrated; pass --overwrite to re-migrate")]
    AlreadyMigrated,
    #[error("source db missing at {0}")]
    SourceMissing(String),
}

pub type Result<T> = std::result::Result<T, MigrateError>;

/// Run a migration. `from` is the v1 `.synth/main.db` path; `to` is a
/// fresh `claims/` directory (will be init'd). Dry-run reads but does
/// not write.
pub fn migrate(from: &Path, to: &Path, dry_run: bool, overwrite: bool) -> Result<MigrationSummary> {
    if !from.exists() {
        return Err(MigrateError::SourceMissing(from.display().to_string()));
    }
    let conn = Connection::open_with_flags(from, OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    if !dry_run {
        if to.exists() && is_already_migrated(to)? {
            if !overwrite {
                return Err(MigrateError::AlreadyMigrated);
            }
            std::fs::remove_dir_all(to)?;
        }
    }

    let mut store = if dry_run {
        // Throwaway store in tmpdir so append/validate paths still exercise.
        let tmp = std::env::temp_dir().join(format!("synth-migrate-dry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        Some(Store::init(&tmp)?)
    } else {
        Some(Store::init(to)?)
    };
    let store_mut = store.as_mut().expect("store initialized");

    let mut summary = MigrationSummary::default();

    migrate_trees(&conn, store_mut, &mut summary)?;
    migrate_specs(&conn, store_mut, &mut summary)?;
    migrate_tasks(&conn, store_mut, &mut summary)?;
    migrate_discoveries(&conn, store_mut, &mut summary)?;
    migrate_campaigns(&conn, store_mut, &mut summary)?;
    migrate_sessions(&conn, store_mut, &mut summary)?;
    migrate_stakeholders(&conn, store_mut, &mut summary)?;
    migrate_dispositions(&conn, store_mut, &mut summary)?;
    migrate_signals(&conn, store_mut, &mut summary)?;
    migrate_phase(&conn, store_mut, &mut summary)?;

    // Write idempotence marker
    let marker = build_claim(ClaimType::Discovery, marker_props(), Utc::now(), None);
    store_mut.append(&marker)?;

    Ok(summary)
}

fn is_already_migrated(claims_dir: &Path) -> Result<bool> {
    if !claims_dir.join("genesis.amc").exists() {
        return Ok(false);
    }
    let mut store = Store::open(claims_dir)?;
    let claims = store.load_claims()?;
    Ok(claims.iter().any(|c| {
        c.claim_type == ClaimType::Discovery
            && c.props.get("tree").and_then(|v| v.as_str()) == Some("__migration__")
    }))
}

fn build_claim(
    claim_type: ClaimType,
    props: Value,
    at: DateTime<Utc>,
    supersedes: Option<ClaimId>,
) -> Claim {
    let id = Claim::compute_id(&claim_type, &props, at, MIGRATION_ASSERTER, at);
    Claim {
        id,
        claim_type,
        props,
        valid_from: at,
        valid_until: None,
        supersedes,
        parent_asserter: None,
        asserted_by: MIGRATION_ASSERTER.to_string(),
        asserted_at: at,
    }
}

fn parse_ts(row: &Row, idx: &str) -> DateTime<Utc> {
    row.get::<_, Option<String>>(idx)
        .ok()
        .flatten()
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap())
}

fn opt_str(row: &Row, idx: &str) -> Option<String> {
    row.get::<_, Option<String>>(idx).ok().flatten()
}

fn migrate_trees(conn: &Connection, store: &mut Store, s: &mut MigrationSummary) -> Result<()> {
    let mut stmt = conn.prepare("SELECT name, status, description FROM trees")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>("name")?,
            row.get::<_, Option<String>>("status")?.unwrap_or_default(),
            row.get::<_, Option<String>>("description")?.unwrap_or_default(),
        ))
    })?;
    for r in rows {
        let (name, _status, description) = r?;
        let props = json!({ "name": name, "description": description });
        let claim = build_claim(ClaimType::Tree, props, Utc::now(), None);
        match store.append(&claim) {
            Ok(()) => s.trees += 1,
            Err(e) => s.skipped.push(format!("tree: {e}")),
        }
    }
    Ok(())
}

fn migrate_specs(conn: &Connection, store: &mut Store, s: &mut MigrationSummary) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT tree, id, goal, constraints, decisions, status, outcome, created FROM specs",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>("tree")?,
            row.get::<_, String>("id")?,
            opt_str(row, "goal").unwrap_or_default(),
            opt_str(row, "constraints").unwrap_or_default(),
            opt_str(row, "decisions").unwrap_or_default(),
            opt_str(row, "status").unwrap_or_else(|| "active".into()),
            opt_str(row, "outcome").unwrap_or_default(),
            parse_ts(row, "created"),
        ))
    })?;
    for r in rows {
        let (tree, id, goal, constraints, decisions, status, outcome, created) = r?;
        // v2 requires topics + valid status enum. Default topics to []; valid_status map.
        let normalized_status = match status.as_str() {
            "active" | "done" | "superseded" | "draft" => status,
            _ => "active".to_string(),
        };
        let props = json!({
            "tree": tree,
            "id": id,
            "goal": if goal.is_empty() { "(migrated; no goal recorded)".to_string() } else { goal },
            "constraints": constraints,
            "decisions": decisions,
            "status": normalized_status,
            "outcome": outcome,
            "topics": ["__migrated__"],
            "agree_snapshot": [],
        });
        let claim = build_claim(ClaimType::Spec, props, created, None);
        match store.append(&claim) {
            Ok(()) => s.specs += 1,
            Err(e) => s.skipped.push(format!("spec: {e}")),
        }
    }
    Ok(())
}

fn migrate_tasks(conn: &Connection, store: &mut Store, s: &mut MigrationSummary) -> Result<()> {
    // Gather deps and files per task first, then stitch into Task claim.
    let deps: std::collections::HashMap<(String, String, String), Vec<String>> = {
        let mut stmt =
            conn.prepare("SELECT tree, spec, task_id, depends_on FROM task_deps")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>("tree")?,
                row.get::<_, String>("spec")?,
                row.get::<_, String>("task_id")?,
                row.get::<_, String>("depends_on")?,
            ))
        })?;
        let mut m: std::collections::HashMap<(String, String, String), Vec<String>> =
            Default::default();
        for r in rows {
            let (t, sp, tid, d) = r?;
            m.entry((t, sp, tid)).or_default().push(d);
        }
        m
    };
    let files: std::collections::HashMap<(String, String, String), Vec<String>> = {
        let mut stmt =
            conn.prepare("SELECT tree, spec, task_id, path FROM task_files")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>("tree")?,
                row.get::<_, String>("spec")?,
                row.get::<_, String>("task_id")?,
                row.get::<_, String>("path")?,
            ))
        })?;
        let mut m: std::collections::HashMap<(String, String, String), Vec<String>> =
            Default::default();
        for r in rows {
            let (t, sp, tid, p) = r?;
            m.entry((t, sp, tid)).or_default().push(p);
        }
        m
    };
    let acceptance: std::collections::HashMap<
        (String, String, String),
        Vec<(i64, String, String)>,
    > = {
        let mut stmt = conn
            .prepare("SELECT tree, spec, task_id, seq, criterion, verify_cmd FROM acceptance")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>("tree")?,
                row.get::<_, String>("spec")?,
                row.get::<_, String>("task_id")?,
                row.get::<_, i64>("seq")?,
                row.get::<_, String>("criterion")?,
                row.get::<_, String>("verify_cmd")?,
            ))
        })?;
        let mut m: std::collections::HashMap<
            (String, String, String),
            Vec<(i64, String, String)>,
        > = Default::default();
        for r in rows {
            let (t, sp, tid, seq, cr, vc) = r?;
            m.entry((t, sp, tid)).or_default().push((seq, cr, vc));
        }
        for v in m.values_mut() {
            v.sort_by_key(|x| x.0);
        }
        m
    };

    let mut stmt = conn.prepare(
        "SELECT tree, spec, id, summary, description, status, gate, owner, created FROM tasks",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>("tree")?,
            row.get::<_, String>("spec")?,
            row.get::<_, String>("id")?,
            row.get::<_, String>("summary")?,
            opt_str(row, "description").unwrap_or_default(),
            opt_str(row, "status").unwrap_or_else(|| "pending".into()),
            opt_str(row, "gate"),
            opt_str(row, "owner"),
            parse_ts(row, "created"),
        ))
    })?;
    for r in rows {
        let (tree, spec, id, summary, description, status, gate, owner, created) = r?;
        let key = (tree.clone(), spec.clone(), id.clone());
        let status_normalized = match status.as_str() {
            "pending" | "in_progress" | "done" | "blocked" | "waiting" | "cancelled" => status,
            _ => "pending".into(),
        };
        let gate_normalized = match gate.as_deref() {
            Some("human") => Some("human".to_string()),
            _ => None,
        };
        let props = json!({
            "tree": tree,
            "spec": spec,
            "id": id,
            "summary": summary,
            "description": description,
            "status": status_normalized,
            "gate": gate_normalized,
            "owner": owner,
            "depends_on": deps.get(&key).cloned().unwrap_or_default(),
            "files": files.get(&key).cloned().unwrap_or_default(),
            "acceptance": acceptance
                .get(&key)
                .map(|v| {
                    v.iter()
                        .map(|(_, c, vc)| json!({ "criterion": c, "verify_cmd": vc }))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        });
        let claim = build_claim(ClaimType::Task, props, created, None);
        match store.append(&claim) {
            Ok(()) => s.tasks += 1,
            Err(e) => s.skipped.push(format!("task: {e}")),
        }
    }
    Ok(())
}

fn migrate_discoveries(conn: &Connection, store: &mut Store, s: &mut MigrationSummary) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT tree, spec, id, date, author, finding, impact, action FROM discoveries",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>("tree")?,
            row.get::<_, String>("spec")?,
            row.get::<_, String>("id")?,
            row.get::<_, String>("date")?,
            opt_str(row, "author").unwrap_or_default(),
            row.get::<_, String>("finding")?,
            opt_str(row, "impact").unwrap_or_default(),
            opt_str(row, "action").unwrap_or_default(),
        ))
    })?;
    for r in rows {
        let (tree, spec, id, date, author, finding, impact, action) = r?;
        let ts = DateTime::parse_from_rfc3339(&date)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let props = json!({
            "tree": tree,
            "spec": spec,
            "id": id,
            "date": date,
            "author": author,
            "finding": finding,
            "impact": impact,
            "action": action,
        });
        let claim = build_claim(ClaimType::Discovery, props, ts, None);
        match store.append(&claim) {
            Ok(()) => s.discoveries += 1,
            Err(e) => s.skipped.push(format!("discovery: {e}")),
        }
    }
    Ok(())
}

fn migrate_campaigns(conn: &Connection, store: &mut Store, s: &mut MigrationSummary) -> Result<()> {
    let blocked: std::collections::HashMap<(String, String), Vec<String>> = {
        let mut stmt =
            conn.prepare("SELECT tree, spec_id, blocked_by FROM campaign_blocked_by")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>("tree")?,
                row.get::<_, String>("spec_id")?,
                row.get::<_, String>("blocked_by")?,
            ))
        })?;
        let mut m: std::collections::HashMap<(String, String), Vec<String>> = Default::default();
        for r in rows {
            let (t, sp, b) = r?;
            m.entry((t, sp)).or_default().push(b);
        }
        m
    };

    let mut active_stmt =
        conn.prepare("SELECT tree, spec_id, summary, phase FROM campaign_active")?;
    let rows = active_stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>("tree")?,
            row.get::<_, String>("spec_id")?,
            opt_str(row, "summary").unwrap_or_default(),
            opt_str(row, "phase").unwrap_or_default(),
        ))
    })?;
    for r in rows {
        let (tree, spec, summary, _phase) = r?;
        let key = (tree.clone(), spec.clone());
        let props = json!({
            "tree": tree,
            "spec": spec,
            "kind": "active",
            "summary": summary,
            "blocked_by": blocked.get(&key).cloned().unwrap_or_default(),
        });
        let claim = build_claim(ClaimType::Campaign, props, Utc::now(), None);
        match store.append(&claim) {
            Ok(()) => s.campaigns += 1,
            Err(e) => s.skipped.push(format!("campaign_active: {e}")),
        }
    }

    let mut backlog_stmt =
        conn.prepare("SELECT tree, spec_id, title, summary FROM campaign_backlog")?;
    let rows = backlog_stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>("tree")?,
            row.get::<_, String>("spec_id")?,
            opt_str(row, "title").unwrap_or_default(),
            opt_str(row, "summary").unwrap_or_default(),
        ))
    })?;
    for r in rows {
        let (tree, spec, title, summary) = r?;
        let key = (tree.clone(), spec.clone());
        let props = json!({
            "tree": tree,
            "spec": spec,
            "kind": "backlog",
            "title": title,
            "summary": summary,
            "blocked_by": blocked.get(&key).cloned().unwrap_or_default(),
        });
        let claim = build_claim(ClaimType::Campaign, props, Utc::now(), None);
        match store.append(&claim) {
            Ok(()) => s.campaigns += 1,
            Err(e) => s.skipped.push(format!("campaign_backlog: {e}")),
        }
    }
    Ok(())
}

fn migrate_sessions(conn: &Connection, store: &mut Store, s: &mut MigrationSummary) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id, started, owner, tree, spec, summary, status FROM session_meta",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>("id")?,
            parse_ts(row, "started"),
            opt_str(row, "owner"),
            opt_str(row, "tree"),
            opt_str(row, "spec"),
            opt_str(row, "summary").unwrap_or_default(),
            opt_str(row, "status").unwrap_or_else(|| "merged".into()),
        ))
    })?;
    for r in rows {
        let (id, started, _owner, tree, spec, summary, _status) = r?;
        let props = json!({
            "id": id,
            "tree": tree,
            "spec": spec,
            "summary": summary,
        });
        let claim = build_claim(ClaimType::Session, props, started, None);
        match store.append(&claim) {
            Ok(()) => s.sessions += 1,
            Err(e) => s.skipped.push(format!("session: {e}")),
        }
    }
    Ok(())
}

fn migrate_stakeholders(
    conn: &Connection,
    store: &mut Store,
    s: &mut MigrationSummary,
) -> Result<()> {
    let orgs: std::collections::HashMap<(String, String), Vec<String>> = {
        let mut stmt =
            conn.prepare("SELECT tree, stakeholder_id, org FROM stakeholder_orgs")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>("tree")?,
                row.get::<_, String>("stakeholder_id")?,
                row.get::<_, String>("org")?,
            ))
        })?;
        let mut m: std::collections::HashMap<(String, String), Vec<String>> = Default::default();
        for r in rows {
            let (t, sh, o) = r?;
            m.entry((t, sh)).or_default().push(o);
        }
        m
    };

    let mut stmt = conn.prepare("SELECT tree, id, name, context FROM stakeholders")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>("tree")?,
            row.get::<_, String>("id")?,
            opt_str(row, "name"),
            row.get::<_, String>("context")?,
        ))
    })?;
    for r in rows {
        let (tree, id, name, context) = r?;
        let key = (tree.clone(), id.clone());
        let props = json!({
            "id": id,
            "name": name,
            "context": context,
            "orgs": orgs.get(&key).cloned().unwrap_or_default(),
        });
        let claim = build_claim(ClaimType::Stakeholder, props, Utc::now(), None);
        match store.append(&claim) {
            Ok(()) => s.stakeholders += 1,
            Err(e) => s.skipped.push(format!("stakeholder: {e}")),
        }
    }
    Ok(())
}

fn migrate_dispositions(
    conn: &Connection,
    store: &mut Store,
    s: &mut MigrationSummary,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT tree, spec, id, stakeholder_id, topic, stance, preferred_approach, detail, confidence, valid_from, valid_until, superseded_by FROM dispositions",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>("tree")?,
            row.get::<_, String>("spec")?,
            row.get::<_, String>("id")?,
            row.get::<_, String>("stakeholder_id")?,
            row.get::<_, String>("topic")?,
            row.get::<_, String>("stance")?,
            opt_str(row, "preferred_approach").unwrap_or_default(),
            opt_str(row, "detail").unwrap_or_default(),
            row.get::<_, String>("confidence")?,
            parse_ts(row, "valid_from"),
            opt_str(row, "valid_until"),
            opt_str(row, "superseded_by"),
        ))
    })?;
    for r in rows {
        let (_tree, _spec, _id, stakeholder_id, topic, stance, preferred, detail, confidence, vf, _vu, _sb) =
            r?;
        let props = json!({
            "stakeholder_id": stakeholder_id,
            "topic": topic,
            "stance": stance,
            "confidence": confidence,
            "preferred_approach": preferred,
            "detail": detail,
        });
        let claim = build_claim(ClaimType::Disposition, props, vf, None);
        match store.append(&claim) {
            Ok(()) => s.dispositions += 1,
            Err(e) => s.skipped.push(format!("disposition: {e}")),
        }
    }
    Ok(())
}

fn migrate_signals(conn: &Connection, store: &mut Store, s: &mut MigrationSummary) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT tree, spec, id, stakeholder_id, date, recorded_date, source, source_type, content, interpretation, our_action FROM signals",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>("tree")?,
            row.get::<_, String>("spec")?,
            row.get::<_, String>("id")?,
            row.get::<_, String>("stakeholder_id")?,
            row.get::<_, String>("date")?,
            opt_str(row, "recorded_date").unwrap_or_default(),
            row.get::<_, String>("source")?,
            row.get::<_, String>("source_type")?,
            row.get::<_, String>("content")?,
            opt_str(row, "interpretation").unwrap_or_default(),
            opt_str(row, "our_action").unwrap_or_default(),
        ))
    })?;
    for r in rows {
        let (_tree, _spec, _id, stakeholder_id, date, recorded_date, source, source_type, content, interpretation, our_action) =
            r?;
        let ts = DateTime::parse_from_rfc3339(&date)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let props = json!({
            "stakeholder_id": stakeholder_id,
            "source": source,
            "source_type": source_type,
            "content": content,
            "event_date": date,
            "record_date": recorded_date,
            "interpretation": interpretation,
            "our_action": our_action,
        });
        let claim = build_claim(ClaimType::Signal, props, ts, None);
        match store.append(&claim) {
            Ok(()) => s.signals += 1,
            Err(e) => s.skipped.push(format!("signal: {e}")),
        }
    }
    Ok(())
}

fn migrate_phase(conn: &Connection, store: &mut Store, s: &mut MigrationSummary) -> Result<()> {
    let name: Option<String> = conn
        .query_row(
            "SELECT name FROM phase WHERE id = 1",
            params![],
            |row| row.get(0),
        )
        .ok();
    if let Some(name) = name {
        let props = json!({
            "session_id": "migrated-singleton",
            "name": name,
        });
        let claim = build_claim(ClaimType::Phase, props, Utc::now(), None);
        match store.append(&claim) {
            Ok(()) => s.phase += 1,
            Err(e) => s.skipped.push(format!("phase: {e}")),
        }
    }
    Ok(())
}

// =============================================================================
// CLI wrappers — v2.1 folded the standalone `synthesist-migrate-v1-to-v2`
// binary into synthesist proper as a subcommand. Single install path,
// single binary users already have, composes with the rest of the CLI.
// See /Users/andrewdunn/gitlab.com/nomograph/synthesist/MIGRATION.md.
// =============================================================================

use crate::cli::MigrateCmd;
use crate::store::json_out;

/// Dispatch a `synthesist migrate <...>` subcommand.
pub fn run(cmd: &MigrateCmd) -> anyhow::Result<()> {
    match cmd {
        MigrateCmd::Status => cmd_status(),
        MigrateCmd::V1ToV2 {
            from,
            to,
            dry_run,
            overwrite,
        } => cmd_v1_to_v2(from, to, *dry_run, *overwrite),
    }
}

/// `synthesist migrate status` — report claim-substrate state and
/// whether a legacy v1 db is present. Named explicitly (was the old
/// `synthesist migrate` no-arg behavior before v2.1).
fn cmd_status() -> anyhow::Result<()> {
    let legacy_db = std::path::Path::new(".synth/main.db");
    let status = if legacy_db.exists() {
        serde_json::json!({
            "v1_legacy_present": true,
            "next_action": "run `synthesist migrate v1-to-v2 --from .synth/main.db --to claims/`",
            "docs": "synthesist/MIGRATION.md",
        })
    } else {
        serde_json::json!({
            "v1_legacy_present": false,
            "schema_owner": "nomograph-claim",
            "note": "v2 claim store has no versioned migrations; genesis.amc + changes/ ARE the schema",
        })
    };
    json_out(&status)
}

/// `synthesist migrate v1-to-v2` — one-shot port of a v1 SQLite db to
/// a v2 claim log.
fn cmd_v1_to_v2(
    from: &std::path::Path,
    to: &std::path::Path,
    dry_run: bool,
    overwrite: bool,
) -> anyhow::Result<()> {
    match migrate(from, to, dry_run, overwrite) {
        Ok(summary) => {
            json_out(&serde_json::json!({
                "ok": true,
                "dry_run": dry_run,
                "from": from.display().to_string(),
                "to": to.display().to_string(),
                "counts": {
                    "trees": summary.trees,
                    "specs": summary.specs,
                    "tasks": summary.tasks,
                    "discoveries": summary.discoveries,
                    "campaigns": summary.campaigns,
                    "sessions": summary.sessions,
                    "stakeholders": summary.stakeholders,
                    "dispositions": summary.dispositions,
                    "signals": summary.signals,
                    "phase": summary.phase,
                },
                "total_claims_appended": summary.total(),
                "skipped": summary.skipped,
                "next_actions": [
                    "run `synthesist check` to verify claim integrity",
                    "run `synthesist status` to confirm trees/tasks match your v1 counts",
                    "commit the claims/ directory to git",
                ],
            }))
        }
        Err(MigrateError::AlreadyMigrated) => {
            anyhow::bail!(
                "destination already migrated; pass --overwrite to re-migrate (and see MIGRATION.md for rollback)"
            )
        }
        Err(MigrateError::SourceMissing(p)) => {
            anyhow::bail!("source db not found at {p}")
        }
        Err(e) => Err(anyhow::anyhow!(e.to_string())),
    }
}

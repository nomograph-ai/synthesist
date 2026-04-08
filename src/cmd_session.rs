//! Session commands: start, merge, list, status, discard.
//!
//! Session isolation uses per-file database copies. Each session gets a copy
//! of main.db with `_snapshot_<table>` tables capturing base state.
//!
//! ## Merge algorithm (EXCEPT-based three-way diff)
//!
//! For each merge table with a known primary key:
//!
//! 1. **Session adds**: PK in session table but not in snapshot.
//! 2. **Session deletes**: PK in snapshot but not in session table.
//! 3. **Session modifies**: PK in both, but full row content differs
//!    (detected via `EXCEPT`).
//! 4. **Concurrent changes**: same analysis against main vs snapshot.
//! 5. **Conflict**: a PK touched by both session and main. Resolved by
//!    `--ours` (session wins) or `--theirs` (main wins). Without a flag,
//!    merge aborts on conflict.
//! 6. **Apply**: INSERT OR REPLACE for adds/mods, DELETE for removes,
//!    wrapped in a single BEGIN/COMMIT on main.db.

use std::fs;

use anyhow::{bail, Result};
use serde_json::json;

use crate::store::{json_out, Store};

pub fn run(cmd: &crate::cli::SessionCmd) -> Result<()> {
    match cmd {
        crate::cli::SessionCmd::Start {
            id,
            tree,
            spec,
            summary,
        } => cmd_session_start(id, tree.as_deref(), spec.as_deref(), summary.as_deref()),
        crate::cli::SessionCmd::Merge {
            id,
            dry_run,
            ours,
            theirs,
        } => cmd_session_merge(id, *dry_run, *ours, *theirs),
        crate::cli::SessionCmd::List => cmd_session_list(),
        crate::cli::SessionCmd::Status { id } => cmd_session_status(id),
        crate::cli::SessionCmd::Discard { id } => cmd_session_discard(id),
    }
}

/// Tables that participate in session merge (have snapshot counterparts).
const MERGE_TABLES: &[&str] = &[
    "trees",
    "specs",
    "tasks",
    "task_deps",
    "task_files",
    "acceptance",
    "discoveries",
    "stakeholders",
    "stakeholder_orgs",
    "dispositions",
    "signals",
    "campaign_active",
    "campaign_backlog",
    "campaign_blocked_by",
    "session_meta",
    "phase",
];

/// Return the primary key column(s) for each merge table.
fn pk_columns(table: &str) -> &'static [&'static str] {
    match table {
        "trees" => &["name"],
        "specs" => &["tree", "id"],
        "tasks" => &["tree", "spec", "id"],
        "task_deps" => &["tree", "spec", "task_id", "depends_on"],
        "task_files" => &["tree", "spec", "task_id", "path"],
        "acceptance" => &["tree", "spec", "task_id", "seq"],
        "discoveries" => &["tree", "spec", "id"],
        "stakeholders" => &["tree", "id"],
        "stakeholder_orgs" => &["tree", "stakeholder_id", "org"],
        "dispositions" => &["tree", "spec", "id"],
        "signals" => &["tree", "spec", "id"],
        "campaign_active" => &["tree", "spec_id"],
        "campaign_backlog" => &["tree", "spec_id"],
        "campaign_blocked_by" => &["tree", "spec_id", "blocked_by"],
        "session_meta" => &["id"],
        "phase" => &["id"],
        _ => &[],
    }
}

/// Build a SQL join condition matching primary key columns between two aliases.
fn pk_join(table: &str, left: &str, right: &str) -> String {
    pk_columns(table)
        .iter()
        .map(|col| format!("{left}.[{col}] = {right}.[{col}]"))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Per-table diff result.
#[derive(Debug)]
struct TableDiff {
    table: String,
    added: i64,
    modified: i64,
    deleted: i64,
    conflicts: i64,
}

/// Compute the three-way diff for one table. Requires session_db already ATTACHed.
///
/// Prefixes: `session_db.<table>` = session current, `session_db.<snapshot>` = base,
/// `main.<table>` = main current.
fn diff_table(conn: &rusqlite::Connection, table: &str) -> Result<Option<TableDiff>> {
    let snapshot = format!("_snapshot_{table}");

    // Check snapshot exists in session_db
    let has_snapshot: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM session_db.sqlite_master WHERE type='table' AND name=?1",
            [&snapshot],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);
    if !has_snapshot {
        return Ok(None);
    }

    let pks = pk_columns(table);
    if pks.is_empty() {
        return Ok(None);
    }

    let pk_join_ss = pk_join(table, "s", "snap"); // session vs snapshot
    let pk_join_ms = pk_join(table, "m", "snap"); // main vs snapshot

    // --- Session changes ---

    // Added in session: PK in session but not in snapshot.
    let session_added: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM session_db.[{table}] s
             WHERE NOT EXISTS (
                 SELECT 1 FROM session_db.[{snapshot}] snap WHERE {pk_join_ss}
             )"
        ),
        [],
        |row| row.get(0),
    )?;

    // Deleted in session: PK in snapshot but not in session.
    let session_deleted: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM session_db.[{snapshot}] snap
             WHERE NOT EXISTS (
                 SELECT 1 FROM session_db.[{table}] s WHERE {pk_join_ss}
             )"
        ),
        [],
        |row| row.get(0),
    )?;

    // Modified in session: PK in both, but full row differs.
    // Use EXCEPT: rows in session that are not byte-identical in snapshot,
    // restricted to PKs present in both.
    let session_modified: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM (
                 SELECT * FROM session_db.[{table}]
                 EXCEPT
                 SELECT * FROM session_db.[{snapshot}]
             ) diff
             WHERE EXISTS (
                 SELECT 1 FROM session_db.[{snapshot}] snap
                 WHERE {pk_join_snap_diff}
             )",
            pk_join_snap_diff = pk_columns(table)
                .iter()
                .map(|col| format!("snap.[{col}] = diff.[{col}]"))
                .collect::<Vec<_>>()
                .join(" AND ")
        ),
        [],
        |row| row.get(0),
    )?;

    // --- Concurrent main changes (for conflict detection) ---
    // Changed PKs in main: rows in main that differ from snapshot (add/mod/del).
    // We only need PKs that overlap with session changes for conflict detection.

    // Main modified or added PKs (relative to snapshot).
    // "main changed" = PK where main row differs from snapshot row, OR PK exists
    // in main but not snapshot (added), OR PK exists in snapshot but not main (deleted).

    // Count conflicts: PKs that are changed in BOTH session AND main relative to snapshot.
    // A PK is "session-changed" if it was added, deleted, or modified in session.
    // A PK is "main-changed" if it was added, deleted, or modified in main.

    let pk_cols_bare = pk_columns(table)
        .iter()
        .map(|c| format!("[{c}]"))
        .collect::<Vec<_>>()
        .join(", ");

    // Session-touched PKs (union of added + deleted + modified PKs).
    let session_touched_pks = format!(
        "SELECT {pk_cols_bare} FROM session_db.[{table}] s
         WHERE NOT EXISTS (SELECT 1 FROM session_db.[{snapshot}] snap WHERE {pk_join_ss})
         UNION
         SELECT {pk_cols_bare} FROM session_db.[{snapshot}] snap
         WHERE NOT EXISTS (SELECT 1 FROM session_db.[{table}] s WHERE {pk_join_ss})
         UNION
         SELECT {pk_cols_bare} FROM (
             SELECT * FROM session_db.[{table}] EXCEPT SELECT * FROM session_db.[{snapshot}]
         ) diff
         WHERE EXISTS (SELECT 1 FROM session_db.[{snapshot}] snap WHERE {pk_join_snap_diff})",
        pk_join_snap_diff = pk_columns(table)
            .iter()
            .map(|col| format!("snap.[{col}] = diff.[{col}]"))
            .collect::<Vec<_>>()
            .join(" AND ")
    );

    // Main-touched PKs: same logic but main vs snapshot.
    let pk_join_ms_snap = pk_join(table, "m", "snap");
    let main_touched_pks = format!(
        "SELECT {pk_cols_bare} FROM main.[{table}] m
         WHERE NOT EXISTS (SELECT 1 FROM session_db.[{snapshot}] snap WHERE {pk_join_ms})
         UNION
         SELECT {pk_cols_bare} FROM session_db.[{snapshot}] snap
         WHERE NOT EXISTS (SELECT 1 FROM main.[{table}] m WHERE {pk_join_ms_snap})
         UNION
         SELECT {pk_cols_bare} FROM (
             SELECT * FROM main.[{table}] EXCEPT SELECT * FROM session_db.[{snapshot}]
         ) diff
         WHERE EXISTS (SELECT 1 FROM session_db.[{snapshot}] snap WHERE {pk_join_snap_diff})",
        pk_join_snap_diff = pk_columns(table)
            .iter()
            .map(|col| format!("snap.[{col}] = diff.[{col}]"))
            .collect::<Vec<_>>()
            .join(" AND ")
    );

    // Conflict count: intersection of session-touched and main-touched PKs.
    let pk_join_st_mt = pk_columns(table)
        .iter()
        .map(|col| format!("st.[{col}] = mt.[{col}]"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let conflicts: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM ({session_touched_pks}) st
             WHERE EXISTS (SELECT 1 FROM ({main_touched_pks}) mt WHERE {pk_join_st_mt})"
        ),
        [],
        |row| row.get(0),
    )?;

    if session_added == 0 && session_modified == 0 && session_deleted == 0 && conflicts == 0 {
        return Ok(None);
    }

    Ok(Some(TableDiff {
        table: table.to_string(),
        added: session_added,
        modified: session_modified,
        deleted: session_deleted,
        conflicts,
    }))
}

fn cmd_session_start(
    id: &str,
    tree: Option<&str>,
    spec: Option<&str>,
    summary: Option<&str>,
) -> Result<()> {
    // Validate session ID against path traversal.
    if id.contains('/') || id.contains('\\') || id.contains("..") || id.is_empty() {
        bail!("invalid session ID '{id}': must not contain path separators or '..'");
    }

    let store = Store::discover()?;
    let session_path = store.session_db_path(id);

    if session_path.exists() {
        bail!("session '{id}' already exists");
    }

    // Ensure sessions directory exists
    fs::create_dir_all(store.sessions_dir())?;

    // Copy main.db to session file
    let main_path = store.main_db_path();
    fs::copy(&main_path, &session_path)?;

    // Open the session database and create snapshot tables
    let session_conn = rusqlite::Connection::open(&session_path)?;
    session_conn.execute_batch(
        "PRAGMA journal_mode = DELETE;
         PRAGMA foreign_keys = ON;",
    )?;

    for table in MERGE_TABLES {
        // Skip session_meta and phase from snapshots (session-local state)
        if *table == "session_meta" || *table == "phase" || *table == "config" {
            continue;
        }
        let snapshot = format!("_snapshot_{table}");
        session_conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS [{snapshot}] AS SELECT * FROM [{table}];"
        ))?;
    }

    // Record session metadata in the session database
    let today = Store::today();
    session_conn.execute(
        "INSERT OR REPLACE INTO session_meta (id, started, owner, tree, spec, summary, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active')",
        rusqlite::params![id, today, id, tree, spec, summary],
    )?;

    // Also record in main.db so session list works without opening session files
    store.conn.execute(
        "INSERT INTO session_meta (id, started, owner, tree, spec, summary, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active')",
        rusqlite::params![id, today, id, tree, spec, summary],
    )?;

    json_out(&json!({
        "id": id,
        "status": "active",
        "started": today,
        "path": session_path.display().to_string(),
    }))
}

fn cmd_session_merge(id: &str, dry_run: bool, ours: bool, theirs: bool) -> Result<()> {
    let store = Store::discover()?;
    let session_path = store.session_db_path(id);

    if !session_path.exists() {
        bail!("session '{id}' not found");
    }

    // Attach session database to main connection
    store.conn.execute(
        "ATTACH DATABASE ?1 AS session_db",
        [session_path.to_str().unwrap()],
    )?;

    // Phase 1: compute per-table diffs
    let mut diffs: Vec<TableDiff> = Vec::new();
    for table in MERGE_TABLES {
        if *table == "session_meta" || *table == "phase" || *table == "config" {
            continue;
        }
        if let Some(d) = diff_table(&store.conn, table)? {
            diffs.push(d);
        }
    }

    // Build JSON summary
    let changes: Vec<serde_json::Value> = diffs
        .iter()
        .map(|d| {
            json!({
                "table": d.table,
                "added": d.added,
                "modified": d.modified,
                "deleted": d.deleted,
                "conflicts": d.conflicts,
            })
        })
        .collect();

    let total_conflicts: i64 = diffs.iter().map(|d| d.conflicts).sum();

    // Abort on unresolved conflicts unless a resolution strategy is given
    if total_conflicts > 0 && !ours && !theirs {
        store.conn.execute("DETACH DATABASE session_db", [])?;
        bail!(
            "merge aborted: {total_conflicts} conflict(s) detected. \
             Use --ours (session wins) or --theirs (main wins) to resolve."
        );
    }

    if dry_run {
        store.conn.execute("DETACH DATABASE session_db", [])?;
        return json_out(&json!({
            "id": id,
            "dry_run": true,
            "changes": changes,
            "conflicts": total_conflicts,
        }));
    }

    // Phase 2: apply session changes to main.db in a single transaction
    store.conn.execute("BEGIN IMMEDIATE", [])?;

    for table in MERGE_TABLES {
        if *table == "session_meta" || *table == "phase" || *table == "config" {
            continue;
        }

        let snapshot = format!("_snapshot_{table}");

        // Check snapshot exists (same check as diff_table)
        let has_snapshot: bool = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM session_db.sqlite_master WHERE type='table' AND name=?1",
                [&snapshot],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);
        if !has_snapshot {
            continue;
        }

        let pks = pk_columns(table);
        if pks.is_empty() {
            continue;
        }

        let pk_join_snap_diff = pks
            .iter()
            .map(|col| format!("snap.[{col}] = diff.[{col}]"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let pk_join_ms = pk_join(table, "m", "snap");

        // Build a conflict PK set (if there are conflicts and we need to
        // skip or override). We need this to decide per-row whether to apply.
        // For --ours: apply all session changes (even conflicting).
        // For --theirs: skip session changes that conflict (main wins).
        // No conflict flag: we already bailed above if conflicts > 0.

        // Subquery: PKs changed in main (relative to snapshot).
        let pk_cols_bare = pks
            .iter()
            .map(|c| format!("[{c}]"))
            .collect::<Vec<_>>()
            .join(", ");
        let main_touched_sub = format!(
            "SELECT {pk_cols_bare} FROM main.[{table}] m
             WHERE NOT EXISTS (SELECT 1 FROM session_db.[{snapshot}] snap WHERE {pk_join_ms})
             UNION
             SELECT {pk_cols_bare} FROM session_db.[{snapshot}] snap
             WHERE NOT EXISTS (SELECT 1 FROM main.[{table}] m WHERE {pk_join_ms})
             UNION
             SELECT {pk_cols_bare} FROM (
                 SELECT * FROM main.[{table}] EXCEPT SELECT * FROM session_db.[{snapshot}]
             ) diff
             WHERE EXISTS (SELECT 1 FROM session_db.[{snapshot}] snap WHERE {pk_join_snap_diff})"
        );

        // If --theirs, we need to exclude conflicting PKs from session apply.
        // Build a NOT EXISTS clause against main-touched PKs.
        let conflict_filter = if theirs {
            let pk_join_row_mt = pks
                .iter()
                .map(|col| format!("_src.[{col}] = _mt.[{col}]"))
                .collect::<Vec<_>>()
                .join(" AND ");
            format!(
                " AND NOT EXISTS (SELECT 1 FROM ({main_touched_sub}) _mt WHERE {pk_join_row_mt})"
            )
        } else {
            String::new()
        };

        // --- Apply session deletes ---
        // Delete from main where PK was in snapshot but removed in session.
        let pk_join_m_snap = pk_join(table, "m", "snap");
        let pk_join_m_s = pk_join(table, "m", "s");

        // For the conflict filter on deletes, _src = m (the main row).
        let delete_conflict_filter = if theirs {
            let pk_join_m_mt = pks
                .iter()
                .map(|col| format!("m.[{col}] = _mt.[{col}]"))
                .collect::<Vec<_>>()
                .join(" AND ");
            format!(
                " AND NOT EXISTS (SELECT 1 FROM ({main_touched_sub}) _mt WHERE {pk_join_m_mt})"
            )
        } else {
            String::new()
        };

        store.conn.execute_batch(&format!(
            "DELETE FROM main.[{table}] WHERE EXISTS (
                 SELECT 1 FROM main.[{table}] m
                 WHERE {pk_self_join}
                   AND EXISTS (
                       SELECT 1 FROM session_db.[{snapshot}] snap WHERE {pk_join_m_snap}
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM session_db.[{table}] s WHERE {pk_join_m_s}
                   )
                   {delete_conflict_filter}
             );",
            pk_self_join = pks
                .iter()
                .map(|col| format!("main.[{table}].[{col}] = m.[{col}]"))
                .collect::<Vec<_>>()
                .join(" AND ")
        ))?;

        // --- Apply session adds (PK in session, not in snapshot) ---
        let pk_join_src_snap = pks
            .iter()
            .map(|col| format!("_src.[{col}] = snap.[{col}]"))
            .collect::<Vec<_>>()
            .join(" AND ");

        store.conn.execute_batch(&format!(
            "INSERT OR REPLACE INTO main.[{table}]
             SELECT * FROM session_db.[{table}] _src
             WHERE NOT EXISTS (
                 SELECT 1 FROM session_db.[{snapshot}] snap WHERE {pk_join_src_snap}
             ){conflict_filter};"
        ))?;

        // --- Apply session modifications (PK in both, row differs) ---
        // Use EXCEPT to get changed rows, then filter to rows whose PK
        // exists in the snapshot (i.e. modifications, not adds).
        store.conn.execute_batch(&format!(
            "INSERT OR REPLACE INTO main.[{table}]
             SELECT * FROM (
                 SELECT * FROM session_db.[{table}]
                 EXCEPT
                 SELECT * FROM session_db.[{snapshot}]
             ) _src
             WHERE EXISTS (
                 SELECT 1 FROM session_db.[{snapshot}] snap WHERE {pk_join_src_snap}
             ){conflict_filter};"
        ))?;
    }

    // Mark session as merged in main.db
    store.conn.execute(
        "UPDATE main.session_meta SET status = 'merged' WHERE id = ?1",
        [id],
    )?;

    store.conn.execute("COMMIT", [])?;
    store.conn.execute("DETACH DATABASE session_db", [])?;

    // Remove session database file
    fs::remove_file(&session_path)?;

    json_out(&json!({
        "id": id,
        "status": "merged",
        "changes": changes,
        "conflicts_resolved": if total_conflicts > 0 {
            if ours { "ours" } else { "theirs" }
        } else {
            "none"
        },
    }))
}

fn cmd_session_list() -> Result<()> {
    let store = Store::discover()?;
    let mut stmt = store.conn.prepare(
        "SELECT id, started, owner, tree, spec, summary, status FROM session_meta ORDER BY started DESC",
    )?;
    let sessions: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "started": row.get::<_, String>(1)?,
                "owner": row.get::<_, Option<String>>(2)?,
                "tree": row.get::<_, Option<String>>(3)?,
                "spec": row.get::<_, Option<String>>(4)?,
                "summary": row.get::<_, Option<String>>(5)?,
                "status": row.get::<_, String>(6)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    json_out(&json!({"sessions": sessions}))
}

fn cmd_session_status(id: &str) -> Result<()> {
    let store = Store::discover()?;
    let session_path = store.session_db_path(id);

    if !session_path.exists() {
        bail!("session '{id}' not found");
    }

    // Attach session database to main connection for cross-db queries
    store.conn.execute(
        "ATTACH DATABASE ?1 AS session_db",
        [session_path.to_str().unwrap()],
    )?;

    // Compute per-table diffs using the shared diff_table helper
    let mut changes: Vec<serde_json::Value> = Vec::new();
    let mut total_added: i64 = 0;
    let mut total_modified: i64 = 0;
    let mut total_deleted: i64 = 0;
    let mut total_conflicts: i64 = 0;

    for table in MERGE_TABLES {
        if *table == "session_meta" || *table == "phase" || *table == "config" {
            continue;
        }
        if let Some(d) = diff_table(&store.conn, table)? {
            total_added += d.added;
            total_modified += d.modified;
            total_deleted += d.deleted;
            total_conflicts += d.conflicts;
            changes.push(json!({
                "table": d.table,
                "added": d.added,
                "modified": d.modified,
                "deleted": d.deleted,
                "conflicts": d.conflicts,
            }));
        }
    }

    // Read session metadata
    let meta = store
        .conn
        .query_row(
            "SELECT started, owner, tree, spec, summary, status \
             FROM session_db.session_meta WHERE id = ?1",
            [id],
            |row| {
                Ok(json!({
                    "started": row.get::<_, Option<String>>(0)?,
                    "owner": row.get::<_, Option<String>>(1)?,
                    "tree": row.get::<_, Option<String>>(2)?,
                    "spec": row.get::<_, Option<String>>(3)?,
                    "summary": row.get::<_, Option<String>>(4)?,
                    "status": row.get::<_, String>(5)?,
                }))
            },
        )
        .ok();

    store.conn.execute("DETACH DATABASE session_db", [])?;

    json_out(&json!({
        "id": id,
        "meta": meta,
        "changes": changes,
        "totals": {
            "added": total_added,
            "modified": total_modified,
            "deleted": total_deleted,
            "conflicts": total_conflicts,
        },
    }))
}

fn cmd_session_discard(id: &str) -> Result<()> {
    let store = Store::discover()?;
    let session_path = store.session_db_path(id);

    if !session_path.exists() {
        bail!("session '{id}' not found");
    }

    fs::remove_file(&session_path)?;

    store.conn.execute(
        "UPDATE session_meta SET status = 'discarded' WHERE id = ?1",
        [id],
    )?;

    json_out(&json!({"id": id, "status": "discarded"}))
}

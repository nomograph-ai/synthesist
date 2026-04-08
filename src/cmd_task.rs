//! Task DAG commands.

use std::process::Command as ShellCommand;

use anyhow::{bail, Result};
use serde_json::json;

use crate::cli::TaskCmd;
use crate::store::{json_out, parse_tree_spec, Store};

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
                tree,
                spec,
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
            cmd_task_list(tree, spec, *active, session)
        }
        TaskCmd::Show { tree_spec, task_id } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_show(tree, spec, task_id, session)
        }
        TaskCmd::Update {
            tree_spec,
            task_id,
            summary,
            description,
            files,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_update(
                tree,
                spec,
                task_id,
                summary.as_deref(),
                description.as_deref(),
                files.as_ref(),
                session,
            )
        }
        TaskCmd::Claim { tree_spec, task_id } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_claim(tree, spec, task_id, session)
        }
        TaskCmd::Done {
            tree_spec,
            task_id,
            skip_verify,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_done(tree, spec, task_id, *skip_verify, session)
        }
        TaskCmd::Reset {
            tree_spec,
            task_id,
            session: reset_session,
            reason,
        } => cmd_task_reset(
            tree_spec.as_deref(),
            task_id.as_deref(),
            reset_session.as_deref(),
            reason.as_deref(),
            session,
        ),
        TaskCmd::Block { tree_spec, task_id } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_block(tree, spec, task_id, session)
        }
        TaskCmd::Wait {
            tree_spec,
            task_id,
            reason,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_wait(tree, spec, task_id, reason, session)
        }
        TaskCmd::Cancel {
            tree_spec,
            task_id,
            reason,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_cancel(tree, spec, task_id, reason.as_deref(), session)
        }
        TaskCmd::Ready { tree_spec } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_ready(tree, spec, session)
        }
        TaskCmd::Acceptance {
            tree_spec,
            task_id,
            criterion,
            verify,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_task_acceptance(tree, spec, task_id, criterion, verify, session)
        }
    }
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
    let store = Store::discover_for(session)?;
    let task_id = match id {
        Some(id) => id.to_string(),
        None => store.next_id("tasks", tree, spec, "t")?,
    };
    let today = Store::today();

    // Wrap multi-table write in a transaction for atomicity.
    store.conn.execute("BEGIN IMMEDIATE", [])?;

    let result = (|| -> Result<()> {
        // Auto-ensure parent tree and spec exist (idempotent) for FK integrity.
        store.conn.execute(
            "INSERT OR IGNORE INTO trees (name) VALUES (?1)",
            rusqlite::params![tree],
        )?;
        store.conn.execute(
            "INSERT OR IGNORE INTO specs (tree, id, created) VALUES (?1, ?2, ?3)",
            rusqlite::params![tree, spec, today],
        )?;

        store.conn.execute(
            "INSERT INTO tasks (tree, spec, id, summary, description, status, gate, created) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7)",
            rusqlite::params![tree, spec, task_id, summary, description, gate, today],
        )?;

        for dep in depends_on {
            if dep.is_empty() {
                continue;
            }
            store.conn.execute(
                "INSERT INTO task_deps (tree, spec, task_id, depends_on) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![tree, spec, task_id, dep],
            )?;
        }

        for file in files {
            if file.is_empty() {
                continue;
            }
            store.conn.execute(
                "INSERT INTO task_files (tree, spec, task_id, path) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![tree, spec, task_id, file],
            )?;
        }
        Ok(())
    })();

    match result {
        Ok(()) => store.conn.execute("COMMIT", [])?,
        Err(e) => {
            store.conn.execute("ROLLBACK", []).ok();
            return Err(e);
        }
    };

    json_out(&json!({
        "id": task_id,
        "summary": summary,
        "status": "pending",
        "depends_on": depends_on,
        "created": today,
    }))
}

fn cmd_task_list(tree: &str, spec: &str, active: bool, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let sql = if active {
        "SELECT id, summary, status, owner, gate, created, completed, failure_note, wait_reason FROM tasks WHERE tree = ?1 AND spec = ?2 AND status != 'cancelled' ORDER BY id"
    } else {
        "SELECT id, summary, status, owner, gate, created, completed, failure_note, wait_reason FROM tasks WHERE tree = ?1 AND spec = ?2 ORDER BY id"
    };

    let mut stmt = store.conn.prepare(sql)?;
    let tasks: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params![tree, spec], |row| {
            let mut m = serde_json::Map::new();
            m.insert("id".into(), json!(row.get::<_, String>(0)?));
            m.insert("summary".into(), json!(row.get::<_, String>(1)?));
            m.insert("status".into(), json!(row.get::<_, String>(2)?));
            if let Ok(Some(v)) = row.get::<_, Option<String>>(3) {
                m.insert("owner".into(), json!(v));
            }
            if let Ok(Some(v)) = row.get::<_, Option<String>>(4) {
                m.insert("gate".into(), json!(v));
            }
            m.insert("created".into(), json!(row.get::<_, String>(5)?));
            if let Ok(Some(v)) = row.get::<_, Option<String>>(6) {
                m.insert("completed".into(), json!(v));
            }
            if let Ok(Some(v)) = row.get::<_, Option<String>>(7) {
                m.insert("failure_note".into(), json!(v));
            }
            if let Ok(Some(v)) = row.get::<_, Option<String>>(8) {
                m.insert("wait_reason".into(), json!(v));
            }
            Ok(serde_json::Value::Object(m))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;

    json_out(&json!({"tree": tree, "spec": spec, "tasks": tasks}))
}

fn cmd_task_show(tree: &str, spec: &str, task_id: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;

    let task = store.conn.query_row(
        "SELECT summary, description, status, gate, owner, created, completed, failure_note, wait_reason FROM tasks WHERE tree = ?1 AND spec = ?2 AND id = ?3",
        rusqlite::params![tree, spec, task_id],
        |row| {
            Ok(json!({
                "id": task_id,
                "summary": row.get::<_, String>(0)?,
                "description": row.get::<_, Option<String>>(1)?,
                "status": row.get::<_, String>(2)?,
                "gate": row.get::<_, Option<String>>(3)?,
                "owner": row.get::<_, Option<String>>(4)?,
                "created": row.get::<_, String>(5)?,
                "completed": row.get::<_, Option<String>>(6)?,
                "failure_note": row.get::<_, Option<String>>(7)?,
                "wait_reason": row.get::<_, Option<String>>(8)?,
            }))
        },
    );

    match task {
        Ok(mut v) => {
            // Add deps
            let mut stmt = store.conn.prepare(
                "SELECT depends_on FROM task_deps WHERE tree = ?1 AND spec = ?2 AND task_id = ?3",
            )?;
            let deps: Vec<String> = stmt
                .query_map(rusqlite::params![tree, spec, task_id], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
            v["depends_on"] = json!(deps);

            // Add files
            let mut stmt = store.conn.prepare(
                "SELECT path FROM task_files WHERE tree = ?1 AND spec = ?2 AND task_id = ?3",
            )?;
            let files: Vec<String> = stmt
                .query_map(rusqlite::params![tree, spec, task_id], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
            v["files"] = json!(files);

            // Add acceptance
            let mut stmt = store.conn.prepare(
                "SELECT criterion, verify_cmd FROM acceptance WHERE tree = ?1 AND spec = ?2 AND task_id = ?3 ORDER BY seq",
            )?;
            let acceptance: Vec<serde_json::Value> = stmt
                .query_map(rusqlite::params![tree, spec, task_id], |row| {
                    Ok(json!({
                        "criterion": row.get::<_, String>(0)?,
                        "verify": row.get::<_, String>(1)?,
                    }))
                })?
                .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
            v["acceptance"] = json!(acceptance);

            json_out(&v)
        }
        Err(_) => bail!("task not found: {tree}/{spec}/{task_id}"),
    }
}

fn cmd_task_update(
    tree: &str,
    spec: &str,
    task_id: &str,
    summary: Option<&str>,
    description: Option<&str>,
    files: Option<&Vec<String>>,
    session: &Option<String>,
) -> Result<()> {
    let store = Store::discover_for(session)?;

    if let Some(s) = summary {
        store.conn.execute(
            "UPDATE tasks SET summary = ?1 WHERE tree = ?2 AND spec = ?3 AND id = ?4",
            rusqlite::params![s, tree, spec, task_id],
        )?;
    }
    if let Some(d) = description {
        store.conn.execute(
            "UPDATE tasks SET description = ?1 WHERE tree = ?2 AND spec = ?3 AND id = ?4",
            rusqlite::params![d, tree, spec, task_id],
        )?;
    }
    if let Some(f) = files {
        store.conn.execute(
            "DELETE FROM task_files WHERE tree = ?1 AND spec = ?2 AND task_id = ?3",
            rusqlite::params![tree, spec, task_id],
        )?;
        for file in f {
            if file.is_empty() {
                continue;
            }
            store.conn.execute(
                "INSERT INTO task_files (tree, spec, task_id, path) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![tree, spec, task_id, file],
            )?;
        }
    }

    json_out(&json!({"id": task_id, "updated": true}))
}

#[allow(clippy::too_many_arguments)]
fn cmd_task_claim(tree: &str, spec: &str, task_id: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let owner = session.as_deref().unwrap_or("synthesist");

    // Wrap dep check + claim in a transaction to prevent race conditions.
    store.conn.execute("BEGIN IMMEDIATE", [])?;

    // Check deps are done
    let mut stmt = store.conn.prepare(
        "SELECT d.depends_on, t.status FROM task_deps d JOIN tasks t ON d.tree = t.tree AND d.spec = t.spec AND d.depends_on = t.id WHERE d.tree = ?1 AND d.spec = ?2 AND d.task_id = ?3",
    )?;
    let deps: Vec<(String, String)> = stmt
        .query_map(rusqlite::params![tree, spec, task_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    drop(stmt);

    for (dep_id, dep_status) in &deps {
        if dep_status != "done" {
            store.conn.execute("ROLLBACK", []).ok();
            bail!("dependency {dep_id} is {dep_status}, not done");
        }
    }

    // Atomic claim
    let affected = store.conn.execute(
        "UPDATE tasks SET status = 'in_progress', owner = ?1 WHERE tree = ?2 AND spec = ?3 AND id = ?4 AND status = 'pending' AND (owner IS NULL OR owner = '')",
        rusqlite::params![owner, tree, spec, task_id],
    )?;

    if affected == 0 {
        store.conn.execute("ROLLBACK", []).ok();
        let row = store.conn.query_row(
            "SELECT status, owner FROM tasks WHERE tree = ?1 AND spec = ?2 AND id = ?3",
            rusqlite::params![tree, spec, task_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        );
        match row {
            Ok((_status, Some(o))) if !o.is_empty() => bail!("task {task_id} already owned by {o}"),
            Ok((status, _)) => bail!("task {task_id} is {status}, not pending"),
            Err(_) => bail!("task not found: {tree}/{spec}/{task_id}"),
        }
    }

    store.conn.execute("COMMIT", [])?;
    json_out(&json!({"id": task_id, "status": "in_progress", "owner": owner}))
}

fn cmd_task_done(
    tree: &str,
    spec: &str,
    task_id: &str,
    skip_verify: bool,
    session: &Option<String>,
) -> Result<()> {
    let store = Store::discover_for(session)?;
    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut all_pass = true;

    if !skip_verify {
        let mut stmt = store.conn.prepare(
            "SELECT seq, criterion, verify_cmd FROM acceptance WHERE tree = ?1 AND spec = ?2 AND task_id = ?3 ORDER BY seq",
        )?;
        let criteria: Vec<(i32, String, String)> = stmt
            .query_map(rusqlite::params![tree, spec, task_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;

        for (_seq, criterion, verify_cmd) in &criteria {
            let output = ShellCommand::new("sh")
                .arg("-c")
                .arg(verify_cmd)
                .current_dir(&store.root)
                .output();

            let passed = output.as_ref().map(|o| o.status.success()).unwrap_or(false);
            let mut result = json!({"criterion": criterion, "verify": verify_cmd, "passed": passed});
            if !passed {
                all_pass = false;
                if let Ok(o) = &output {
                    result["output"] = json!(String::from_utf8_lossy(&o.stdout).trim().to_string());
                }
            }
            results.push(result);
        }
    }

    let today = Store::today();
    if all_pass {
        let affected = store.conn.execute(
            "UPDATE tasks SET status = 'done', completed = ?1, owner = NULL, failure_note = NULL WHERE tree = ?2 AND spec = ?3 AND id = ?4 AND status = 'in_progress'",
            rusqlite::params![today, tree, spec, task_id],
        )?;
        if affected == 0 {
            let status: String = store.conn.query_row(
                "SELECT status FROM tasks WHERE tree = ?1 AND spec = ?2 AND id = ?3",
                rusqlite::params![tree, spec, task_id],
                |row| row.get(0),
            ).map_err(|_| anyhow::anyhow!("task not found: {tree}/{spec}/{task_id}"))?;
            bail!("task {task_id} is {status}, not in_progress");
        }
    } else {
        store.conn.execute(
            "UPDATE tasks SET status = 'pending', owner = NULL, failure_note = 'acceptance criteria failed' WHERE tree = ?1 AND spec = ?2 AND id = ?3 AND status = 'in_progress'",
            rusqlite::params![tree, spec, task_id],
        )?;
    }

    json_out(&json!({
        "id": task_id,
        "all_passed": all_pass,
        "status": if all_pass { "done" } else { "pending" },
        "criteria": results,
    }))
}

fn cmd_task_reset(
    tree_spec: Option<&str>,
    task_id: Option<&str>,
    session: Option<&str>,
    reason: Option<&str>,
    outer_session: &Option<String>,
) -> Result<()> {
    let store = Store::discover_for(outer_session)?;

    // Bulk mode: reset all in_progress tasks owned by a session.
    if let Some(sess) = session {
        let query = if let Some(ts) = tree_spec {
            let (tree, spec) = parse_tree_spec(ts)?;
            store.conn.execute(
                "UPDATE tasks SET status = 'pending', owner = NULL, failure_note = ?1 WHERE owner = ?2 AND status = 'in_progress' AND tree = ?3 AND spec = ?4",
                rusqlite::params![reason, sess, tree, spec],
            )?
        } else {
            store.conn.execute(
                "UPDATE tasks SET status = 'pending', owner = NULL, failure_note = ?1 WHERE owner = ?2 AND status = 'in_progress'",
                rusqlite::params![reason, sess],
            )?
        };
        return json_out(&json!({"session": sess, "reset_count": query}));
    }

    // Single-task mode
    let ts = tree_spec.ok_or_else(|| anyhow::anyhow!("provide tree/spec + task-id or --session"))?;
    let tid = task_id.ok_or_else(|| anyhow::anyhow!("provide tree/spec + task-id or --session"))?;
    let (tree, spec) = parse_tree_spec(ts)?;

    let affected = store.conn.execute(
        "UPDATE tasks SET status = 'pending', owner = NULL, failure_note = ?1 WHERE tree = ?2 AND spec = ?3 AND id = ?4 AND status = 'in_progress'",
        rusqlite::params![reason, tree, spec, tid],
    )?;

    if affected == 0 {
        let status: String = store.conn.query_row(
            "SELECT status FROM tasks WHERE tree = ?1 AND spec = ?2 AND id = ?3",
            rusqlite::params![tree, spec, tid],
            |row| row.get(0),
        ).map_err(|_| anyhow::anyhow!("task not found: {tree}/{spec}/{tid}"))?;
        bail!("task {tid} is {status}, not in_progress");
    }

    json_out(&json!({"id": tid, "status": "pending"}))
}

#[allow(clippy::too_many_arguments)]
fn cmd_task_block(tree: &str, spec: &str, task_id: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let affected = store.conn.execute(
        "UPDATE tasks SET status = 'blocked' WHERE tree = ?1 AND spec = ?2 AND id = ?3 AND status IN ('pending', 'in_progress')",
        rusqlite::params![tree, spec, task_id],
    )?;
    if affected == 0 {
        bail!("task {task_id} cannot be blocked (not pending or in_progress)");
    }
    json_out(&json!({"id": task_id, "status": "blocked"}))
}

#[allow(clippy::too_many_arguments)]
fn cmd_task_wait(tree: &str, spec: &str, task_id: &str, reason: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let affected = store.conn.execute(
        "UPDATE tasks SET status = 'waiting', wait_reason = ?1 WHERE tree = ?2 AND spec = ?3 AND id = ?4 AND status IN ('pending', 'in_progress')",
        rusqlite::params![reason, tree, spec, task_id],
    )?;
    if affected == 0 {
        bail!("task {task_id} cannot be set to waiting (not pending or in_progress)");
    }
    json_out(&json!({"id": task_id, "status": "waiting", "reason": reason}))
}

#[allow(clippy::too_many_arguments)]
fn cmd_task_cancel(tree: &str, spec: &str, task_id: &str, reason: Option<&str>, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let affected = store.conn.execute(
        "UPDATE tasks SET status = 'cancelled', failure_note = ?1 WHERE tree = ?2 AND spec = ?3 AND id = ?4 AND status NOT IN ('done', 'cancelled')",
        rusqlite::params![reason, tree, spec, task_id],
    )?;
    if affected == 0 {
        let status: String = store.conn.query_row(
            "SELECT status FROM tasks WHERE tree = ?1 AND spec = ?2 AND id = ?3",
            rusqlite::params![tree, spec, task_id],
            |row| row.get(0),
        ).map_err(|_| anyhow::anyhow!("task not found: {tree}/{spec}/{task_id}"))?;
        bail!("task {task_id} is {status}, cannot cancel");
    }
    json_out(&json!({"id": task_id, "status": "cancelled"}))
}

fn cmd_task_ready(tree: &str, spec: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let mut stmt = store.conn.prepare(
        "SELECT t.id, t.summary, t.gate
         FROM tasks t
         WHERE t.tree = ?1 AND t.spec = ?2 AND t.status = 'pending'
         AND NOT EXISTS (
             SELECT 1 FROM task_deps d
             JOIN tasks dep ON d.tree = dep.tree AND d.spec = dep.spec AND d.depends_on = dep.id
             WHERE d.tree = t.tree AND d.spec = t.spec AND d.task_id = t.id
             AND dep.status != 'done'
         )
         ORDER BY t.id",
    )?;
    let ready: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params![tree, spec], |row| {
            let mut m = serde_json::Map::new();
            m.insert("id".into(), json!(row.get::<_, String>(0)?));
            m.insert("summary".into(), json!(row.get::<_, String>(1)?));
            if let Ok(Some(g)) = row.get::<_, Option<String>>(2) {
                m.insert("gate".into(), json!(g));
            }
            Ok(serde_json::Value::Object(m))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;

    json_out(&json!({"tree": tree, "spec": spec, "ready": ready}))
}

fn cmd_task_acceptance(
    tree: &str,
    spec: &str,
    task_id: &str,
    criterion: &str,
    verify: &str,
    session: &Option<String>,
) -> Result<()> {
    let store = Store::discover_for(session)?;
    let next_seq: i32 = store.conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM acceptance WHERE tree = ?1 AND spec = ?2 AND task_id = ?3",
        rusqlite::params![tree, spec, task_id],
        |row| row.get(0),
    )?;
    store.conn.execute(
        "INSERT INTO acceptance (tree, spec, task_id, seq, criterion, verify_cmd) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![tree, spec, task_id, next_seq, criterion, verify],
    )?;
    json_out(&json!({"task_id": task_id, "seq": next_seq, "criterion": criterion}))
}

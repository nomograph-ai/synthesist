//! Init, status, and check commands.

use anyhow::Result;
use serde_json::json;

use crate::store::{json_out, Store};

pub fn cmd_init() -> Result<()> {
    let root = std::env::current_dir()?;
    let _store = Store::init(&root)?;
    json_out(&json!({
        "status": "initialized",
        "path": root.join("synthesist").display().to_string(),
    }))
}

pub fn cmd_status() -> Result<()> {
    let store = Store::discover()?;

    let mut result = serde_json::Map::new();

    // Trees
    let mut stmt = store.conn.prepare("SELECT name, status, description FROM trees ORDER BY name")?;
    let trees: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            Ok(json!({
                "name": row.get::<_, String>(0)?,
                "status": row.get::<_, String>(1)?,
                "description": row.get::<_, String>(2)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    result.insert("trees".into(), json!(trees));

    // Task counts
    let counts = |status: &str| -> Result<i64> {
        Ok(store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status = ?1",
                [status],
                |row| row.get(0),
            )?)
    };
    result.insert(
        "task_counts".into(),
        json!({
            "pending": counts("pending")?,
            "in_progress": counts("in_progress")?,
            "done": counts("done")?,
            "waiting": counts("waiting")?,
            "blocked": counts("blocked")?,
            "cancelled": counts("cancelled")?,
        }),
    );

    // Ready tasks
    let mut stmt = store.conn.prepare(
        "SELECT t.tree, t.spec, t.id, t.summary, t.gate
         FROM tasks t
         WHERE t.status = 'pending'
         AND NOT EXISTS (
             SELECT 1 FROM task_deps d
             JOIN tasks dep ON d.tree = dep.tree AND d.spec = dep.spec AND d.depends_on = dep.id
             WHERE d.tree = t.tree AND d.spec = t.spec AND d.task_id = t.id
             AND dep.status != 'done'
         )
         ORDER BY t.tree, t.spec, t.id",
    )?;
    let ready: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            let gate: Option<String> = row.get(4)?;
            let mut m = serde_json::Map::new();
            m.insert("tree".into(), json!(row.get::<_, String>(0)?));
            m.insert("spec".into(), json!(row.get::<_, String>(1)?));
            m.insert("id".into(), json!(row.get::<_, String>(2)?));
            m.insert("summary".into(), json!(row.get::<_, String>(3)?));
            if let Some(g) = gate {
                m.insert("gate".into(), json!(g));
            }
            Ok(serde_json::Value::Object(m))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    result.insert("ready_tasks".into(), json!(ready));

    // Stakeholder count
    let stakeholder_count: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM stakeholders", [], |row| row.get(0))?;
    result.insert("stakeholder_count".into(), json!(stakeholder_count));

    // Phase
    let phase: String = store
        .conn
        .query_row("SELECT name FROM phase WHERE id = 1", [], |row| row.get(0))?;
    result.insert("phase".into(), json!(phase));

    // Active sessions
    let mut stmt = store
        .conn
        .prepare("SELECT id, started, owner, summary, status FROM session_meta ORDER BY started DESC")?;
    let sessions: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "started": row.get::<_, String>(1)?,
                "owner": row.get::<_, Option<String>>(2)?,
                "summary": row.get::<_, Option<String>>(3)?,
                "status": row.get::<_, String>(4)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    result.insert("sessions".into(), json!(sessions));

    json_out(&serde_json::Value::Object(result))
}

pub fn cmd_check() -> Result<()> {
    let store = Store::discover()?;
    let mut issues: Vec<serde_json::Value> = Vec::new();

    let add_issue = |issues: &mut Vec<serde_json::Value>, level: &str, msg: String| {
        issues.push(json!({"level": level, "message": msg}));
    };

    // Dangling task dependencies
    let mut stmt = store.conn.prepare(
        "SELECT d.tree, d.spec, d.task_id, d.depends_on
         FROM task_deps d
         WHERE NOT EXISTS (
             SELECT 1 FROM tasks t
             WHERE t.tree = d.tree AND t.spec = d.spec AND t.id = d.depends_on
         )",
    )?;
    let rows: Vec<(String, String, String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    for (tree, spec, task_id, dep) in rows {
        add_issue(
            &mut issues,
            "error",
            format!("task {tree}/{spec}/{task_id} depends on {dep} which does not exist"),
        );
    }

    // Waiting tasks without reason
    let mut stmt = store.conn.prepare(
        "SELECT tree, spec, id FROM tasks WHERE status = 'waiting' AND wait_reason IS NULL",
    )?;
    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    for (tree, spec, id) in rows {
        add_issue(
            &mut issues,
            "error",
            format!("task {tree}/{spec}/{id} is waiting but has no wait_reason"),
        );
    }

    // Disposition supersession consistency
    let mut stmt = store.conn.prepare(
        "SELECT tree, spec, id FROM dispositions WHERE valid_until IS NOT NULL AND superseded_by IS NULL",
    )?;
    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    for (tree, spec, id) in rows {
        add_issue(
            &mut issues,
            "warn",
            format!("disposition {tree}/{spec}/{id} has valid_until but no superseded_by"),
        );
    }

    let errors = issues.iter().filter(|i| i["level"] == "error").count();
    let warnings = issues.iter().filter(|i| i["level"] == "warn").count();

    json_out(&json!({
        "errors": errors,
        "warnings": warnings,
        "issues": issues,
        "passed": errors == 0,
    }))
}

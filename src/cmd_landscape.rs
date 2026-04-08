//! Stakeholder, disposition, signal, and stance commands.

use anyhow::Result;
use serde_json::json;

use crate::cli::{DispositionCmd, SignalCmd, StakeholderCmd};
use crate::store::{json_out, parse_tree_spec, Store};

// --- Stakeholder ---

pub fn run_stakeholder(cmd: &StakeholderCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        StakeholderCmd::Add {
            tree,
            id,
            context,
            name,
            orgs,
        } => cmd_stakeholder_add(tree, id, context, name.as_deref(), orgs, session),
        StakeholderCmd::List { tree } => cmd_stakeholder_list(tree, session),
    }
}

fn cmd_stakeholder_add(
    tree: &str,
    id: &str,
    context: &str,
    name: Option<&str>,
    orgs: &[String],
    session: &Option<String>,
) -> Result<()> {
    let store = Store::discover_for(session)?;
    store.conn.execute(
        "INSERT INTO stakeholders (tree, id, name, context) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![tree, id, name, context],
    )?;
    for org in orgs {
        if org.is_empty() {
            continue;
        }
        store.conn.execute(
            "INSERT INTO stakeholder_orgs (tree, stakeholder_id, org) VALUES (?1, ?2, ?3)",
            rusqlite::params![tree, id, org],
        )?;
    }
    json_out(&json!({"tree": tree, "id": id, "context": context}))
}

fn cmd_stakeholder_list(tree: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let mut stmt = store
        .conn
        .prepare("SELECT id, name, context FROM stakeholders WHERE tree = ?1 ORDER BY id")?;
    let stakeholders: Vec<serde_json::Value> = stmt
        .query_map([tree], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, Option<String>>(1)?,
                "context": row.get::<_, String>(2)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    json_out(&json!({"tree": tree, "stakeholders": stakeholders}))
}

// --- Disposition ---

pub fn run_disposition(cmd: &DispositionCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        DispositionCmd::Add {
            tree_spec,
            stakeholder,
            topic,
            stance,
            confidence,
            preferred,
            detail,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_disposition_add(
                tree,
                spec,
                stakeholder,
                topic,
                stance,
                confidence,
                preferred.as_deref(),
                detail.as_deref(),
                session,
            )
        }
        DispositionCmd::List { tree_spec } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_disposition_list(tree, spec, session)
        }
        DispositionCmd::Supersede {
            tree_spec,
            old_id,
            stance,
            confidence,
            preferred,
            detail,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_disposition_supersede(
                tree,
                spec,
                old_id,
                stance,
                confidence,
                preferred.as_deref(),
                detail.as_deref(),
                session,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_disposition_add(
    tree: &str,
    spec: &str,
    stakeholder: &str,
    topic: &str,
    stance: &str,
    confidence: &str,
    preferred: Option<&str>,
    detail: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    let store = Store::discover_for(session)?;
    let id = store.next_id("dispositions", tree, spec, "disp")?;
    let today = Store::today();
    store.conn.execute(
        "INSERT INTO dispositions (tree, spec, id, stakeholder_id, topic, stance, preferred_approach, detail, confidence, valid_from) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![tree, spec, id, stakeholder, topic, stance, preferred, detail, confidence, today],
    )?;
    json_out(&json!({
        "id": id,
        "stakeholder": stakeholder,
        "topic": topic,
        "stance": stance,
        "confidence": confidence,
    }))
}

fn cmd_disposition_list(tree: &str, spec: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let mut stmt = store.conn.prepare(
        "SELECT id, stakeholder_id, topic, stance, preferred_approach, detail, confidence, valid_from, valid_until, superseded_by FROM dispositions WHERE tree = ?1 AND spec = ?2 ORDER BY valid_from DESC",
    )?;
    let dispositions: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params![tree, spec], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "stakeholder": row.get::<_, String>(1)?,
                "topic": row.get::<_, String>(2)?,
                "stance": row.get::<_, String>(3)?,
                "preferred_approach": row.get::<_, Option<String>>(4)?,
                "detail": row.get::<_, Option<String>>(5)?,
                "confidence": row.get::<_, String>(6)?,
                "valid_from": row.get::<_, String>(7)?,
                "valid_until": row.get::<_, Option<String>>(8)?,
                "superseded_by": row.get::<_, Option<String>>(9)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    json_out(&json!({"tree": tree, "spec": spec, "dispositions": dispositions}))
}

#[allow(clippy::too_many_arguments)]
fn cmd_disposition_supersede(
    tree: &str,
    spec: &str,
    old_id: &str,
    stance: &str,
    confidence: &str,
    preferred: Option<&str>,
    detail: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    let store = Store::discover_for(session)?;

    // Get the old disposition's stakeholder and topic
    let (stakeholder, topic): (String, String) = store
        .conn
        .query_row(
            "SELECT stakeholder_id, topic FROM dispositions WHERE tree = ?1 AND spec = ?2 AND id = ?3",
            rusqlite::params![tree, spec, old_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| anyhow::anyhow!("disposition not found: {tree}/{spec}/{old_id}"))?;

    let new_id = store.next_id("dispositions", tree, spec, "disp")?;
    let today = Store::today();

    // Wrap supersession in a transaction (two rows must update atomically).
    store.conn.execute("BEGIN IMMEDIATE", [])?;

    let result = (|| -> Result<()> {
        store.conn.execute(
            "INSERT INTO dispositions (tree, spec, id, stakeholder_id, topic, stance, preferred_approach, detail, confidence, valid_from) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![tree, spec, new_id, stakeholder, topic, stance, preferred, detail, confidence, today],
        )?;
        store.conn.execute(
            "UPDATE dispositions SET valid_until = ?1, superseded_by = ?2 WHERE tree = ?3 AND spec = ?4 AND id = ?5",
            rusqlite::params![today, new_id, tree, spec, old_id],
        )?;
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
        "old_id": old_id,
        "new_id": new_id,
        "stakeholder": stakeholder,
        "topic": topic,
        "stance": stance,
    }))
}

// --- Signal ---

pub fn run_signal(cmd: &SignalCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        SignalCmd::Add {
            tree_spec,
            stakeholder,
            source,
            source_type,
            content,
            interpretation,
            our_action,
            date,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_signal_add(
                tree,
                spec,
                stakeholder,
                source,
                source_type,
                content,
                interpretation.as_deref(),
                our_action.as_deref(),
                date.as_deref(),
                session,
            )
        }
        SignalCmd::List { tree_spec } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_signal_list(tree, spec, session)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_signal_add(
    tree: &str,
    spec: &str,
    stakeholder: &str,
    source: &str,
    source_type: &str,
    content: &str,
    interpretation: Option<&str>,
    our_action: Option<&str>,
    date: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    let store = Store::discover_for(session)?;
    let id = store.next_id("signals", tree, spec, "sig")?;
    let event_date = date.unwrap_or(&Store::today()).to_string();
    let recorded = Store::today();
    store.conn.execute(
        "INSERT INTO signals (tree, spec, id, stakeholder_id, date, recorded_date, source, source_type, content, interpretation, our_action) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![tree, spec, id, stakeholder, event_date, recorded, source, source_type, content, interpretation, our_action],
    )?;
    json_out(&json!({"id": id, "stakeholder": stakeholder, "source": source, "date": event_date}))
}

fn cmd_signal_list(tree: &str, spec: &str, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;
    let mut stmt = store.conn.prepare(
        "SELECT id, stakeholder_id, date, recorded_date, source, source_type, content, interpretation, our_action FROM signals WHERE tree = ?1 AND spec = ?2 ORDER BY date DESC",
    )?;
    let signals: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params![tree, spec], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "stakeholder": row.get::<_, String>(1)?,
                "date": row.get::<_, String>(2)?,
                "recorded_date": row.get::<_, String>(3)?,
                "source": row.get::<_, String>(4)?,
                "source_type": row.get::<_, String>(5)?,
                "content": row.get::<_, String>(6)?,
                "interpretation": row.get::<_, Option<String>>(7)?,
                "our_action": row.get::<_, Option<String>>(8)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    json_out(&json!({"tree": tree, "spec": spec, "signals": signals}))
}

// --- Stance query ---

pub fn cmd_stance(stakeholder: &str, topic: Option<&str>, session: &Option<String>) -> Result<()> {
    let store = Store::discover_for(session)?;

    let sql = if topic.is_some() {
        "SELECT d.tree, d.spec, d.id, d.topic, d.stance, d.preferred_approach, d.detail, d.confidence, d.valid_from
         FROM dispositions d
         JOIN stakeholders s ON d.tree = s.tree AND d.stakeholder_id = s.id
         WHERE s.id = ?1 AND d.topic LIKE ?2 AND d.valid_until IS NULL
         ORDER BY d.valid_from DESC"
    } else {
        "SELECT d.tree, d.spec, d.id, d.topic, d.stance, d.preferred_approach, d.detail, d.confidence, d.valid_from
         FROM dispositions d
         JOIN stakeholders s ON d.tree = s.tree AND d.stakeholder_id = s.id
         WHERE s.id = ?1 AND d.valid_until IS NULL
         ORDER BY d.valid_from DESC"
    };

    let topic_pattern = topic.map(|t| format!("%{t}%")).unwrap_or_default();
    let params: Vec<&dyn rusqlite::types::ToSql> = if topic.is_some() {
        vec![&stakeholder as &dyn rusqlite::types::ToSql, &topic_pattern]
    } else {
        vec![&stakeholder as &dyn rusqlite::types::ToSql]
    };

    let mut stmt = store.conn.prepare(sql)?;
    let dispositions: Vec<serde_json::Value> = stmt
        .query_map(params.as_slice(), |row| {
            Ok(json!({
                "tree": row.get::<_, String>(0)?,
                "spec": row.get::<_, String>(1)?,
                "id": row.get::<_, String>(2)?,
                "topic": row.get::<_, String>(3)?,
                "stance": row.get::<_, String>(4)?,
                "preferred_approach": row.get::<_, Option<String>>(5)?,
                "detail": row.get::<_, Option<String>>(6)?,
                "confidence": row.get::<_, String>(7)?,
                "valid_from": row.get::<_, String>(8)?,
            }))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;

    json_out(&json!({"stakeholder": stakeholder, "dispositions": dispositions}))
}

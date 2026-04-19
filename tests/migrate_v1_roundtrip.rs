//! End-to-end coverage for the v1 SQLite to v2 claim-store migrator.
//!
//! Builds a realistic v1 `.synth/main.db` fixture in a tempdir, drives
//! the migrator via the `synthesist migrate v1-to-v2` CLI subcommand
//! (assert_cmd subprocess), then opens the resulting claims/ directory
//! with `nomograph_claim::Store` to verify per-row correctness.
//!
//! Covers:
//!   - fixture insertion for every v1 table
//!   - dry-run does not materialize `to/`
//!   - real run produces the expected per-table claim counts
//!   - spot-check claim props (Task acceptance/depends_on, Spec goal +
//!     asserted_at timestamp, idempotence marker Discovery)
//!   - idempotence: second run without --overwrite fails; with
//!     --overwrite succeeds and produces the same counts
//!   - error path: missing source db fails cleanly

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use chrono::{DateTime, Utc};
use nomograph_claim::{ClaimType, Store};
use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

// -----------------------------------------------------------------------------
// Constants — match exactly what we insert so per-table counts can be asserted.
// -----------------------------------------------------------------------------

const N_TREES: usize = 2;
const N_SPECS: usize = 3;
const N_TASKS: usize = 5;
const N_DISCOVERIES: usize = 2;
const N_CAMPAIGNS: usize = 2; // 1 active + 1 backlog
const N_SESSIONS: usize = 2;
const N_STAKEHOLDERS: usize = 2;
const N_DISPOSITIONS: usize = 2;
const N_SIGNALS: usize = 3;
const N_PHASE: usize = 1;

const SPEC_GOAL_KEATON_ALPHA: &str = "ship the alpha migrator";
const SPEC_CREATED_KEATON_ALPHA: &str = "2026-02-01T12:00:00+00:00";

// -----------------------------------------------------------------------------
// Fixture builder
// -----------------------------------------------------------------------------

fn v1_schema() -> String {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/migrations/0001_initial.sql");
    std::fs::read_to_string(schema_path).expect("read v1 schema")
}

fn build_v1_fixture(db_path: &Path) {
    let conn = Connection::open(db_path).expect("open fixture db");
    conn.execute_batch(&v1_schema()).expect("apply v1 schema");

    // trees
    conn.execute_batch(
        "INSERT INTO trees (name, status, description) VALUES \
            ('keaton', 'active', 'main keaton tree'), \
            ('upstream', 'active', 'upstream contributions');",
    )
    .unwrap();

    // specs — one rich, one bare, one with outcome/completed status
    conn.execute(
        "INSERT INTO specs (tree, id, goal, constraints, decisions, status, outcome, created) \
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        rusqlite::params![
            "keaton",
            "alpha",
            SPEC_GOAL_KEATON_ALPHA,
            "no new deps",
            "use rusqlite",
            "active",
            "",
            SPEC_CREATED_KEATON_ALPHA,
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO specs (tree, id, goal, status, created) VALUES (?, ?, ?, ?, ?)",
        rusqlite::params![
            "keaton",
            "bare",
            "minimal goal",
            "active",
            "2026-02-02T12:00:00+00:00",
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO specs (tree, id, goal, status, outcome, created) VALUES (?, ?, ?, ?, ?, ?)",
        rusqlite::params![
            "upstream",
            "done-one",
            "finish upstream port",
            "done",
            "landed in 2.0",
            "2026-01-15T12:00:00+00:00",
        ],
    )
    .unwrap();

    // tasks — 5 total, every status once
    for (tree, spec, id, summary, status, created) in [
        ("keaton", "alpha", "t1", "pending task", "pending", "2026-02-01T13:00:00+00:00"),
        ("keaton", "alpha", "t2", "in flight", "in_progress", "2026-02-01T13:05:00+00:00"),
        ("keaton", "alpha", "t3", "done task", "done", "2026-02-01T13:10:00+00:00"),
        ("keaton", "bare", "t4", "blocked task", "blocked", "2026-02-02T13:00:00+00:00"),
        ("upstream", "done-one", "t5", "cancelled task", "cancelled", "2026-01-15T13:00:00+00:00"),
    ] {
        conn.execute(
            "INSERT INTO tasks (tree, spec, id, summary, description, status, created) \
                VALUES (?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![tree, spec, id, summary, "", status, created],
        )
        .unwrap();
    }

    // depends_on edges: t2 depends on t1; t3 depends on t1 and t2
    conn.execute_batch(
        "INSERT INTO task_deps (tree, spec, task_id, depends_on) VALUES \
            ('keaton', 'alpha', 't2', 't1'), \
            ('keaton', 'alpha', 't3', 't1'), \
            ('keaton', 'alpha', 't3', 't2');",
    )
    .unwrap();

    // acceptance — 3 criteria on t3
    conn.execute_batch(
        "INSERT INTO acceptance (tree, spec, task_id, seq, criterion, verify_cmd) VALUES \
            ('keaton', 'alpha', 't3', 1, 'compiles clean', 'cargo build'), \
            ('keaton', 'alpha', 't3', 2, 'tests pass', 'cargo test'), \
            ('keaton', 'alpha', 't3', 3, 'no warnings', 'cargo clippy');",
    )
    .unwrap();

    // discoveries — 2
    conn.execute_batch(
        "INSERT INTO discoveries (tree, spec, id, date, author, finding, impact, action) VALUES \
            ('keaton', 'alpha', 'd1', '2026-02-03T10:00:00+00:00', 'andunn', \
             'schema is stable', 'low', 'move on'), \
            ('upstream', 'done-one', 'd2', '2026-01-16T10:00:00+00:00', 'andunn', \
             'merged upstream', 'medium', 'announce');",
    )
    .unwrap();

    // campaigns
    conn.execute(
        "INSERT INTO campaign_active (tree, spec_id, summary, phase) VALUES (?, ?, ?, ?)",
        rusqlite::params!["keaton", "alpha", "in flight", "execute"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO campaign_backlog (tree, spec_id, title, summary) VALUES (?, ?, ?, ?)",
        rusqlite::params!["keaton", "bare", "Bare spec", "queued"],
    )
    .unwrap();
    conn.execute_batch(
        "INSERT INTO campaign_blocked_by (tree, spec_id, blocked_by) VALUES \
            ('keaton', 'alpha', 'upstream/done-one');",
    )
    .unwrap();

    // sessions — one merged, one active
    conn.execute_batch(
        "INSERT INTO session_meta (id, started, owner, tree, spec, summary, status) VALUES \
            ('s-merged', '2026-02-01T12:30:00+00:00', 'andunn', 'keaton', 'alpha', 'first', 'merged'), \
            ('s-active', '2026-02-04T08:00:00+00:00', 'andunn', 'keaton', 'bare', 'second', 'active');",
    )
    .unwrap();

    // stakeholders + orgs
    conn.execute_batch(
        "INSERT INTO stakeholders (tree, id, name, context) VALUES \
            ('keaton', 'alice', 'Alice', 'reviewer'), \
            ('keaton', 'bob', 'Bob', 'sponsor');",
    )
    .unwrap();
    conn.execute_batch(
        "INSERT INTO stakeholder_orgs (tree, stakeholder_id, org) VALUES \
            ('keaton', 'alice', 'nomograph'), \
            ('keaton', 'bob', 'gitlab');",
    )
    .unwrap();

    // dispositions — one superseded by the other (same spec)
    conn.execute_batch(
        "INSERT INTO dispositions (tree, spec, id, stakeholder_id, topic, stance, \
                preferred_approach, detail, confidence, valid_from, valid_until, superseded_by) VALUES \
            ('keaton', 'alpha', 'disp-old', 'alice', 'naming', 'oppose', 'snake_case', \
             'initial take', 'high', '2026-02-01T09:00:00+00:00', \
             '2026-02-03T09:00:00+00:00', 'disp-new'), \
            ('keaton', 'alpha', 'disp-new', 'alice', 'naming', 'support', 'kebab-case', \
             'revised', 'high', '2026-02-03T09:00:00+00:00', NULL, NULL);",
    )
    .unwrap();

    // signals — 3
    conn.execute_batch(
        "INSERT INTO signals (tree, spec, id, stakeholder_id, date, recorded_date, source, \
                source_type, content, interpretation, our_action) VALUES \
            ('keaton', 'alpha', 'sig1', 'alice', '2026-02-02T09:00:00+00:00', \
             '2026-02-02T10:00:00+00:00', 'slack', 'chat', 'looks good', 'approval', 'merge'), \
            ('keaton', 'alpha', 'sig2', 'bob', '2026-02-02T11:00:00+00:00', \
             '2026-02-02T11:30:00+00:00', 'email', 'msg', 'ship it', 'approval', 'merge'), \
            ('keaton', 'bare', 'sig3', 'alice', '2026-02-03T11:00:00+00:00', \
             '2026-02-03T11:30:00+00:00', 'mr-comment', 'gitlab', 'concerns', 'hold', 'revise');",
    )
    .unwrap();

    // phase — id=1, name=execute
    conn.execute("UPDATE phase SET name = 'execute' WHERE id = 1", [])
        .unwrap();
}

// -----------------------------------------------------------------------------
// CLI driver
// -----------------------------------------------------------------------------

fn migrate_cmd(from: &Path, to: &Path, dry_run: bool, overwrite: bool) -> Command {
    let mut cmd = Command::cargo_bin("synthesist").unwrap();
    cmd.args([
        "migrate",
        "v1-to-v2",
        "--from",
        from.to_str().unwrap(),
        "--to",
        to.to_str().unwrap(),
    ]);
    if dry_run {
        cmd.arg("--dry-run");
    }
    if overwrite {
        cmd.arg("--overwrite");
    }
    cmd.env("SYNTHESIST_OFFLINE", "1");
    cmd
}

/// Run the migrator, return stdout JSON and assert success.
fn run_migrate_ok(from: &Path, to: &Path, dry_run: bool, overwrite: bool) -> Value {
    let out = migrate_cmd(from, to, dry_run, overwrite)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).expect("utf8 stdout");
    // The subprocess emits one JSON line; some enhancements may prepend
    // progress lines. Find the last valid JSON object.
    let last_line = text
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON object in stdout: {text}"));
    serde_json::from_str(last_line).expect("parse json")
}

fn counts(json: &Value) -> &Value {
    json.get("counts").expect("counts object in output")
}

fn count_of(json: &Value, table: &str) -> u64 {
    counts(json)
        .get(table)
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| panic!("missing count for {table}: {json}"))
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

fn fixture(dir: &TempDir) -> PathBuf {
    let db = dir.path().join("main.db");
    build_v1_fixture(&db);
    db
}

#[test]
fn dry_run_does_not_materialize_output() {
    let tmp = TempDir::new().unwrap();
    let from = fixture(&tmp);
    let to = tmp.path().join("claims");

    let json = run_migrate_ok(&from, &to, true, false);

    // Summary reports every table even in dry-run.
    assert_eq!(count_of(&json, "trees"), N_TREES as u64);
    assert_eq!(count_of(&json, "specs"), N_SPECS as u64);
    assert_eq!(count_of(&json, "tasks"), N_TASKS as u64);
    assert_eq!(count_of(&json, "discoveries"), N_DISCOVERIES as u64);
    assert_eq!(count_of(&json, "campaigns"), N_CAMPAIGNS as u64);
    assert_eq!(count_of(&json, "sessions"), N_SESSIONS as u64);
    assert_eq!(count_of(&json, "stakeholders"), N_STAKEHOLDERS as u64);
    assert_eq!(count_of(&json, "dispositions"), N_DISPOSITIONS as u64);
    assert_eq!(count_of(&json, "signals"), N_SIGNALS as u64);
    assert_eq!(count_of(&json, "phase"), N_PHASE as u64);

    // `to` must not be created.
    assert!(!to.exists(), "dry-run must not create {to:?}");
}

#[test]
fn real_run_writes_claims_and_counts_match() {
    let tmp = TempDir::new().unwrap();
    let from = fixture(&tmp);
    let to = tmp.path().join("claims");

    let json = run_migrate_ok(&from, &to, false, false);

    // Per-table counts.
    assert_eq!(count_of(&json, "trees"), N_TREES as u64);
    assert_eq!(count_of(&json, "specs"), N_SPECS as u64);
    assert_eq!(count_of(&json, "tasks"), N_TASKS as u64);
    assert_eq!(count_of(&json, "discoveries"), N_DISCOVERIES as u64);
    assert_eq!(count_of(&json, "campaigns"), N_CAMPAIGNS as u64);
    assert_eq!(count_of(&json, "sessions"), N_SESSIONS as u64);
    assert_eq!(count_of(&json, "stakeholders"), N_STAKEHOLDERS as u64);
    assert_eq!(count_of(&json, "dispositions"), N_DISPOSITIONS as u64);
    assert_eq!(count_of(&json, "signals"), N_SIGNALS as u64);
    assert_eq!(count_of(&json, "phase"), N_PHASE as u64);

    // total_claims_appended matches sum (marker is appended on top).
    let total = json
        .get("total_claims_appended")
        .and_then(|v| v.as_u64())
        .expect("total_claims_appended");
    let expected: u64 = (N_TREES
        + N_SPECS
        + N_TASKS
        + N_DISCOVERIES
        + N_CAMPAIGNS
        + N_SESSIONS
        + N_STAKEHOLDERS
        + N_DISPOSITIONS
        + N_SIGNALS
        + N_PHASE) as u64;
    assert_eq!(total, expected);

    // Store filesystem shape.
    assert!(to.join("genesis.amc").exists(), "genesis.amc missing");
    let changes_dir = to.join("changes");
    assert!(changes_dir.exists(), "changes/ missing");
    let n_change_files = std::fs::read_dir(&changes_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .ok()
                .map(|de| de.path().extension().and_then(|s| s.to_str()) == Some("amc"))
                .unwrap_or(false)
        })
        .count();
    assert!(n_change_files > 0, "expected change files in {changes_dir:?}");
}

#[test]
fn claim_level_spot_checks() {
    let tmp = TempDir::new().unwrap();
    let from = fixture(&tmp);
    let to = tmp.path().join("claims");
    let _ = run_migrate_ok(&from, &to, false, false);

    let mut store = Store::open(&to).expect("open output store");
    let claims = store.load_claims().expect("load claims");

    let mut by_type: std::collections::HashMap<ClaimType, Vec<&nomograph_claim::Claim>> =
        Default::default();
    for c in &claims {
        by_type.entry(c.claim_type.clone()).or_default().push(c);
    }

    // Per-type counts include the migration marker as an extra Discovery.
    assert_eq!(by_type.get(&ClaimType::Tree).map(Vec::len), Some(N_TREES));
    assert_eq!(by_type.get(&ClaimType::Spec).map(Vec::len), Some(N_SPECS));
    assert_eq!(by_type.get(&ClaimType::Task).map(Vec::len), Some(N_TASKS));
    assert_eq!(
        by_type.get(&ClaimType::Discovery).map(Vec::len),
        Some(N_DISCOVERIES + 1), // +1 idempotence marker
    );
    assert_eq!(
        by_type.get(&ClaimType::Campaign).map(Vec::len),
        Some(N_CAMPAIGNS),
    );
    assert_eq!(
        by_type.get(&ClaimType::Session).map(Vec::len),
        Some(N_SESSIONS),
    );
    assert_eq!(
        by_type.get(&ClaimType::Stakeholder).map(Vec::len),
        Some(N_STAKEHOLDERS),
    );
    assert_eq!(
        by_type.get(&ClaimType::Disposition).map(Vec::len),
        Some(N_DISPOSITIONS),
    );
    assert_eq!(
        by_type.get(&ClaimType::Signal).map(Vec::len),
        Some(N_SIGNALS),
    );
    assert_eq!(by_type.get(&ClaimType::Phase).map(Vec::len), Some(N_PHASE));

    // Every inserted tree has a matching Tree claim by name.
    let tree_names: Vec<_> = by_type[&ClaimType::Tree]
        .iter()
        .filter_map(|c| c.props.get("name").and_then(|v| v.as_str()))
        .collect();
    assert!(tree_names.contains(&"keaton"));
    assert!(tree_names.contains(&"upstream"));

    // Task t3: acceptance len 3, depends_on len 2, status done.
    let t3 = by_type[&ClaimType::Task]
        .iter()
        .find(|c| c.props.get("id").and_then(|v| v.as_str()) == Some("t3"))
        .expect("t3 claim present");
    assert_eq!(
        t3.props.get("status").and_then(|v| v.as_str()),
        Some("done"),
    );
    assert_eq!(
        t3.props
            .get("acceptance")
            .and_then(|v| v.as_array())
            .map(Vec::len),
        Some(3),
    );
    assert_eq!(
        t3.props
            .get("depends_on")
            .and_then(|v| v.as_array())
            .map(Vec::len),
        Some(2),
    );

    // Task t1: no deps, no acceptance.
    let t1 = by_type[&ClaimType::Task]
        .iter()
        .find(|c| c.props.get("id").and_then(|v| v.as_str()) == Some("t1"))
        .expect("t1 claim present");
    assert_eq!(
        t1.props
            .get("depends_on")
            .and_then(|v| v.as_array())
            .map(Vec::len),
        Some(0),
    );

    // Spec keaton/alpha: goal preserved, asserted_at close to inserted value.
    let alpha_spec = by_type[&ClaimType::Spec]
        .iter()
        .find(|c| {
            c.props.get("tree").and_then(|v| v.as_str()) == Some("keaton")
                && c.props.get("id").and_then(|v| v.as_str()) == Some("alpha")
        })
        .expect("keaton/alpha spec");
    assert_eq!(
        alpha_spec.props.get("goal").and_then(|v| v.as_str()),
        Some(SPEC_GOAL_KEATON_ALPHA),
    );
    let expected_ts: DateTime<Utc> = DateTime::parse_from_rfc3339(SPEC_CREATED_KEATON_ALPHA)
        .unwrap()
        .with_timezone(&Utc);
    let delta = (alpha_spec.asserted_at - expected_ts).num_seconds().abs();
    assert!(
        delta <= 1,
        "asserted_at {} differs from inserted {} by {}s",
        alpha_spec.asserted_at,
        expected_ts,
        delta,
    );

    // Idempotence marker: a Discovery claim with tree=__migration__.
    let has_marker = by_type[&ClaimType::Discovery].iter().any(|c| {
        c.props.get("tree").and_then(|v| v.as_str()) == Some("__migration__")
    });
    assert!(has_marker, "migration marker Discovery missing");
}

#[test]
fn second_run_without_overwrite_fails_then_overwrite_succeeds() {
    let tmp = TempDir::new().unwrap();
    let from = fixture(&tmp);
    let to = tmp.path().join("claims");

    // First run: success.
    let _ = run_migrate_ok(&from, &to, false, false);

    // Second run, no overwrite: failure.
    migrate_cmd(&from, &to, false, false).assert().failure();

    // Third run, with overwrite: success and same per-table counts.
    let json = run_migrate_ok(&from, &to, false, true);
    assert_eq!(count_of(&json, "trees"), N_TREES as u64);
    assert_eq!(count_of(&json, "specs"), N_SPECS as u64);
    assert_eq!(count_of(&json, "tasks"), N_TASKS as u64);
    assert_eq!(count_of(&json, "discoveries"), N_DISCOVERIES as u64);
    assert_eq!(count_of(&json, "campaigns"), N_CAMPAIGNS as u64);
    assert_eq!(count_of(&json, "sessions"), N_SESSIONS as u64);
    assert_eq!(count_of(&json, "stakeholders"), N_STAKEHOLDERS as u64);
    assert_eq!(count_of(&json, "dispositions"), N_DISPOSITIONS as u64);
    assert_eq!(count_of(&json, "signals"), N_SIGNALS as u64);
    assert_eq!(count_of(&json, "phase"), N_PHASE as u64);
}

#[test]
fn missing_source_db_fails_cleanly() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("does-not-exist.db");
    let to = tmp.path().join("claims");

    migrate_cmd(&missing, &to, false, false).assert().failure();
    assert!(!to.exists(), "failed run should not materialize output");
}

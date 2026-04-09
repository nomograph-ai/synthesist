//! Integration tests for synthesist v1.
//!
//! Each test creates a temp directory, initializes synthesist, and exercises
//! the CLI. Tests run the compiled binary as a subprocess (like the Go golden
//! tests did) to validate the full stack.

use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::TempDir;

fn synth(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("synthesist").unwrap();
    cmd.current_dir(dir.path());
    cmd.env("SYNTHESIST_OFFLINE", "1");
    cmd
}

fn synth_session(dir: &TempDir, session: &str) -> Command {
    let mut cmd = synth(dir);
    cmd.arg("--session").arg(session).arg("--force");
    cmd
}

fn init(dir: &TempDir) {
    synth(dir).arg("init").assert().success();
}

#[test]
fn test_init() {
    let dir = TempDir::new().unwrap();
    synth(&dir)
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("initialized"));

    // Double init should fail
    synth(&dir)
        .arg("init")
        .assert()
        .failure()
        .stderr(predicate::str::contains("already initialized"));
}

#[test]
fn test_tree_add_list() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    synth_session(&dir, "test")
        .arg("tree")
        .arg("add")
        .arg("alpha")
        .arg("--description")
        .arg("First tree")
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"));

    synth(&dir)
        .arg("tree")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"));
}

#[test]
fn test_task_dag_flow() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    // Setup
    synth_session(&dir, "test")
        .args(["tree", "add", "test", "--description", "Test tree"])
        .assert()
        .success();

    synth_session(&dir, "test")
        .args(["task", "add", "test/spec", "First task"])
        .assert()
        .success()
        .stdout(predicate::str::contains("t1"));

    synth_session(&dir, "test")
        .args(["task", "add", "test/spec", "Second task", "--depends-on", "t1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("t2"));

    // Only t1 should be ready (t2 depends on t1)
    synth(&dir)
        .args(["task", "ready", "test/spec"])
        .assert()
        .success()
        .stdout(predicate::str::contains("t1"))
        .stdout(predicate::str::contains("t2").not());

    // Claim t1
    synth_session(&dir, "test")
        .args(["task", "claim", "test/spec", "t1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("in_progress"));

    // Complete t1
    synth_session(&dir, "test")
        .args(["task", "done", "test/spec", "t1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("done"));

    // Now t2 should be ready
    synth(&dir)
        .args(["task", "ready", "test/spec"])
        .assert()
        .success()
        .stdout(predicate::str::contains("t2"));
}

#[test]
fn test_task_claim_atomicity() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    synth_session(&dir, "test")
        .args(["tree", "add", "test", "--description", "Test"])
        .assert()
        .success();

    synth_session(&dir, "test")
        .args(["task", "add", "test/spec", "Contested task"])
        .assert()
        .success();

    // First claim succeeds
    synth_session(&dir, "agent-a")
        .args(["task", "claim", "test/spec", "t1"])
        .assert()
        .success();

    // Second claim fails (already owned)
    synth_session(&dir, "agent-b")
        .args(["task", "claim", "test/spec", "t1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already owned"));
}

#[test]
fn test_task_reset() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    synth_session(&dir, "test")
        .args(["tree", "add", "test", "--description", "Test"])
        .assert()
        .success();

    synth_session(&dir, "test")
        .args(["task", "add", "test/spec", "Task to reset"])
        .assert()
        .success();

    synth_session(&dir, "factory-01")
        .args(["task", "claim", "test/spec", "t1"])
        .assert()
        .success();

    // Reset by session
    synth_session(&dir, "admin")
        .args(["task", "reset", "--session", "factory-01", "--reason", "agent crashed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("reset_count"));
}

#[test]
fn test_phase_enforcement() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    // Default phase is orient -- no writes allowed
    synth(&dir)
        .args(["--session", "test", "tree", "add", "test", "--description", "Test"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("phase violation"));

    // With --force, writes succeed
    synth_session(&dir, "test")
        .args(["tree", "add", "test", "--description", "Test"])
        .assert()
        .success();
}

#[test]
fn test_session_lifecycle() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    // Start session
    synth(&dir)
        .args(["session", "start", "my-session", "--summary", "Test session"])
        .assert()
        .success()
        .stdout(predicate::str::contains("my-session"));

    // List sessions
    synth(&dir)
        .args(["session", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("my-session"));

    // Discard session
    synth(&dir)
        .args(["session", "discard", "my-session"])
        .assert()
        .success()
        .stdout(predicate::str::contains("discarded"));
}

#[test]
fn test_spec_crud() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    synth_session(&dir, "test")
        .args(["tree", "add", "test", "--description", "Test"])
        .assert()
        .success();

    synth_session(&dir, "test")
        .args(["spec", "add", "test/myspec", "--goal", "Build something"])
        .assert()
        .success()
        .stdout(predicate::str::contains("myspec"));

    synth(&dir)
        .args(["spec", "show", "test/myspec"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Build something"));

    synth(&dir)
        .args(["spec", "list", "test"])
        .assert()
        .success()
        .stdout(predicate::str::contains("myspec"));

    synth_session(&dir, "test")
        .args(["spec", "update", "test/myspec", "--status", "completed", "--outcome", "It worked"])
        .assert()
        .success();

    synth(&dir)
        .args(["spec", "show", "test/myspec"])
        .assert()
        .success()
        .stdout(predicate::str::contains("completed"))
        .stdout(predicate::str::contains("It worked"));
}

#[test]
fn test_discovery() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    synth_session(&dir, "test")
        .args(["tree", "add", "test", "--description", "Test"])
        .assert()
        .success();

    synth_session(&dir, "test")
        .args([
            "discovery",
            "add",
            "test/spec",
            "--finding",
            "SQLite is faster than expected",
            "--impact",
            "high",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("d1"));

    synth(&dir)
        .args(["discovery", "list", "test/spec"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SQLite is faster"));
}

#[test]
fn test_disposition_graph() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    synth_session(&dir, "test")
        .args(["tree", "add", "upstream", "--description", "Upstream project"])
        .assert()
        .success();

    // Add stakeholder
    synth_session(&dir, "test")
        .args([
            "stakeholder",
            "add",
            "upstream",
            "mwilson",
            "--context",
            "lead maintainer",
            "--name",
            "M. Wilson",
        ])
        .assert()
        .success();

    // Add disposition
    synth_session(&dir, "test")
        .args([
            "disposition",
            "add",
            "upstream/core",
            "mwilson",
            "--topic",
            "API versioning",
            "--stance",
            "opposed",
            "--confidence",
            "documented",
            "--preferred",
            "incremental migration",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("disp1"));

    // Query stance
    synth(&dir)
        .args(["stance", "mwilson"])
        .assert()
        .success()
        .stdout(predicate::str::contains("opposed"))
        .stdout(predicate::str::contains("API versioning"));

    // Supersede disposition
    synth_session(&dir, "test")
        .args([
            "disposition",
            "supersede",
            "upstream/core",
            "disp1",
            "--stance",
            "cautious",
            "--confidence",
            "verified",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("disp2"));

    // Stance should show new disposition
    synth(&dir)
        .args(["stance", "mwilson"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cautious"));
}

#[test]
fn test_export_import_roundtrip() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    synth_session(&dir, "test")
        .args(["tree", "add", "test", "--description", "Export test"])
        .assert()
        .success();

    synth_session(&dir, "test")
        .args(["task", "add", "test/spec", "A task"])
        .assert()
        .success();

    // Export
    let export_output = synth(&dir)
        .args(["export"])
        .output()
        .unwrap();
    assert!(export_output.status.success());
    let export_json = String::from_utf8(export_output.stdout).unwrap();

    // Write to file
    let export_file = dir.path().join("export.json");
    std::fs::write(&export_file, &export_json).unwrap();

    // Import into fresh database
    let dir2 = TempDir::new().unwrap();
    init(&dir2);

    synth_session(&dir2, "test")
        .args(["import", export_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete"));

    // Verify data exists
    synth(&dir2)
        .args(["task", "list", "test/spec"])
        .assert()
        .success()
        .stdout(predicate::str::contains("A task"));
}

#[test]
fn test_sql_query() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    synth_session(&dir, "test")
        .args(["tree", "add", "test", "--description", "SQL test"])
        .assert()
        .success();

    synth(&dir)
        .args(["sql", "SELECT name, description FROM trees"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test"))
        .stdout(predicate::str::contains("SQL test"));
}

#[test]
fn test_version() {
    let dir = TempDir::new().unwrap();
    // Version doesn't need init
    synth(&dir)
        .args(["version", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("version"));
}

#[test]
fn test_skill_output() {
    let dir = TempDir::new().unwrap();
    synth(&dir)
        .args(["skill"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Specification Graph Manager"))
        .stdout(predicate::str::contains("ORIENT"))
        .stdout(predicate::str::contains("AGREE"))
        .stdout(predicate::str::contains("task claim"));
}

// ---------------------------------------------------------------------------
// Session isolation and merge tests
// ---------------------------------------------------------------------------

/// Helper: start a real session (creates the session .db file).
fn session_start(dir: &TempDir, id: &str) {
    synth(dir)
        .args(["session", "start", id, "--summary", &format!("Session {id}")])
        .assert()
        .success()
        .stdout(predicate::str::contains(id));
}

/// Helper: write to a started session (session .db must already exist).
fn synth_started_session(dir: &TempDir, session: &str) -> Command {
    let mut cmd = synth(dir);
    cmd.arg("--session").arg(session).arg("--force");
    cmd
}

#[test]
fn test_session_isolation() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    // Start a real session -- creates sessions/iso.db
    session_start(&dir, "iso");

    // Write data into the session
    synth_started_session(&dir, "iso")
        .args(["tree", "add", "isolated", "--description", "Isolated tree"])
        .assert()
        .success()
        .stdout(predicate::str::contains("isolated"));

    synth_started_session(&dir, "iso")
        .args(["task", "add", "isolated/spec", "Do the thing"])
        .assert()
        .success()
        .stdout(predicate::str::contains("t1"));

    // Query main.db (no --session) -- should NOT see the isolated tree
    synth(&dir)
        .args(["tree", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("isolated").not());

    // Query main.db task list -- should fail or return empty (tree doesn't exist in main)
    synth(&dir)
        .args(["task", "list", "isolated/spec"])
        .assert()
        .success()
        .stdout(predicate::str::contains("t1").not());

    // Session status should show changes
    synth(&dir)
        .args(["session", "status", "iso"])
        .assert()
        .success()
        .stdout(predicate::str::contains("trees"))
        .stdout(predicate::str::contains("added"));

    // Merge the session
    synth(&dir)
        .args(["session", "merge", "iso"])
        .assert()
        .success()
        .stdout(predicate::str::contains("merged"));

    // NOW main.db should have the data
    synth(&dir)
        .args(["tree", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("isolated"));

    synth(&dir)
        .args(["task", "list", "isolated/spec"])
        .assert()
        .success()
        .stdout(predicate::str::contains("t1"))
        .stdout(predicate::str::contains("Do the thing"));
}

#[test]
fn test_session_merge_with_changes() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    session_start(&dir, "merge-test");

    // Add tree and tasks in the session
    synth_started_session(&dir, "merge-test")
        .args(["tree", "add", "proj", "--description", "Merge project"])
        .assert()
        .success();

    synth_started_session(&dir, "merge-test")
        .args(["task", "add", "proj/spec", "First task"])
        .assert()
        .success()
        .stdout(predicate::str::contains("t1"));

    synth_started_session(&dir, "merge-test")
        .args(["task", "add", "proj/spec", "Second task"])
        .assert()
        .success()
        .stdout(predicate::str::contains("t2"));

    // Main should be empty before merge
    synth(&dir)
        .args(["tree", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("proj").not());

    // Merge
    synth(&dir)
        .args(["session", "merge", "merge-test"])
        .assert()
        .success()
        .stdout(predicate::str::contains("merged"));

    // Verify all data appeared in main
    synth(&dir)
        .args(["tree", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("proj"))
        .stdout(predicate::str::contains("Merge project"));

    synth(&dir)
        .args(["task", "list", "proj/spec"])
        .assert()
        .success()
        .stdout(predicate::str::contains("t1"))
        .stdout(predicate::str::contains("First task"))
        .stdout(predicate::str::contains("t2"))
        .stdout(predicate::str::contains("Second task"));
}

#[test]
fn test_session_merge_dry_run() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    session_start(&dir, "dry");

    // Make changes in the session
    synth_started_session(&dir, "dry")
        .args(["tree", "add", "drytest", "--description", "Dry run tree"])
        .assert()
        .success();

    synth_started_session(&dir, "dry")
        .args(["task", "add", "drytest/spec", "Dry task"])
        .assert()
        .success();

    // Dry-run merge -- should report changes but not apply
    synth(&dir)
        .args(["session", "merge", "dry", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("dry_run"))
        .stdout(predicate::str::contains("trees"))
        .stdout(predicate::str::contains("added"));

    // Main should still be unchanged after dry-run
    synth(&dir)
        .args(["tree", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("drytest").not());

    // Session should still exist (not deleted by dry-run)
    synth(&dir)
        .args(["session", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("dry"));

    // Now do actual merge
    synth(&dir)
        .args(["session", "merge", "dry"])
        .assert()
        .success()
        .stdout(predicate::str::contains("merged"));

    // Now main should have the data
    synth(&dir)
        .args(["tree", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("drytest"));
}

#[test]
fn test_concurrent_sessions() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    // Start two concurrent sessions
    session_start(&dir, "alpha");
    session_start(&dir, "beta");

    // Alpha adds its own tree and task
    synth_started_session(&dir, "alpha")
        .args(["tree", "add", "a-proj", "--description", "Alpha project"])
        .assert()
        .success();

    synth_started_session(&dir, "alpha")
        .args(["task", "add", "a-proj/spec", "Alpha task"])
        .assert()
        .success();

    // Beta adds different tree and task
    synth_started_session(&dir, "beta")
        .args(["tree", "add", "b-proj", "--description", "Beta project"])
        .assert()
        .success();

    synth_started_session(&dir, "beta")
        .args(["task", "add", "b-proj/spec", "Beta task"])
        .assert()
        .success();

    // Neither should be visible in main yet
    synth(&dir)
        .args(["tree", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("a-proj").not())
        .stdout(predicate::str::contains("b-proj").not());

    // Merge alpha first
    synth(&dir)
        .args(["session", "merge", "alpha"])
        .assert()
        .success()
        .stdout(predicate::str::contains("merged"));

    // Alpha data visible, beta not yet
    synth(&dir)
        .args(["tree", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("a-proj"))
        .stdout(predicate::str::contains("b-proj").not());

    // Merge beta
    synth(&dir)
        .args(["session", "merge", "beta"])
        .assert()
        .success()
        .stdout(predicate::str::contains("merged"));

    // Both should be present now
    synth(&dir)
        .args(["tree", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("a-proj"))
        .stdout(predicate::str::contains("b-proj"));

    synth(&dir)
        .args(["task", "list", "a-proj/spec"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Alpha task"));

    synth(&dir)
        .args(["task", "list", "b-proj/spec"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Beta task"));
}

#[test]
fn test_session_discard_cleanup() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    session_start(&dir, "throwaway");

    // Add data in the session
    synth_started_session(&dir, "throwaway")
        .args(["tree", "add", "discard-me", "--description", "Will be discarded"])
        .assert()
        .success();

    synth_started_session(&dir, "throwaway")
        .args(["task", "add", "discard-me/spec", "Ephemeral task"])
        .assert()
        .success();

    // Session file should exist
    let session_file = dir.path().join("synthesist").join("sessions").join("throwaway.db");
    assert!(session_file.exists(), "session .db file should exist before discard");

    // Discard the session
    synth(&dir)
        .args(["session", "discard", "throwaway"])
        .assert()
        .success()
        .stdout(predicate::str::contains("discarded"));

    // Session file should be gone
    assert!(!session_file.exists(), "session .db file should be deleted after discard");

    // Main should be unchanged (no data leaked)
    synth(&dir)
        .args(["tree", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("discard-me").not());

    synth(&dir)
        .args(["task", "list", "discard-me/spec"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Ephemeral task").not());

    // Session list should show discarded status
    synth(&dir)
        .args(["session", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("discarded"));
}

// ---------------------------------------------------------------------------
// Security and robustness tests from adversarial review
// ---------------------------------------------------------------------------

#[test]
fn test_sql_read_only_enforcement() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    // SELECT should work
    synth(&dir)
        .args(["sql", "SELECT COUNT(*) as cnt FROM trees"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cnt"));

    // WITH (CTE) should work
    synth(&dir)
        .args(["sql", "WITH t AS (SELECT 1 as n) SELECT * FROM t"])
        .assert()
        .success();

    // DELETE should be rejected
    synth(&dir)
        .args(["sql", "DELETE FROM trees"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("read-only"));

    // DROP should be rejected
    synth(&dir)
        .args(["sql", "DROP TABLE tasks"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("read-only"));

    // UPDATE should be rejected
    synth(&dir)
        .args(["sql", "UPDATE phase SET name='execute'"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("read-only"));

    // INSERT should be rejected
    synth(&dir)
        .args(["sql", "INSERT INTO trees VALUES ('hack','active','')"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("read-only"));
}

#[test]
fn test_session_id_path_traversal() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    // Path traversal in session ID should be rejected
    synth(&dir)
        .args(["session", "start", "../../main"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid session ID"));

    synth(&dir)
        .args(["session", "start", "foo/bar"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid session ID"));
}

#[test]
fn test_cancel_done_task_rejected() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    synth_session(&dir, "test")
        .args(["tree", "add", "test", "--description", "Test"])
        .assert()
        .success();

    synth_session(&dir, "test")
        .args(["task", "add", "test/spec", "Task to complete"])
        .assert()
        .success();

    synth_session(&dir, "test")
        .args(["task", "claim", "test/spec", "t1"])
        .assert()
        .success();

    synth_session(&dir, "test")
        .args(["task", "done", "test/spec", "t1"])
        .assert()
        .success();

    // Cancelling a done task should fail
    synth_session(&dir, "test")
        .args(["task", "cancel", "test/spec", "t1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot cancel"));
}

#[test]
fn test_phase_transitions() {
    let dir = TempDir::new().unwrap();
    init(&dir);

    // Default phase is orient. orient -> plan should succeed.
    synth(&dir)
        .args(["phase", "set", "plan"])
        .assert()
        .success()
        .stdout(predicate::str::contains("plan"));

    // plan -> execute should fail (must go through agree).
    synth(&dir)
        .args(["phase", "set", "execute"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid phase transition"))
        .stderr(predicate::str::contains("agree"));

    // plan -> agree should succeed.
    synth(&dir)
        .args(["phase", "set", "agree"])
        .assert()
        .success()
        .stdout(predicate::str::contains("agree"));

    // With --force, any transition works (agree -> reflect is not normally valid).
    synth(&dir)
        .args(["--force", "phase", "set", "reflect"])
        .assert()
        .success()
        .stdout(predicate::str::contains("reflect"));
}

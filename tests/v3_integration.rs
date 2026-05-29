//! T3.6: End-to-end CLI integration test on v3 substrate.
//!
//! Exercises the synthesist CLI on the dual-write v3 substrate. Every
//! write command triggers a v2 .amc append AND a v3 JSON-LD dual-write.
//! After the happy-path scenario this test verifies:
//!
//!  1. v2 `claims/changes/<hash>.amc` files exist.
//!  2. v3 `claims/<asserter-dir>/log.jsonl` exists with the correct
//!     line count.
//!  3. One v3 line round-trips as valid JSON with the expected envelope.
//!  4. `nomograph_claim::graph_view::rebuild` reports
//!     `claims_loaded == expected_count`.
//!
//! Uses `assert_cmd` to drive the real binary so tests run the actual
//! binary rather than library code. A tempdir is used for SYNTHESIST_DIR.

use std::fs;
use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn synth(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("synthesist").unwrap();
    cmd.current_dir(dir.path());
    cmd.env("SYNTHESIST_OFFLINE", "1");
    cmd.env_remove("SYNTHESIST_DIR");
    cmd.env_remove("SYNTHESIST_SESSION");
    // Fix USER so the asserter directory name is deterministic across
    // machines.
    cmd.env("USER", "t3test");
    cmd
}

/// Return a Command with --session prepended before the subcommand.
fn synth_s(dir: &TempDir, session: &str) -> Command {
    let mut cmd = synth(dir);
    cmd.args(["--session", session]);
    cmd
}

/// Count non-empty lines in a file.
fn count_lines(path: &std::path::Path) -> usize {
    let text = fs::read_to_string(path).unwrap_or_default();
    text.lines().filter(|l| !l.trim().is_empty()).count()
}

/// The asserter directory name for `user:local:t3test:<session>`.
/// LogWriter maps colons to hyphens.
fn asserter_dir(session: &str) -> String {
    format!("user-local-t3test-{session}")
}

/// Assert v2 .amc files exist under claims/changes/.
fn assert_v2_amc_exists(dir: &TempDir) {
    let changes = dir.path().join("claims").join("changes");
    assert!(changes.is_dir(), "claims/changes/ must exist after writes");
    let amc_count = fs::read_dir(&changes)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "amc").unwrap_or(false))
        .count();
    assert!(amc_count > 0, "at least one .amc file must exist in claims/changes/");
}

/// Assert v3 log exists with expected line count; return the first line.
fn assert_v3_log(dir: &TempDir, session: &str, expected_lines: usize) -> String {
    let log_path = dir
        .path()
        .join("claims")
        .join(asserter_dir(session))
        .join("log.jsonl");
    assert!(
        log_path.exists(),
        "v3 log must exist at {}",
        log_path.display()
    );
    let actual = count_lines(&log_path);
    assert_eq!(
        actual, expected_lines,
        "v3 log line count: expected {expected_lines}, got {actual}"
    );
    fs::read_to_string(&log_path)
        .unwrap()
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap()
        .to_string()
}

/// Spot-check a v3 log line for the required JSON-LD envelope predicates.
fn assert_jsonld_envelope(line: &str, session: &str) {
    let doc: serde_json::Value =
        serde_json::from_str(line).expect("v3 log line must be valid JSON");

    let at_id = doc["@id"].as_str().expect("@id must be present");
    assert!(
        at_id.starts_with("synth:claim/"),
        "@id must start with synth:claim/, got: {at_id}"
    );

    let gen_time = doc["prov:generatedAtTime"]
        .as_str()
        .expect("prov:generatedAtTime must be present");
    assert!(
        gen_time.ends_with('Z') && gen_time.contains('T'),
        "generatedAtTime must be ISO-8601 with Z suffix"
    );

    let attributed = doc["prov:wasAttributedTo"]
        .as_str()
        .expect("prov:wasAttributedTo must be present");
    let expected = format!("asserter:user:local:t3test:{session}");
    assert_eq!(attributed, &expected, "prov:wasAttributedTo mismatch");
}

/// Run a GraphView in-memory rebuild; assert claims_loaded == expected.
fn assert_graph_view_rebuild(dir: &TempDir, expected_claims: usize) {
    use nomograph_claim::graph_view::{GraphView, rebuild};

    let claims_dir = dir.path().join("claims");
    let view = GraphView::open_in_memory().expect("open in-memory GraphView");
    let stats = rebuild(&view, &claims_dir).expect("GraphView rebuild must succeed");
    assert_eq!(
        stats.claims_loaded, expected_claims,
        "GraphView rebuild: expected {expected_claims} claims_loaded, got {}",
        stats.claims_loaded
    );
    assert_eq!(stats.parse_failures, 0, "GraphView rebuild must have 0 parse failures");
}

// ---------------------------------------------------------------------------
// T3.6 happy-path scenario
//
// Commands exercised (the v2.5 subset from the task block):
//   init, session start, tree add, spec add, task add, task claim,
//   task done, task ready, status, phase set, discovery add, session close.
// ---------------------------------------------------------------------------

#[test]
fn v3_happy_path_dual_write() {
    let tmp = tempfile::tempdir().unwrap();
    let session = "t36-happy";

    // 1. init
    synth(&tmp)
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\":true"));

    // 2. session start
    synth(&tmp)
        .args(["session", "start", session])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\":\"t36-happy\""));

    // 3. phase set plan (required before tree/spec/task writes)
    synth_s(&tmp, session)
        .args(["phase", "set", "plan"])
        .assert()
        .success();

    // 4. tree add
    synth_s(&tmp, session)
        .args(["tree", "add", "alpha", "--description", "alpha tree"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\":\"alpha\""));

    // 5. spec add
    synth_s(&tmp, session)
        .args(["spec", "add", "alpha/graphs", "--goal", "build graph"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\":\"graphs\""));

    // 6. task add (two tasks so we can verify the ready list)
    synth_s(&tmp, session)
        .args(["task", "add", "alpha/graphs", "write the reader", "--id", "t1"])
        .assert()
        .success();

    synth_s(&tmp, session)
        .args(["task", "add", "alpha/graphs", "write the writer", "--id", "t2"])
        .assert()
        .success();

    // 7. phase set agree + execute (required before task claim/done)
    synth_s(&tmp, session)
        .args(["phase", "set", "agree"])
        .assert()
        .success();

    synth_s(&tmp, session)
        .args(["phase", "set", "execute"])
        .assert()
        .success();

    // 8. task claim (t1 -> in_progress)
    synth_s(&tmp, session)
        .args(["task", "claim", "alpha/graphs", "t1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\":\"in_progress\""));

    // 9. task done (t1 -> done)
    synth_s(&tmp, session)
        .args(["task", "done", "alpha/graphs", "t1", "--skip-verify"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\":\"done\""));

    // 10. task ready (t2 should be ready; no blocking deps)
    let ready_out = synth_s(&tmp, session)
        .args(["task", "ready", "alpha/graphs"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let ready_str = String::from_utf8(ready_out).unwrap();
    assert!(
        ready_str.contains("\"id\":\"t2\""),
        "task ready must list t2: {ready_str}"
    );

    // 11. status
    synth(&tmp)
        .args(["status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total_claims\""));

    // 12. discovery add
    synth_s(&tmp, session)
        .args([
            "discovery", "add", "alpha/graphs",
            "--finding", "json-ld encoding works end to end",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"finding\":"));

    // 13. session close
    synth(&tmp)
        .args(["session", "close", session])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"closed\":true"));

    // ------------------------------------------------------------------
    // v3 dual-write verification
    //
    // Only commands that call SynthStore::append WITH a session-scoped
    // asserter (via discover_for) produce v3 log lines.
    // `phase set` uses SynthStore::discover() (no session) so Phase
    // claims do NOT dual-write. `session start` / `session close` go
    // through nomograph_workflow::Session and do not route through
    // SynthStore::append. `task ready` and `status` are read-only.
    //
    // Session-scoped dual-write claims:
    //   tree add         -> 1 Tree claim
    //   spec add         -> 1 Spec claim
    //   task add t1      -> 1 Task claim
    //   task add t2      -> 1 Task claim
    //   task claim t1    -> 1 Task claim (supersession)
    //   task done t1     -> 1 Task claim (supersession)
    //   discovery add    -> 1 Discovery claim
    //
    // Total: 7 v3 claims.
    // ------------------------------------------------------------------
    const EXPECTED_V3_CLAIMS: usize = 7;

    // a) v2 .amc files exist.
    assert_v2_amc_exists(&tmp);

    // b) v3 log exists with the correct line count.
    let first_line = assert_v3_log(&tmp, session, EXPECTED_V3_CLAIMS);

    // c) Spot-check JSON-LD envelope on the first line.
    assert_jsonld_envelope(&first_line, session);

    // d) GraphView rebuild confirms claims_loaded == EXPECTED_V3_CLAIMS.
    assert_graph_view_rebuild(&tmp, EXPECTED_V3_CLAIMS);
}

// ---------------------------------------------------------------------------
// Exit-code + JSON-shape contract (byte-identical to v2.5)
// ---------------------------------------------------------------------------

#[test]
fn v3_init_exit_code_and_json_shape() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp)
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\":true"))
        .stdout(predicate::str::contains("\"root\":"));
}

#[test]
fn v3_write_without_session_exits_nonzero() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).args(["init"]).assert().success();
    synth(&tmp)
        .args(["tree", "add", "beta", "--description", "x"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("session required"));
}

#[test]
fn v3_phase_transition_invalid_exit_nonzero() {
    let tmp = tempfile::tempdir().unwrap();
    let session = "t36-phase";
    synth(&tmp).args(["init"]).assert().success();
    synth(&tmp).args(["session", "start", session]).assert().success();
    synth_s(&tmp, session).args(["phase", "set", "plan"]).assert().success();
    synth_s(&tmp, session).args(["phase", "set", "agree"]).assert().success();
    synth_s(&tmp, session).args(["phase", "set", "execute"]).assert().success();
    // execute -> plan is invalid without --force.
    synth_s(&tmp, session)
        .args(["phase", "set", "plan"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid phase transition"));
}

#[test]
fn v3_session_close_hides_from_list() {
    let tmp = tempfile::tempdir().unwrap();
    let session = "t36-close";
    synth(&tmp).args(["init"]).assert().success();
    synth(&tmp).args(["session", "start", session]).assert().success();
    synth(&tmp)
        .args(["session", "close", session])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"closed\":true"));
    let listed = synth(&tmp)
        .args(["session", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(listed).unwrap();
    assert!(out.contains("\"sessions\":[]"), "after close: {out}");
}

// ---------------------------------------------------------------------------
// v3 log contains the correct @type for each claim type.
// ---------------------------------------------------------------------------

#[test]
fn v3_log_line_contains_correct_type_for_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let session = "t36-tree-type";
    synth(&tmp).args(["init"]).assert().success();
    synth(&tmp).args(["session", "start", session]).assert().success();
    synth_s(&tmp, session).args(["phase", "set", "plan"]).assert().success();
    synth_s(&tmp, session)
        .args(["tree", "add", "gamma", "--description", "test"])
        .assert()
        .success();

    // `phase set plan` uses SynthStore::discover() (no session-scoped asserter)
    // so it does NOT dual-write. Only `tree add` (via discover_for) produces
    // a v3 log line.
    let log_path = tmp
        .path()
        .join("claims")
        .join(asserter_dir(session))
        .join("log.jsonl");
    let text = fs::read_to_string(&log_path)
        .expect("v3 log must exist after tree add");
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(), 1,
        "expected 1 v3 log line (tree only; phase set uses sessionless discover path)"
    );

    let doc: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(doc["@type"].as_str().unwrap(), "synth:Tree");
    assert_eq!(
        doc["synth:name"].as_str().unwrap(),
        "gamma",
        "synth:name prop must propagate to v3 log"
    );
}

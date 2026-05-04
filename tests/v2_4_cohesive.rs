//! Coverage for the cohesive v2.4.0 architectural changes:
//! - Issue #6 fix: clap rejects out-of-enum spec status at parse time
//!   with a clear message naming the schema-permitted set, and the
//!   structured SchemaError chain reaches the user.
//! - Issue #5 fix: `task update --depends-on` with replacement
//!   semantics, self-dep / unknown-id / cycle rejection, cancelled-dep
//!   surfaced as a JSON warning rather than stderr eprintln.
//! - Outcome claim surface: `outcome add` and `outcome list` cover the
//!   workflow that was previously inaccessible from the CLI.
//! - Schema-CLI parity: every value clap accepts for `spec update
//!   --status` round-trips through the validator (and vice versa).
//! - claims compact: `--dry-run` reports without writing; `--yes`
//!   skips the prompt; logical state survives compaction.

use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::TempDir;

fn synth(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("synthesist").unwrap();
    cmd.current_dir(dir.path());
    cmd.env("SYNTHESIST_OFFLINE", "1");
    cmd.env_remove("SYNTHESIST_DIR");
    cmd.env_remove("SYNTHESIST_SESSION");
    cmd
}

fn bootstrap(tmp: &TempDir) {
    synth(tmp).args(["init"]).assert().success();
    synth(tmp)
        .args(["session", "start", "s1"])
        .assert()
        .success();
    synth(tmp)
        .args(["--session", "s1", "--force", "phase", "set", "plan"])
        .assert()
        .success();
    synth(tmp)
        .args(["--session", "s1", "tree", "add", "k", "--description", "k"])
        .assert()
        .success();
    synth(tmp)
        .args([
            "--session",
            "s1",
            "spec",
            "add",
            "k/sample",
            "--goal",
            "g",
        ])
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// Issue #6: spec update --status enum rejection at clap-parse time
// ---------------------------------------------------------------------------

#[test]
fn spec_update_status_completed_rejected_at_parse() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "spec",
            "update",
            "k/sample",
            "--status",
            "completed",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value 'completed'"));
}

#[test]
fn spec_update_status_lists_valid_values() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    let out = synth(&tmp)
        .args([
            "--session",
            "s1",
            "spec",
            "update",
            "k/sample",
            "--status",
            "completed",
        ])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(out).unwrap();
    // The clap PossibleValuesParser surfaces each schema-permitted
    // value in its error; verifying all four reach the user.
    for s in ["draft", "active", "done", "superseded"] {
        assert!(
            stderr.contains(s),
            "stderr should list {s}; got: {stderr}"
        );
    }
}

#[test]
fn spec_update_status_done_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "spec",
            "update",
            "k/sample",
            "--status",
            "done",
        ])
        .assert()
        .success();
}

#[test]
fn spec_update_each_schema_value_accepted_at_cli() {
    // Schema-CLI parity: every value the schema accepts for Spec
    // status must also pass clap's PossibleValuesParser, since they
    // reference the same const. Exercises that parity by walking the
    // four schema values through clap.
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    for s in ["draft", "active", "done", "superseded"] {
        synth(&tmp)
            .args(["--session", "s1", "spec", "update", "k/sample", "--status", s])
            .assert()
            .success();
    }
}

// ---------------------------------------------------------------------------
// Issue #5: task update --depends-on
// ---------------------------------------------------------------------------

fn add_three_tasks(tmp: &TempDir) {
    for id in ["t1", "t2", "t3"] {
        synth(tmp)
            .args([
                "--session",
                "s1",
                "task",
                "add",
                "k/sample",
                "summary",
                "--id",
                id,
            ])
            .assert()
            .success();
    }
}

#[test]
fn task_update_depends_on_replaces_dep_list() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    add_three_tasks(&tmp);
    let out = synth(&tmp)
        .args([
            "--session",
            "s1",
            "task",
            "update",
            "k/sample",
            "t3",
            "--depends-on",
            "t1,t2",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body = String::from_utf8(out).unwrap();
    assert!(body.contains("\"depends_on\":[\"t1\",\"t2\"]"), "{body}");
}

#[test]
fn task_update_depends_on_self_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    add_three_tasks(&tmp);
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "task",
            "update",
            "k/sample",
            "t3",
            "--depends-on",
            "t3",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot depend on self"));
}

#[test]
fn task_update_depends_on_unknown_id_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    add_three_tasks(&tmp);
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "task",
            "update",
            "k/sample",
            "t3",
            "--depends-on",
            "t99",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn task_update_depends_on_cycle_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    add_three_tasks(&tmp);
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "task",
            "update",
            "k/sample",
            "t3",
            "--depends-on",
            "t2",
        ])
        .assert()
        .success();
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "task",
            "update",
            "k/sample",
            "t2",
            "--depends-on",
            "t1",
        ])
        .assert()
        .success();
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "task",
            "update",
            "k/sample",
            "t1",
            "--depends-on",
            "t3",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cycle"));
}

// ---------------------------------------------------------------------------
// Outcome surface
// ---------------------------------------------------------------------------

#[test]
fn outcome_add_with_completed_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    let out = synth(&tmp)
        .args([
            "--session",
            "s1",
            "outcome",
            "add",
            "k/sample",
            "--status",
            "completed",
            "--note",
            "shipped",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body = String::from_utf8(out).unwrap();
    assert!(body.contains("\"status\":\"completed\""), "{body}");
    assert!(body.contains("\"note\":\"shipped\""), "{body}");
}

#[test]
fn outcome_add_unknown_status_rejected_at_parse() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "outcome",
            "add",
            "k/sample",
            "--status",
            "shipped",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value 'shipped'"));
}

#[test]
fn outcome_superseded_by_requires_linked_spec() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "outcome",
            "add",
            "k/sample",
            "--status",
            "superseded_by",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("linked_spec"));
}

#[test]
fn outcome_list_returns_recorded_outcomes() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "outcome",
            "add",
            "k/sample",
            "--status",
            "abandoned",
            "--note",
            "scope folded",
        ])
        .assert()
        .success();
    let out = synth(&tmp)
        .args(["--session", "s1", "outcome", "list", "k/sample"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body = String::from_utf8(out).unwrap();
    assert!(body.contains("\"abandoned\""), "{body}");
    assert!(body.contains("\"scope folded\""), "{body}");
}

// ---------------------------------------------------------------------------
// claims compact safety belts
// ---------------------------------------------------------------------------

#[test]
fn claims_compact_dry_run_makes_no_changes() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    add_three_tasks(&tmp);
    let out = synth(&tmp)
        .args(["--session", "s1", "--force", "claims", "compact", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let body = String::from_utf8(out).unwrap();
    assert!(body.contains("\"dry_run\":true"), "{body}");
    // After a dry-run, regular reads still see all the tasks we
    // just wrote — no compaction occurred.
    let listed = synth(&tmp)
        .args(["--session", "s1", "task", "list", "k/sample"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let listed = String::from_utf8(listed).unwrap();
    for id in ["t1", "t2", "t3"] {
        assert!(listed.contains(&format!("\"id\":\"{id}\"")), "{listed}");
    }
}

#[test]
fn claims_compact_yes_skips_prompt() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    add_three_tasks(&tmp);
    synth(&tmp)
        .args(["--session", "s1", "--force", "claims", "compact", "--yes"])
        .assert()
        .success();
    // After compaction, all three tasks are still readable — logical
    // history is preserved.
    let listed = synth(&tmp)
        .args(["--session", "s1", "task", "list", "k/sample"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let listed = String::from_utf8(listed).unwrap();
    for id in ["t1", "t2", "t3"] {
        assert!(listed.contains(&format!("\"id\":\"{id}\"")), "{listed}");
    }
}

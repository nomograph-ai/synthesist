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
    // v2.5.0: clap enforces this at parse time via required_if_eq,
    // so the rejection lands before the schema validator runs. The
    // schema-level coupling still exists as a backstop.
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
        .stderr(predicate::str::contains("--linked-spec"));
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

// ---------------------------------------------------------------------------
// v2.5.0: shape conformity pass.
// - claims compact, outcome list, tree show: sessionless and phase-free.
// - phase set / phase show: per-session, no global fallback.
// - status: top-level `phase` removed, per-session phase in sessions[].
// ---------------------------------------------------------------------------

#[test]
fn claims_compact_runs_without_session_or_force() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    synth(&tmp)
        .args(["claims", "compact", "--dry-run"])
        .assert()
        .success();
}

#[test]
fn outcome_list_runs_without_session() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    synth(&tmp)
        .args(["outcome", "list", "k/sample"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"outcomes\""));
}

#[test]
fn tree_show_runs_without_session() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    synth(&tmp)
        .args(["tree", "show", "k"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\":\"k\""));
}

#[test]
fn phase_show_without_session_errors_with_guidance() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    synth(&tmp)
        .args(["phase", "show"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("per-session"))
        .stderr(predicate::str::contains("--session"));
}

#[test]
fn phase_set_without_session_errors_with_guidance() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    synth(&tmp)
        .args(["phase", "set", "agree"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("per-session"));
}

#[test]
fn phase_show_with_session_returns_session_phase() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    let out = synth(&tmp)
        .args(["phase", "show", "--session", "s1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("\"session_id\":\"s1\""), "{s}");
    assert!(s.contains("\"phase\":\"plan\""), "{s}");
}

#[test]
fn status_drops_top_level_phase_and_embeds_per_session() {
    let tmp = tempfile::tempdir().unwrap();
    bootstrap(&tmp);
    let out = synth(&tmp)
        .args(["status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(
        v.get("phase").is_none(),
        "status must not emit a top-level `phase` field; got {v}"
    );
    let sessions = v.get("sessions").and_then(|s| s.as_array()).unwrap();
    assert!(!sessions.is_empty(), "expected at least one live session");
    for s in sessions {
        assert!(
            s.get("phase").is_some(),
            "every session entry must carry its own phase: {s}"
        );
    }
}

//! Integration tests for synthesist v2 — CLI against real claim store.
//!
//! Each test writes to a tempdir. Every command runs the release binary
//! as a subprocess so we exercise argument parsing, phase enforcement,
//! and the claim substrate end-to-end.
//!
//! The v1 integration suite (SQL schema, ATTACH merge, .synth/main.db
//! file-copy sessions) was removed in the v2 cutover (2026-04-18); see
//! `wave4_*` for migrated shape coverage and this file for the current
//! CLI contract.

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

// -----------------------------------------------------------------------------
// Basic CLI surface
// -----------------------------------------------------------------------------

#[test]
fn test_version() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp)
        .args(["version", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"version\":\"v"));
}

#[test]
fn test_skill_output() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp)
        .args(["skill"])
        .assert()
        .success();
}

// -----------------------------------------------------------------------------
// Init materializes claims/
// -----------------------------------------------------------------------------

#[test]
fn test_init_creates_claims_dir() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp)
        .args(["init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\":true"));
    assert!(tmp.path().join("claims").is_dir());
    assert!(tmp.path().join("claims/genesis.amc").is_file());
    assert!(tmp.path().join("claims/config.toml").is_file());
}

// -----------------------------------------------------------------------------
// Session required for writes
// -----------------------------------------------------------------------------

#[test]
fn test_write_without_session_errors_clearly() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).args(["init"]).assert().success();
    synth(&tmp)
        .args(["tree", "add", "keaton", "--description", "x"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("session required"));
}

// -----------------------------------------------------------------------------
// Tree + spec + task happy path
// -----------------------------------------------------------------------------

#[test]
fn test_tree_spec_task_flow() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).args(["init"]).assert().success();
    synth(&tmp)
        .args(["session", "start", "s1"])
        .assert()
        .success();
    // Phase must be `plan` to add trees/specs/tasks.
    synth(&tmp)
        .args(["--session", "s1", "phase", "set", "plan"])
        .assert()
        .success();
    synth(&tmp)
        .args(["--session", "s1", "tree", "add", "keaton", "--description", "k"])
        .assert()
        .success();
    synth(&tmp)
        .args(["--session", "s1", "spec", "add", "keaton/graphs", "--goal", "g"])
        .assert()
        .success();
    synth(&tmp)
        .args(["--session", "s1", "task", "add", "keaton/graphs", "first", "--id", "t1"])
        .assert()
        .success();
    let listed = synth(&tmp)
        .args(["--session", "s1", "task", "list", "keaton/graphs"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(listed).unwrap();
    assert!(out.contains("\"id\":\"t1\""), "task list: {out}");
}

// -----------------------------------------------------------------------------
// Phase transitions + enforcement
// -----------------------------------------------------------------------------

#[test]
fn test_phase_transition_rules() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).args(["init"]).assert().success();
    synth(&tmp)
        .args(["session", "start", "s1"])
        .assert()
        .success();
    // orient -> plan -> agree -> execute valid.
    synth(&tmp)
        .args(["--session", "s1", "phase", "set", "plan"])
        .assert()
        .success();
    synth(&tmp)
        .args(["--session", "s1", "phase", "set", "agree"])
        .assert()
        .success();
    synth(&tmp)
        .args(["--session", "s1", "phase", "set", "execute"])
        .assert()
        .success();
    // execute -> plan is NOT a valid transition without --force.
    synth(&tmp)
        .args(["--session", "s1", "phase", "set", "plan"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid phase transition"));
}

#[test]
fn test_phase_is_per_session() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).args(["init"]).assert().success();
    synth(&tmp)
        .args(["session", "start", "s1"])
        .assert()
        .success();
    synth(&tmp)
        .args(["session", "start", "s2"])
        .assert()
        .success();
    synth(&tmp)
        .args(["--session", "s1", "phase", "set", "plan"])
        .assert()
        .success();
    // s2 should still be in orient despite s1 having moved.
    let s2_phase = synth(&tmp)
        .args(["--session", "s2", "phase", "show"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(s2_phase).unwrap();
    assert!(out.contains("\"phase\":\"orient\""), "s2 phase: {out}");
}

// -----------------------------------------------------------------------------
// Landscape commands moved to `lattice` — verify migration message fires.
// -----------------------------------------------------------------------------

#[test]
fn test_stakeholder_moved_to_lattice() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).args(["init"]).assert().success();
    synth(&tmp)
        .args(["stakeholder", "list", "keaton"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("moved to `lattice`"));
}

// -----------------------------------------------------------------------------
// --data-dir flag + SYNTHESIST_DIR env resolve a remote claims/
// -----------------------------------------------------------------------------

#[test]
fn test_data_dir_flag_resolves_remote() {
    let tmp = tempfile::tempdir().unwrap();
    // Init claims at /tmp/xxx/subdir/
    let sub = tmp.path().join("subdir");
    std::fs::create_dir_all(&sub).unwrap();
    Command::cargo_bin("synthesist")
        .unwrap()
        .current_dir(&sub)
        .env("SYNTHESIST_OFFLINE", "1")
        .args(["init"])
        .assert()
        .success();
    // CWD is tmp root (not sub). --data-dir points at sub/. Tree add
    // should write into subdir/claims/.
    Command::cargo_bin("synthesist")
        .unwrap()
        .current_dir(tmp.path())
        .env("SYNTHESIST_OFFLINE", "1")
        .args([
            "--data-dir",
            sub.to_str().unwrap(),
            "session",
            "start",
            "s1",
        ])
        .assert()
        .success();
    assert!(sub.join("claims/genesis.amc").is_file());
}

#[test]
fn test_synthesist_dir_env_resolves_remote() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("subdir");
    std::fs::create_dir_all(&sub).unwrap();
    Command::cargo_bin("synthesist")
        .unwrap()
        .current_dir(&sub)
        .env("SYNTHESIST_OFFLINE", "1")
        .args(["init"])
        .assert()
        .success();
    Command::cargo_bin("synthesist")
        .unwrap()
        .current_dir(tmp.path())
        .env("SYNTHESIST_OFFLINE", "1")
        .env("SYNTHESIST_DIR", sub.to_str().unwrap())
        .args(["session", "start", "s1"])
        .assert()
        .success();
}

// -----------------------------------------------------------------------------
// Session close — non-destructive supersession
// -----------------------------------------------------------------------------

#[test]
fn test_session_close_hides_from_list() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).args(["init"]).assert().success();
    synth(&tmp)
        .args(["session", "start", "s1"])
        .assert()
        .success();
    synth(&tmp)
        .args(["session", "close", "s1"])
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

//! End-to-end tests for manifest RUNTIME dispatch (Phase D).
//!
//! Drives the real binary via `assert_cmd`. Covers:
//!   - a restrictive surface (`pruned`) rejects an excluded command with the
//!     prescriptive message and exit code 2;
//!   - the default surface (`baseline-v25`) allows baseline commands;
//!   - `surface use <name>` persists and switches the active surface;
//!   - `surface`/`version` are never blocked, even under a restrictive
//!     surface (no lock-out);
//!   - `SYNTHESIST_MANIFEST` and `--manifest` override the sticky setting.

use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::TempDir;

/// A bare binary handle scoped to `dir`, with inherited env stripped so the
/// real estate is never touched.
fn synth(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("synthesist").unwrap();
    cmd.current_dir(dir.path());
    cmd.env("SYNTHESIST_OFFLINE", "1");
    cmd.env_remove("SYNTHESIST_DIR");
    cmd.env_remove("SYNTHESIST_SESSION");
    cmd.env_remove("SYNTHESIST_MANIFEST");
    cmd
}

/// Initialize an estate in a fresh tempdir and return it.
fn init_estate() -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).arg("init").assert().success();
    tmp
}

// -----------------------------------------------------------------------------
// Baseline allows; pruned rejects
// -----------------------------------------------------------------------------

#[test]
fn baseline_allows_a_baseline_command() {
    let tmp = init_estate();
    // `status` is a baseline command; the default surface permits it.
    synth(&tmp).arg("status").assert().success();
}

#[test]
fn pruned_surface_rejects_excluded_command_with_exit_2() {
    let tmp = init_estate();
    // `pruned` excludes `task block`. Select it one-shot via --manifest.
    // `task block` would normally need a session; the rejection layer fires
    // first, before session enforcement, so we assert on exit 2 and message.
    synth(&tmp)
        .args([
            "--manifest",
            "pruned",
            "task",
            "block",
            "tree/spec",
            "t1",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not permitted by the active surface"))
        .stderr(predicate::str::contains("pruned"))
        .stderr(predicate::str::contains("synthesist surface use"));
}

#[test]
fn baseline_does_not_block_excluded_pruned_command() {
    // Under the default baseline surface, `task block` is permitted (it only
    // fails later on the missing session, NOT exit 2 from the surface layer).
    let tmp = init_estate();
    let out = synth(&tmp)
        .args(["task", "block", "tree/spec", "t1"])
        .assert()
        .failure()
        .get_output()
        .clone();
    // Not the surface rejection (which is exit 2 + the surface message).
    assert_ne!(out.status.code(), Some(2), "should not be a surface rejection");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("not permitted by the active surface"),
        "baseline must not surface-reject task block, got: {stderr}"
    );
}

// -----------------------------------------------------------------------------
// No lock-out: surface + version always allowed
// -----------------------------------------------------------------------------

#[test]
fn surface_command_allowed_under_restrictive_surface() {
    let tmp = init_estate();
    // Even though `pruned` does not list any `surface ...` key, `surface
    // list` must work: the operator can always inspect/switch surfaces.
    synth(&tmp)
        .args(["--manifest", "pruned", "surface", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("baseline-v25"));
}

#[test]
fn version_allowed_under_restrictive_surface() {
    let tmp = init_estate();
    synth(&tmp)
        .args(["--manifest", "pruned", "version", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("version"));
}

// -----------------------------------------------------------------------------
// surface use: persist + switch
// -----------------------------------------------------------------------------

#[test]
fn surface_use_persists_and_switches_active_surface() {
    let tmp = init_estate();

    // Switch to pruned via the sticky setting.
    synth(&tmp)
        .args(["surface", "use", "pruned"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\":true"))
        .stdout(predicate::str::contains("\"active\":\"pruned\""));

    // surface show now reports pruned as active.
    synth(&tmp)
        .args(["surface", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"active\":\"pruned\""));

    // And a pruned-excluded command (task block) is now rejected without any
    // --manifest override: the sticky setting governs.
    synth(&tmp)
        .args(["task", "block", "tree/spec", "t1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("pruned"));
}

#[test]
fn surface_use_unknown_name_fails() {
    let tmp = init_estate();
    synth(&tmp)
        .args(["surface", "use", "no-such-surface-xyz"])
        .assert()
        .failure();
}

// -----------------------------------------------------------------------------
// Precedence: env and --manifest override the sticky setting
// -----------------------------------------------------------------------------

#[test]
fn env_overrides_sticky_setting() {
    let tmp = init_estate();
    // Sticky = pruned.
    synth(&tmp).args(["surface", "use", "pruned"]).assert().success();

    // SYNTHESIST_MANIFEST=baseline-v25 overrides the sticky pruned, so
    // `surface show` reports baseline-v25.
    synth(&tmp)
        .env("SYNTHESIST_MANIFEST", "baseline-v25")
        .args(["surface", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"active\":\"baseline-v25\""));
}

#[test]
fn cli_manifest_overrides_sticky_and_env() {
    let tmp = init_estate();
    synth(&tmp).args(["surface", "use", "pruned"]).assert().success();

    // --manifest beats both env (pruned) and sticky (pruned).
    synth(&tmp)
        .env("SYNTHESIST_MANIFEST", "pruned")
        .args(["--manifest", "baseline-v25", "surface", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"active\":\"baseline-v25\""));
}

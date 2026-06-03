//! End-to-end tests for manifest RUNTIME dispatch (Phase D).
//!
//! Drives the real binary via `assert_cmd`. Covers:
//!   - filtering is OPT-IN: with NO surface configured, the full v3 surface is
//!     available -- even a non-baseline command like `overlay run` / `jig run`
//!     is allowed (no surface rejection);
//!   - a restrictive surface (`pruned`), selected explicitly, rejects an
//!     excluded command with the prescriptive message and exit code 2;
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
// Unconfigured default is unfiltered (full v3 surface); pruned rejects
// -----------------------------------------------------------------------------

#[test]
fn default_allows_a_baseline_command() {
    let tmp = init_estate();
    // No surface configured: the full surface is available, so `status` runs.
    synth(&tmp).arg("status").assert().success();
}

#[test]
fn default_allows_a_non_baseline_command() {
    // With NO surface configured, filtering is off: a non-baseline command
    // (`overlay run` / `jig run`) must NOT be surface-rejected. It may still
    // fail later for its own reasons, but never with the exit-2 surface
    // rejection.
    let tmp = init_estate();
    // `jig run` takes its own `--manifest` flag, which collides with the
    // global `--manifest` surface override; use `jig list-scenarios` (also a
    // non-baseline command) to exercise the jig family without that ambiguity.
    for args in [
        vec!["overlay", "run", "plan-at-risk"],
        vec!["jig", "list-scenarios"],
    ] {
        let out = synth(&tmp).args(&args).assert().get_output().clone();
        // The command may fail for its own reasons (missing scenario, empty
        // graph), but never with the surface rejection.
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("not permitted by the active surface"),
            "default must not surface-reject {args:?}, got: {stderr}"
        );
    }
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
fn default_does_not_block_excluded_pruned_command() {
    // Under the unconfigured (unfiltered) default, `task block` is permitted
    // (it only fails later on the missing session, NOT exit 2 from the surface
    // layer).
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
        "unfiltered default must not surface-reject task block, got: {stderr}"
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

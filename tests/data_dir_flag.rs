//! Integration test for the `--data-dir` flag and `SYNTHESIST_DIR`
//! environment variable.
//!
//! v2.1.0 silently ignored both, falling through to `cwd/claims/` via
//! the workflow crate's `Store::discover` fallback. v2.1.1 honors the
//! override and errors loudly on invalid paths.

use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::TempDir;

fn synth() -> Command {
    let mut cmd = Command::cargo_bin("synthesist").unwrap();
    cmd.env("SYNTHESIST_OFFLINE", "1");
    cmd.env_remove("SYNTHESIST_DIR");
    cmd
}

#[test]
fn data_dir_opens_initialized_store_from_elsewhere() {
    // Initialize a store in `init_dir` and add a Tree claim so there
    // is something observable in the view that is NOT at the elsewhere
    // cwd — proves discover honored the override.
    let init_dir = TempDir::new().unwrap();
    synth()
        .current_dir(init_dir.path())
        .arg("init")
        .assert()
        .success();

    synth()
        .current_dir(init_dir.path())
        .args(["session", "start", "s1"])
        .assert()
        .success();

    synth()
        .current_dir(init_dir.path())
        .args(["--session=s1", "--force", "tree", "add", "mytree"])
        .assert()
        .success();

    // Run status from a totally different cwd, using --data-dir. Should
    // surface the Tree claim from init_dir, not init an empty store at
    // elsewhere.
    let elsewhere = TempDir::new().unwrap();
    synth()
        .current_dir(elsewhere.path())
        .args(["--data-dir", init_dir.path().to_str().unwrap(), "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"mytree\""));
}

#[test]
fn data_dir_missing_path_errors() {
    let cwd = TempDir::new().unwrap();
    synth()
        .current_dir(cwd.path())
        .args([
            "--data-dir",
            "/tmp/definitely-does-not-exist-xyz-123",
            "status",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn data_dir_uninitialized_path_errors() {
    // Path B v3-native dropped the v2 `claims/genesis.amc` signal in
    // favor of a `claims/` directory check; assert the new error shape.
    let not_a_store = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    synth()
        .current_dir(elsewhere.path())
        .args(["--data-dir", not_a_store.path().to_str().unwrap(), "status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no `claims/` directory"))
        .stderr(predicate::str::contains("synthesist init"));
}

#[test]
fn init_data_dir_creates_target_claims_not_ancestor() {
    // Regression: `init` used to ignore --data-dir / SYNTHESIST_DIR and
    // always init `<cwd>/claims`. It now resolves the root the same way
    // every other command does (the SYNTHESIST_DIR main.rs propagates
    // from --data-dir), and it must NOT walk up to an ancestor claims/.
    //
    // Layout to prove no walk-up:
    //   parent/claims         <- a pre-existing ancestor estate
    //   parent/nested/target  <- the --data-dir we point init at
    // init must create parent/nested/target/claims, never reuse
    // parent/claims.
    let parent = TempDir::new().unwrap();
    let ancestor_claims = parent.path().join("claims");
    std::fs::create_dir_all(&ancestor_claims).unwrap();

    let target = parent.path().join("nested").join("target");
    std::fs::create_dir_all(&target).unwrap();

    // Run init from a totally unrelated cwd so cwd cannot be the source
    // of the resolved root; the only signal is --data-dir.
    let elsewhere = TempDir::new().unwrap();
    synth()
        .current_dir(elsewhere.path())
        .args(["--data-dir", target.to_str().unwrap(), "init"])
        .assert()
        .success();

    // The claims dir lands under the --data-dir target, exactly.
    let target_claims = target.join("claims");
    assert!(
        target_claims.is_dir(),
        "init --data-dir must create <target>/claims at {}",
        target_claims.display()
    );

    // No-flag cwd behavior still works: init with neither flag nor env
    // creates <cwd>/claims.
    let cwd_only = TempDir::new().unwrap();
    synth()
        .current_dir(cwd_only.path())
        .arg("init")
        .assert()
        .success();
    assert!(
        cwd_only.path().join("claims").is_dir(),
        "init with no flag must still create <cwd>/claims"
    );
}

#[test]
fn synthesist_dir_env_is_honored() {
    let init_dir = TempDir::new().unwrap();
    synth()
        .current_dir(init_dir.path())
        .arg("init")
        .assert()
        .success();

    synth()
        .current_dir(init_dir.path())
        .args(["session", "start", "s1"])
        .assert()
        .success();

    synth()
        .current_dir(init_dir.path())
        .args(["--session=s1", "--force", "tree", "add", "envtree"])
        .assert()
        .success();

    let elsewhere = TempDir::new().unwrap();
    synth()
        .current_dir(elsewhere.path())
        .env("SYNTHESIST_DIR", init_dir.path())
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"envtree\""));
}

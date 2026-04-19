//! CLI smoke test: `claim init` in a tempdir scaffolds `claims/genesis.amc`.
//!
//! Uses `assert_cmd` with `current_dir` so the CLI resolves `claims/` under
//! the tempdir rather than the repo's working tree.

use assert_cmd::Command;

#[test]
fn init_scaffolds_claims_genesis() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let assert = Command::cargo_bin("claim")
        .unwrap()
        .current_dir(root)
        .arg("init")
        .assert();

    assert.success();

    let genesis = root.join("claims").join("genesis.amc");
    assert!(
        genesis.exists(),
        "expected genesis at {}, missing after init",
        genesis.display()
    );
    let config = root.join("claims").join("config.toml");
    assert!(config.exists(), "expected config.toml after init");
}

#[test]
fn init_refuses_when_genesis_already_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    Command::cargo_bin("claim")
        .unwrap()
        .current_dir(root)
        .arg("init")
        .assert()
        .success();

    // Second init must fail; Store::init refuses to clobber.
    Command::cargo_bin("claim")
        .unwrap()
        .current_dir(root)
        .arg("init")
        .assert()
        .failure();
}

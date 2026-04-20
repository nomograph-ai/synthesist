//! Wave 4 M3 — Landscape family moved to `lattice`.
//!
//! The stakeholder/disposition/signal/stance commands used to write to the
//! v1 landscape tables. In v2 they moved to the `lattice` binary. The CLI
//! still accepts them so existing muscle memory gets a clear migration
//! message, but every invocation exits non-zero with code 3.

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

#[test]
fn stakeholder_add_prints_moved_message() {
    let dir = TempDir::new().unwrap();
    synth(&dir).arg("init").assert().success();

    synth(&dir)
        .args([
            "stakeholder",
            "add",
            "demo",
            "sh-1",
            "--context",
            "lead reviewer",
        ])
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains(
            "the `stakeholder` command moved to `lattice` in v2",
        ))
        .stderr(predicate::str::contains("cargo install nomograph-lattice"))
        .stderr(predicate::str::contains("stakeholder"));
}

#[test]
fn disposition_add_prints_moved_message() {
    let dir = TempDir::new().unwrap();
    synth(&dir).arg("init").assert().success();

    synth(&dir)
        .args([
            "disposition",
            "add",
            "demo/any",
            "sh-1",
            "--topic",
            "claim substrate",
            "--stance",
            "supportive",
            "--confidence",
            "verified",
        ])
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains(
            "the `disposition` command moved to `lattice` in v2",
        ));
}

#[test]
fn signal_add_prints_moved_message() {
    let dir = TempDir::new().unwrap();
    synth(&dir).arg("init").assert().success();

    synth(&dir)
        .args([
            "signal",
            "add",
            "demo/any",
            "sh-1",
            "--source",
            "https://example.com",
            "--source-type",
            "issue_comment",
            "--content",
            "lgtm",
        ])
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains(
            "the `signal` command moved to `lattice` in v2",
        ));
}

#[test]
fn stance_prints_moved_message() {
    let dir = TempDir::new().unwrap();
    synth(&dir).arg("init").assert().success();

    synth(&dir)
        .args(["stance", "sh-1"])
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains(
            "the `stance` command moved to `lattice` in v2",
        ));
}

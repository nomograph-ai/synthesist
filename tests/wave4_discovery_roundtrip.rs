//! Wave 4 M3 — Discovery claim roundtrip.
//!
//! Add a Discovery via the CLI, list it, and verify the output carries the
//! same finding/date/author back. This is the smoke test for the v2
//! claim-backed Discovery port.

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

fn synth_session(dir: &TempDir, session: &str) -> Command {
    let mut cmd = synth(dir);
    cmd.arg("--session").arg(session).arg("--force");
    cmd
}

#[test]
fn discovery_add_then_list_returns_the_finding() {
    let dir = TempDir::new().unwrap();
    synth(&dir).arg("init").assert().success();

    synth_session(&dir, "m3")
        .args(["tree", "add", "demo", "--description", "M3 demo tree"])
        .assert()
        .success();

    synth_session(&dir, "m3")
        .args([
            "discovery",
            "add",
            "demo/wave4",
            "--finding",
            "claim projection wins on concurrency",
            "--impact",
            "changes approach",
            "--action",
            "port discovery to claims",
            "--author",
            "andunn",
            "--date",
            "2026-04-18",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "claim projection wins on concurrency",
        ))
        .stdout(predicate::str::contains("2026-04-18"));

    synth(&dir)
        .args(["discovery", "list", "demo/wave4"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "claim projection wins on concurrency",
        ))
        .stdout(predicate::str::contains("andunn"))
        .stdout(predicate::str::contains("port discovery to claims"));
}

#[test]
fn discovery_add_rejects_empty_finding() {
    let dir = TempDir::new().unwrap();
    synth(&dir).arg("init").assert().success();

    synth_session(&dir, "m3")
        .args(["tree", "add", "demo"])
        .assert()
        .success();

    synth_session(&dir, "m3")
        .args(["discovery", "add", "demo/wave4", "--finding", ""])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Discovery requires non-empty 'finding' field",
        ));
}

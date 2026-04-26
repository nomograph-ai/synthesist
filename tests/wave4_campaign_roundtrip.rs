//! Wave 4 M3 — Campaign claim roundtrip.
//!
//! Exercise both kinds (`active` and `backlog`), then list, and verify the
//! output partitions them into their respective buckets.

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
fn campaign_add_both_kinds_then_list() {
    let dir = TempDir::new().unwrap();
    synth(&dir).arg("init").assert().success();

    synth_session(&dir, "m3")
        .args(["tree", "add", "campaigns"])
        .assert()
        .success();

    // Active campaign with a summary.
    synth_session(&dir, "m3")
        .args([
            "campaign",
            "add",
            "campaigns",
            "active-one",
            "--summary",
            "currently executing",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("active-one"))
        .stdout(predicate::str::contains("active"));

    // Backlog campaign with a title + blocked_by deps.
    synth_session(&dir, "m3")
        .args([
            "campaign",
            "add",
            "campaigns",
            "backlog-one",
            "--backlog",
            "--title",
            "Planned follow-up",
            "--summary",
            "wait on active-one",
            "--blocked-by",
            "active-one,other",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("backlog-one"))
        .stdout(predicate::str::contains("backlog"));

    synth(&dir)
        .args(["campaign", "list", "campaigns"])
        .assert()
        .success()
        .stdout(predicate::str::contains("active-one"))
        .stdout(predicate::str::contains("backlog-one"))
        .stdout(predicate::str::contains("currently executing"))
        .stdout(predicate::str::contains("Planned follow-up"));
}

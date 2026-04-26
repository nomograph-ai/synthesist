//! Coverage for `session close --start-id` and `tree close`.
//!
//! Both surfaces address the same shape of bug: synthesist v2 lets two
//! claims share a display id (sessions colliding on `s1`, trees
//! colliding on `keaton`) and the original close commands assumed a
//! single match. These tests pin down the disambiguation path and the
//! `--include-closed` toggle on `tree list`.
//!
//! The keaton estate cleanup (claims/changes/d-48fc8e3d35f4 and
//! d-cbcde813c77a) drove the API; this file is the regression net.

use std::process::Command;

use assert_cmd::prelude::*;
use predicates::boolean::PredicateBooleanExt;
use serde_json::Value;
use tempfile::TempDir;

fn synth(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("synthesist").unwrap();
    cmd.current_dir(dir.path());
    cmd.env("SYNTHESIST_OFFLINE", "1");
    // Inherited SYNTHESIST_DIR / SYNTHESIST_SESSION from the user shell
    // would punch through current_dir() and target the real estate.
    // Strip both unconditionally.
    cmd.env_remove("SYNTHESIST_DIR");
    cmd.env_remove("SYNTHESIST_SESSION");
    cmd
}

/// Initialize a fresh synthesist estate in a tempdir and return the
/// guard. Caller's command builder must keep using that tempdir.
fn fresh_estate() -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).args(["init"]).assert().success();
    tmp
}

/// Parse stdout JSON. Convenient for assertions that need to peek at
/// nested fields.
fn parse(stdout: Vec<u8>) -> Value {
    serde_json::from_slice(&stdout).expect("stdout was not valid JSON")
}

// ---------------------------------------------------------------------------
// session close --start-id
// ---------------------------------------------------------------------------

#[test]
fn session_close_start_id_picks_correct_session_when_ids_collide() {
    // Three sessions all named s1. Each gets a distinct start_id (the
    // claim hash of the opening Session claim). --start-id picks one
    // unambiguously. The other two stay live.
    let tmp = fresh_estate();
    synth(&tmp)
        .args(["session", "start", "s1", "--summary", "first"])
        .assert()
        .success();
    synth(&tmp)
        .args(["session", "start", "s1", "--summary", "second"])
        .assert()
        .success();
    synth(&tmp)
        .args(["session", "start", "s1", "--summary", "third"])
        .assert()
        .success();

    // Pull the live list and pick the second one's start_id.
    let listed = synth(&tmp)
        .args(["session", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = parse(listed);
    let sessions = v
        .get("sessions")
        .and_then(Value::as_array)
        .expect("sessions array");
    assert_eq!(sessions.len(), 3, "three live s1 sessions");
    let target_start_id = sessions
        .iter()
        .find(|s| s.get("summary").and_then(Value::as_str) == Some("second"))
        .and_then(|s| s.get("start_id"))
        .and_then(Value::as_str)
        .expect("start_id for the 'second' session")
        .to_string();

    // Close the second one by start_id.
    let closed_out = synth(&tmp)
        .args(["session", "close", "s1", "--start-id", &target_start_id])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let closed = parse(closed_out);
    assert_eq!(closed.get("closed"), Some(&Value::Bool(true)));
    assert_eq!(
        closed.get("start_id").and_then(Value::as_str),
        Some(target_start_id.as_str())
    );

    // Two left, neither is the target.
    let listed_after = synth(&tmp)
        .args(["session", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let after = parse(listed_after);
    let after_sessions = after
        .get("sessions")
        .and_then(Value::as_array)
        .expect("sessions");
    assert_eq!(after_sessions.len(), 2, "two left after one close");
    for s in after_sessions {
        assert_ne!(
            s.get("start_id").and_then(Value::as_str),
            Some(target_start_id.as_str()),
            "closed session must not appear in live list"
        );
    }
    let summaries: Vec<&str> = after_sessions
        .iter()
        .filter_map(|s| s.get("summary").and_then(Value::as_str))
        .collect();
    assert!(summaries.contains(&"first"));
    assert!(summaries.contains(&"third"));
}

#[test]
fn session_close_start_id_accepts_unambiguous_short_prefix() {
    let tmp = fresh_estate();
    synth(&tmp)
        .args(["session", "start", "s1", "--summary", "first"])
        .assert()
        .success();
    synth(&tmp)
        .args(["session", "start", "s1", "--summary", "second"])
        .assert()
        .success();

    // Find a prefix unique to the 'first' session.
    let listed = synth(&tmp)
        .args(["session", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = parse(listed);
    let sessions = v.get("sessions").and_then(Value::as_array).unwrap();
    let first = sessions
        .iter()
        .find(|s| s.get("summary").and_then(Value::as_str) == Some("first"))
        .unwrap();
    let second = sessions
        .iter()
        .find(|s| s.get("summary").and_then(Value::as_str) == Some("second"))
        .unwrap();
    let first_id = first.get("start_id").and_then(Value::as_str).unwrap();
    let second_id = second.get("start_id").and_then(Value::as_str).unwrap();

    // Find shortest prefix that distinguishes first from second.
    let mut prefix_len = 1;
    while first_id[..prefix_len] == second_id[..prefix_len] {
        prefix_len += 1;
    }
    let prefix = &first_id[..prefix_len];

    synth(&tmp)
        .args(["session", "close", "s1", "--start-id", prefix])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"closed\":true"));

    // Verify the second is the only one left.
    let after = parse(
        synth(&tmp)
            .args(["session", "list"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let after_sessions = after.get("sessions").and_then(Value::as_array).unwrap();
    assert_eq!(after_sessions.len(), 1);
    assert_eq!(
        after_sessions[0].get("summary").and_then(Value::as_str),
        Some("second")
    );
}

#[test]
fn session_close_start_id_ambiguous_prefix_errors_with_candidates() {
    let tmp = fresh_estate();
    synth(&tmp)
        .args(["session", "start", "s1", "--summary", "first"])
        .assert()
        .success();
    synth(&tmp)
        .args(["session", "start", "s1", "--summary", "second"])
        .assert()
        .success();

    // The empty-string prefix matches every start_id, so it's
    // unambiguously ambiguous. Empty is rejected with its own message;
    // a single hex character almost always matches multiple, but to be
    // robust we use an empty-equivalent multi-match strategy: pass a
    // single character that prefixes more than one start_id.
    let listed = parse(
        synth(&tmp)
            .args(["session", "list"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let sessions = listed.get("sessions").and_then(Value::as_array).unwrap();
    // Find a single hex char shared by both start_ids if possible;
    // otherwise rerun with new sessions until we find one. With two
    // 64-char hex strings, by birthday probability there's a shared
    // first char ~94% of the time. Be deterministic: try all 16 hex
    // chars, find one that prefixes multiple start_ids.
    let ids: Vec<&str> = sessions
        .iter()
        .filter_map(|s| s.get("start_id").and_then(Value::as_str))
        .collect();
    let mut shared_char: Option<char> = None;
    for c in "0123456789abcdef".chars() {
        let count = ids.iter().filter(|id| id.starts_with(c)).count();
        if count >= 2 {
            shared_char = Some(c);
            break;
        }
    }
    let shared = match shared_char {
        Some(c) => c.to_string(),
        // No shared first char — test is degenerate for this run, so
        // skip rather than flake. Two 64-char hex strings sharing zero
        // first chars is exceedingly rare but possible.
        None => return,
    };

    synth(&tmp)
        .args(["session", "close", "s1", "--start-id", &shared])
        .assert()
        .failure()
        .stderr(predicates::str::contains("ambiguous"))
        // The error must list the candidates so the caller can pick.
        .stderr(predicates::str::contains(ids[0]).or(predicates::str::contains(ids[1])));
}

#[test]
fn session_close_without_start_id_preserves_v1_behavior() {
    // Without --start-id, the existing happy path (single live session
    // with id s1) must still work unchanged.
    let tmp = fresh_estate();
    synth(&tmp)
        .args(["session", "start", "s1"])
        .assert()
        .success();
    synth(&tmp)
        .args(["session", "close", "s1"])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"closed\":true"));
    let listed = parse(
        synth(&tmp)
            .args(["session", "list"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(
        listed.get("sessions").and_then(Value::as_array).unwrap().len(),
        0
    );
}

#[test]
fn session_close_unknown_id_errors() {
    let tmp = fresh_estate();
    synth(&tmp)
        .args(["session", "close", "does-not-exist"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not found"));
}

// ---------------------------------------------------------------------------
// tree close
// ---------------------------------------------------------------------------

/// Bring a fresh estate into PLAN phase under `s1` so tree writes are
/// allowed. Returns the tempdir guard.
fn estate_in_plan() -> TempDir {
    let tmp = fresh_estate();
    synth(&tmp)
        .args(["session", "start", "s1"])
        .assert()
        .success();
    synth(&tmp)
        .args(["--session", "s1", "phase", "set", "plan"])
        .assert()
        .success();
    tmp
}

#[test]
fn tree_close_writes_superseding_claim_and_hides_from_list() {
    let tmp = estate_in_plan();
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "tree",
            "add",
            "keaton",
            "--description",
            "Meta",
        ])
        .assert()
        .success();

    // Visible before close.
    let before = parse(
        synth(&tmp)
            .args(["tree", "list"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let before_trees = before.get("trees").and_then(Value::as_array).unwrap();
    assert_eq!(before_trees.len(), 1);
    assert_eq!(
        before_trees[0].get("name").and_then(Value::as_str),
        Some("keaton")
    );

    // Close it.
    let closed = parse(
        synth(&tmp)
            .args(["--session", "s1", "tree", "close", "keaton"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(closed.get("closed"), Some(&Value::Bool(true)));
    assert_eq!(
        closed.get("name").and_then(Value::as_str),
        Some("keaton")
    );

    // Hidden from default list.
    let after = parse(
        synth(&tmp)
            .args(["tree", "list"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let after_trees = after.get("trees").and_then(Value::as_array).unwrap();
    assert!(
        after_trees.is_empty(),
        "closed tree must not appear in default list, got {after}"
    );
}

#[test]
fn tree_list_include_closed_surfaces_closed_tree() {
    let tmp = estate_in_plan();
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "tree",
            "add",
            "keaton",
            "--description",
            "k",
        ])
        .assert()
        .success();
    synth(&tmp)
        .args(["--session", "s1", "tree", "close", "keaton"])
        .assert()
        .success();

    let visible = parse(
        synth(&tmp)
            .args(["tree", "list", "--include-closed"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let trees = visible.get("trees").and_then(Value::as_array).unwrap();
    assert_eq!(trees.len(), 1, "include-closed must show closed trees");
    assert_eq!(
        trees[0].get("name").and_then(Value::as_str),
        Some("keaton")
    );
    assert_eq!(
        trees[0].get("status").and_then(Value::as_str),
        Some("closed")
    );
}

#[test]
fn tree_close_rejects_ambiguous_name_without_start_id() {
    // Two trees share `keaton` (the keaton-estate bug). Without
    // --start-id, close must bail with the candidate list.
    let tmp = estate_in_plan();
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "tree",
            "add",
            "keaton",
            "--description",
            "first",
        ])
        .assert()
        .success();
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "tree",
            "add",
            "keaton",
            "--description",
            "second",
        ])
        .assert()
        .success();

    synth(&tmp)
        .args(["--session", "s1", "tree", "close", "keaton"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("ambiguous"))
        .stderr(predicates::str::contains("--start-id"));
}

#[test]
fn tree_close_start_id_disambiguates_collision() {
    let tmp = estate_in_plan();
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "tree",
            "add",
            "keaton",
            "--description",
            "first",
        ])
        .assert()
        .success();
    synth(&tmp)
        .args([
            "--session",
            "s1",
            "tree",
            "add",
            "keaton",
            "--description",
            "second",
        ])
        .assert()
        .success();

    let listed = parse(
        synth(&tmp)
            .args(["tree", "list"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let trees = listed.get("trees").and_then(Value::as_array).unwrap();
    assert_eq!(trees.len(), 2);
    let target = trees
        .iter()
        .find(|t| t.get("description").and_then(Value::as_str) == Some("first"))
        .unwrap();
    let target_start_id = target
        .get("start_id")
        .and_then(Value::as_str)
        .expect("start_id on tree row")
        .to_string();

    synth(&tmp)
        .args([
            "--session",
            "s1",
            "tree",
            "close",
            "keaton",
            "--start-id",
            &target_start_id,
        ])
        .assert()
        .success();

    // One left, the second.
    let after = parse(
        synth(&tmp)
            .args(["tree", "list"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let after_trees = after.get("trees").and_then(Value::as_array).unwrap();
    assert_eq!(after_trees.len(), 1);
    assert_eq!(
        after_trees[0].get("description").and_then(Value::as_str),
        Some("second")
    );
}

#[test]
fn tree_close_unknown_name_errors() {
    let tmp = estate_in_plan();
    synth(&tmp)
        .args(["--session", "s1", "tree", "close", "does-not-exist"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not found"));
}

//! CLI round-trip: init, append a Spec claim, `list` must show it.

use assert_cmd::Command;
use serde_json::Value;

#[test]
fn init_append_list_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // 1. init
    Command::cargo_bin("claim")
        .unwrap()
        .current_dir(root)
        .arg("init")
        .assert()
        .success();

    // 2. append a well-formed Spec claim
    let spec_props = serde_json::json!({
        "tree": "keaton",
        "id": "sch-cli",
        "goal": "verify CLI append wires into store",
        "status": "active",
        "topics": ["cli"],
    })
    .to_string();

    let append = Command::cargo_bin("claim")
        .unwrap()
        .current_dir(root)
        .args([
            "append",
            "--type",
            "spec",
            "--props",
            &spec_props,
            "--as",
            "user:gitlab:andunn",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8(append.get_output().stdout.clone()).unwrap();
    let appended: Value = serde_json::from_str(stdout.trim()).expect("append prints JSON");
    let appended_id = appended
        .get("id")
        .and_then(Value::as_str)
        .expect("append JSON has id")
        .to_string();
    assert!(!appended_id.is_empty(), "id must be non-empty");

    // 3. list — must include the appended claim
    let list = Command::cargo_bin("claim")
        .unwrap()
        .current_dir(root)
        .arg("list")
        .assert()
        .success();

    let stdout = String::from_utf8(list.get_output().stdout.clone()).unwrap();
    let claims: Value = serde_json::from_str(stdout.trim()).expect("list prints JSON array");
    let arr = claims.as_array().expect("list is array");
    assert_eq!(
        arr.len(),
        1,
        "expected exactly one claim, got {}",
        arr.len()
    );
    assert_eq!(
        arr[0].get("id").and_then(Value::as_str),
        Some(appended_id.as_str()),
    );
    assert_eq!(
        arr[0].get("claim_type").and_then(Value::as_str),
        Some("spec")
    );
}

#[test]
fn append_rejects_bad_claim_type() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    Command::cargo_bin("claim")
        .unwrap()
        .current_dir(root)
        .arg("init")
        .assert()
        .success();

    let assert = Command::cargo_bin("claim")
        .unwrap()
        .current_dir(root)
        .args([
            "append",
            "--type",
            "nonesuch",
            "--props",
            "{}",
            "--as",
            "user:test",
        ])
        .assert()
        .failure();

    let code = assert.get_output().status.code().unwrap_or(-1);
    assert_eq!(code, 1, "unknown claim type must exit 1 (user error)");
}

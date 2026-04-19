//! Integration tests for the snapshot compaction dance (05 rule #2).

use nomograph_claim::{Claim, ClaimType, Store};
use std::fs;

fn mk(i: u64) -> Claim {
    Claim::new(
        ClaimType::Disposition,
        serde_json::json!({ "topic": format!("topic-{i}"), "stance": "favors" }),
        "user:gitlab:andunn",
    )
}

#[test]
fn compact_produces_snapshot_and_clears_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    for i in 0..12 {
        store.append(&mk(i)).unwrap();
    }
    let before = fs::read_dir(root.join("changes")).unwrap().count();
    assert!(before > 0);
    store.compact().unwrap();
    assert!(root.join("snapshot.amc").exists());
    assert!(!root.join("snapshot.amc.new").exists());
    let after = fs::read_dir(root.join("changes")).unwrap().count();
    assert_eq!(after, 0, "compact must sweep superseded changes");
}

#[test]
fn open_uses_snapshot_and_preserves_claims() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    for i in 0..10 {
        store.append(&mk(i)).unwrap();
    }
    store.compact().unwrap();
    drop(store);
    let mut reopened = Store::open(&root).unwrap();
    let claims = reopened.load_claims().unwrap();
    assert_eq!(claims.len(), 10);
}

#[test]
fn corrupt_snapshot_falls_back_to_genesis_plus_changes() {
    // Per 05 rule #3: a corrupt snapshot must not crash open().
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    for i in 0..4 {
        store.append(&mk(i)).unwrap();
    }
    drop(store);
    // Drop a junk snapshot next to the real change files.
    fs::write(root.join("snapshot.amc"), b"not a valid automerge doc").unwrap();
    let mut reopened = Store::open(&root).unwrap();
    let claims = reopened.load_claims().unwrap();
    assert_eq!(
        claims.len(),
        4,
        "must fall back to genesis + changes replay"
    );
}

#[test]
fn compact_can_be_run_twice() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    for i in 0..6 {
        store.append(&mk(i)).unwrap();
    }
    store.compact().unwrap();
    // Add more then compact again.
    for i in 100..103 {
        store.append(&mk(i)).unwrap();
    }
    store.compact().unwrap();
    assert!(root.join("snapshot.amc").exists());
    let mut reopened = Store::open(&root).unwrap();
    let claims = reopened.load_claims().unwrap();
    assert_eq!(claims.len(), 9);
}

#[test]
fn append_after_compact_still_writes_to_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    store.append(&mk(1)).unwrap();
    store.compact().unwrap();
    // After compaction, any new append must land as a new change file
    // next to the snapshot.
    store.append(&mk(2)).unwrap();
    let count = fs::read_dir(root.join("changes"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .map(|e| e.path().extension().and_then(|s| s.to_str()) == Some("amc"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(count, 1, "post-compact append must persist");
}

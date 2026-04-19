//! Integration tests for [`View::sync`] — head-gated rebuild.

use nomograph_claim::{Claim, ClaimType, Store, View};

fn mk(i: u64) -> Claim {
    Claim::new(
        ClaimType::Task,
        serde_json::json!({ "title": format!("task-{i}") }),
        "user:gitlab:andunn",
    )
}

#[test]
fn sync_on_unchanged_store_is_noop() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    store.append(&mk(1)).unwrap();
    let mut view = View::open(&root).unwrap();

    // First sync populates and persists heads.
    let first = view.sync(&mut store).unwrap();
    assert!(first, "first sync on a non-empty store must rebuild");

    // Second sync with no new writes must be a no-op.
    let second = view.sync(&mut store).unwrap();
    assert!(!second, "repeat sync on unchanged heads must be no-op");
}

#[test]
fn sync_after_append_rebuilds() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    let mut view = View::open(&root).unwrap();

    // Prime the heads cache on an empty store; sync returns true because
    // the cache was empty and current heads are non-empty (the genesis
    // commit). Accept either outcome deterministically.
    let _ = view.sync(&mut store).unwrap();
    let baseline = view
        .query("SELECT COUNT(*) AS n FROM claims", &[])
        .unwrap();
    assert_eq!(baseline[0]["n"], serde_json::json!(0));

    // Append advances heads; next sync must rebuild.
    store.append(&mk(1)).unwrap();
    let rebuilt = view.sync(&mut store).unwrap();
    assert!(rebuilt, "sync after append must return true");
    let rows = view
        .query("SELECT COUNT(*) AS n FROM claims", &[])
        .unwrap();
    assert_eq!(rows[0]["n"], serde_json::json!(1));
}

#[test]
fn sync_persists_heads_across_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    store.append(&mk(1)).unwrap();
    {
        let mut view = View::open(&root).unwrap();
        let rebuilt = view.sync(&mut store).unwrap();
        assert!(rebuilt);
    }
    // Reopen: the persisted heads file must prevent a second rebuild
    // when the store is unchanged.
    let mut view = View::open(&root).unwrap();
    let rebuilt = view.sync(&mut store).unwrap();
    assert!(
        !rebuilt,
        "reopen + sync on unchanged heads must not rebuild"
    );
    let rows = view
        .query("SELECT COUNT(*) AS n FROM claims", &[])
        .unwrap();
    assert_eq!(
        rows[0]["n"],
        serde_json::json!(1),
        "projection row survives reopen"
    );
}

#[test]
fn query_only_accepts_reads() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let _store = Store::init(&root).unwrap();
    let view = View::open(&root).unwrap();
    let err = view
        .query("INSERT INTO claims(id,claim_type,props,valid_from,asserted_by,asserted_at) VALUES ('x','spec','{}',0,'u',0)", &[])
        .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("SELECT"), "err was: {msg}");
}

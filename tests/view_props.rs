//! Deterministic-sweep property tests for [`View`] invariants.
//!
//! `proptest` is not in this crate's dev-deps (see `store_props.rs`
//! rationale). These sweep a fixed range of inputs to give property-
//! grade signal without adding a dependency.

use nomograph_claim::{Claim, ClaimType, Store, View};
use std::collections::HashSet;

fn mk(who: &str, i: u64) -> Claim {
    Claim::new(
        ClaimType::Signal,
        serde_json::json!({ "seed": i, "topic": format!("t-{}", i % 5) }),
        who,
    )
}

fn all_ids(view: &View) -> Vec<String> {
    let rows = view
        .query("SELECT id FROM claims ORDER BY id", &[])
        .unwrap();
    rows.into_iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect()
}

fn row_count(view: &View) -> i64 {
    let rows = view.query("SELECT COUNT(*) AS n FROM claims", &[]).unwrap();
    rows[0]["n"].as_i64().unwrap()
}

#[test]
fn rebuild_is_deterministic_across_sizes() {
    // Invariant: rebuilding twice from the same Automerge doc produces
    // identical row counts and identical sorted id lists.
    for n in [0u64, 1, 2, 5, 11, 23] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("claims");
        let mut store = Store::init(&root).unwrap();
        for i in 0..n {
            store.append(&mk("user:gitlab:andunn", i)).unwrap();
        }
        let mut view = View::open(&root).unwrap();
        view.rebuild(&mut store).unwrap();
        let first_count = row_count(&view);
        let first_ids = all_ids(&view);

        view.rebuild(&mut store).unwrap();
        let second_count = row_count(&view);
        let second_ids = all_ids(&view);

        assert_eq!(
            first_count, n as i64,
            "row count must equal input size for n={n}"
        );
        assert_eq!(first_count, second_count, "rebuild count drift at n={n}");
        assert_eq!(first_ids, second_ids, "rebuild id order drift at n={n}");
    }
}

#[test]
fn projection_rows_equal_load_claims_ids() {
    // Invariant: the set of ids visible through View matches the set
    // returned by Store::load_claims for every sweep size.
    for n in [0u64, 1, 3, 7, 15] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("claims");
        let mut store = Store::init(&root).unwrap();
        for i in 0..n {
            store.append(&mk("user:gitlab:andunn", i)).unwrap();
        }
        let mut view = View::open(&root).unwrap();
        view.rebuild(&mut store).unwrap();

        let from_view: HashSet<String> = all_ids(&view).into_iter().collect();
        let from_store: HashSet<String> = store
            .load_claims()
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(
            from_view, from_store,
            "view / store id sets must match at n={n}"
        );
    }
}

#[test]
fn sync_returns_true_exactly_when_rebuild_is_needed() {
    // Invariant: sync returns true iff the projection was stale.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    let mut view = View::open(&root).unwrap();

    // Initial sync from a fresh dir: heads file is absent, so must rebuild.
    assert!(view.sync(&mut store).unwrap());
    // Immediate repeat: no change, no rebuild.
    assert!(!view.sync(&mut store).unwrap());

    for i in 0..4 {
        store.append(&mk("user:gitlab:andunn", i)).unwrap();
        // Each append advances heads; exactly one sync must rebuild.
        assert!(
            view.sync(&mut store).unwrap(),
            "append {i} did not trigger rebuild"
        );
        assert!(
            !view.sync(&mut store).unwrap(),
            "append {i} second sync must be no-op"
        );
    }
}

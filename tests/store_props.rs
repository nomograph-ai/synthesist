//! Property-style tests for the `Store` CRDT contract.
//!
//! `proptest` is not available in this crate (Cargo.toml is out of scope
//! for Wave 2 module implementors). These tests exercise the invariants
//! by sweeping deterministic seed ranges, which is sufficient signal for
//! the weekend sprint. Swap in `proptest` once Cargo.toml opens up.

use nomograph_claim::{Claim, ClaimType, Store};
use std::fs;

fn mk(who: &str, i: u64) -> Claim {
    Claim::new(
        ClaimType::Signal,
        serde_json::json!({ "seed": i, "topic": format!("t-{}", i % 7) }),
        who,
    )
}

fn sorted_ids(claims: &[Claim]) -> Vec<String> {
    let mut v: Vec<String> = claims.iter().map(|c| c.id.clone()).collect();
    v.sort();
    v
}

fn peers() -> (tempfile::TempDir, Store, Store) {
    let tmp = tempfile::tempdir().unwrap();
    let a_root = tmp.path().join("a/claims");
    let b_root = tmp.path().join("b/claims");
    let a = Store::init(&a_root).unwrap();
    fs::create_dir_all(&b_root).unwrap();
    fs::copy(a_root.join("genesis.amc"), b_root.join("genesis.amc")).unwrap();
    fs::copy(a_root.join("config.toml"), b_root.join("config.toml")).unwrap();
    fs::create_dir_all(b_root.join("changes")).unwrap();
    let b = Store::open(&b_root).unwrap();
    (tmp, a, b)
}

#[test]
fn merge_commutativity_across_many_shapes() {
    // Invariant: for every (k_a, k_b) in a range of sizes, the set of
    // claim ids visible after merge(a, b) equals the set after merge(b, a).
    for (k_a, k_b) in [(1u64, 1u64), (2, 5), (4, 4), (7, 3), (11, 2), (0, 6)] {
        let (_tmp, mut a, mut b) = peers();
        for i in 0..k_a {
            a.append(&mk("user:gitlab:andunn", i)).unwrap();
        }
        for i in 0..k_b {
            b.append(&mk("user:gitlab:joshua", 1_000 + i)).unwrap();
        }
        let a_root = a.root().to_path_buf();
        let b_root = b.root().to_path_buf();

        a.merge(&mut b).unwrap();
        let ids_ab = sorted_ids(&a.load_claims().unwrap());

        let mut a2 = Store::open(&a_root).unwrap();
        let mut b2 = Store::open(&b_root).unwrap();
        b2.merge(&mut a2).unwrap();
        let ids_ba = sorted_ids(&b2.load_claims().unwrap());

        assert_eq!(
            ids_ab, ids_ba,
            "commutativity failed for (k_a={k_a}, k_b={k_b})"
        );
    }
}

#[test]
fn content_hash_append_is_idempotent_via_dedup() {
    // Invariant: appending the same claim twice yields one row in
    // load_claims() (dedup on claim.id per 09g).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    let c = mk("user:gitlab:andunn", 77);
    store.append(&c).unwrap();
    store.append(&c).unwrap();
    store.append(&c).unwrap();
    let loaded = store.load_claims().unwrap();
    assert_eq!(loaded.len(), 1, "duplicate appends must dedup by id");
    assert_eq!(loaded[0].id, c.id);
}

#[test]
fn diamond_supersession_preserves_both_branches() {
    // Both peers supersede the same base claim. Post-merge, both
    // superseder ids must be reachable; base id is deduped.
    let (_tmp, mut a, mut b) = peers();
    let base = mk("user:gitlab:andunn", 0);
    a.append(&base).unwrap();
    b.append(&base).unwrap();
    let from_a = Claim::new(
        ClaimType::Signal,
        serde_json::json!({"from": "a"}),
        "user:gitlab:andunn",
    )
    .with_supersedes(base.id.clone());
    let from_b = Claim::new(
        ClaimType::Signal,
        serde_json::json!({"from": "b"}),
        "user:gitlab:joshua",
    )
    .with_supersedes(base.id.clone());
    a.append(&from_a).unwrap();
    b.append(&from_b).unwrap();
    a.merge(&mut b).unwrap();
    let claims = a.load_claims().unwrap();
    let ids: Vec<&str> = claims.iter().map(|c| c.id.as_str()).collect();
    assert!(ids.contains(&base.id.as_str()), "base must be present");
    assert!(ids.contains(&from_a.id.as_str()), "a's branch must be present");
    assert!(ids.contains(&from_b.id.as_str()), "b's branch must be present");
    assert_eq!(claims.len(), 3, "3 distinct ids after dedup");
}

#[test]
fn reopen_round_trips_many_claim_counts() {
    // Invariant: init + N appends + reopen preserves load_claims() count
    // for every N in a sweep.
    for n in [0u64, 1, 2, 5, 11, 25] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("claims");
        {
            let mut store = Store::init(&root).unwrap();
            for i in 0..n {
                store.append(&mk("user:gitlab:andunn", i)).unwrap();
            }
        }
        let mut reopened = Store::open(&root).unwrap();
        let claims = reopened.load_claims().unwrap();
        assert_eq!(claims.len() as u64, n, "round-trip failed at n={n}");
    }
}

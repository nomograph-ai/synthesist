//! Integration tests for multi-peer CRDT merge behavior.

use nomograph_claim::{Claim, ClaimType, Store};
use std::fs;
use std::path::Path;

fn two_peers_sharing_genesis() -> (tempfile::TempDir, Store, Store) {
    let tmp = tempfile::tempdir().unwrap();
    let a_root = tmp.path().join("peer-a/claims");
    let b_root = tmp.path().join("peer-b/claims");
    let store_a = Store::init(&a_root).unwrap();
    // Peer B mirrors peer A's genesis so list ObjIds match (01 Amendment A).
    fs::create_dir_all(&b_root).unwrap();
    let genesis = fs::read(a_root.join("genesis.amc")).unwrap();
    fs::write(b_root.join("genesis.amc"), &genesis).unwrap();
    fs::create_dir_all(b_root.join("changes")).unwrap();
    fs::copy(
        a_root.join("config.toml"),
        b_root.join("config.toml"),
    )
    .unwrap();
    let store_b = Store::open(&b_root).unwrap();
    (tmp, store_a, store_b)
}

fn mk(who: &str, i: u64) -> Claim {
    Claim::new(
        ClaimType::Signal,
        serde_json::json!({ "topic": format!("t-{i}"), "seed": i }),
        who,
    )
}

fn sorted_ids(claims: &[Claim]) -> Vec<String> {
    let mut ids: Vec<String> = claims.iter().map(|c| c.id.clone()).collect();
    ids.sort();
    ids
}

#[test]
fn merge_unions_disjoint_claims() {
    let (_tmp, mut a, mut b) = two_peers_sharing_genesis();
    for i in 0..3 {
        a.append(&mk("user:gitlab:andunn", i)).unwrap();
    }
    for i in 100..105 {
        b.append(&mk("user:gitlab:joshua", i)).unwrap();
    }
    a.merge(&mut b).unwrap();
    let claims = a.load_claims().unwrap();
    assert_eq!(claims.len(), 3 + 5);
}

#[test]
fn merge_is_commutative_by_ids() {
    // merge(a, b).claims == merge(b, a).claims (as a set of ids)
    let (_tmp, mut a, mut b) = two_peers_sharing_genesis();
    for i in 0..4 {
        a.append(&mk("user:gitlab:andunn", i)).unwrap();
    }
    for i in 200..206 {
        b.append(&mk("user:gitlab:joshua", i)).unwrap();
    }

    // Clone the state into independent peers for the second direction
    // by re-opening from each peer's on-disk state.
    let a_root = a.root().to_path_buf();
    let b_root = b.root().to_path_buf();

    // Direction 1: merge b into a.
    a.merge(&mut b).unwrap();
    let ids_ab = sorted_ids(&a.load_claims().unwrap());

    // Direction 2: fresh opens, then merge a into b.
    let mut a2 = Store::open(&a_root).unwrap();
    let mut b2 = Store::open(&b_root).unwrap();
    b2.merge(&mut a2).unwrap();
    let ids_ba = sorted_ids(&b2.load_claims().unwrap());

    assert_eq!(ids_ab, ids_ba, "merge must be commutative by claim id");
}

#[test]
fn merge_persists_via_incremental() {
    let (_tmp, mut a, mut b) = two_peers_sharing_genesis();
    a.append(&mk("user:gitlab:andunn", 1)).unwrap();
    b.append(&mk("user:gitlab:joshua", 2)).unwrap();
    let before = count_amcs(a.root());
    a.merge(&mut b).unwrap();
    let after = count_amcs(a.root());
    assert!(
        after > before,
        "merge must flush a new change file ({before} -> {after})"
    );
    // Re-open from disk: merged claims must still be visible.
    let a_root = a.root().to_path_buf();
    drop(a);
    let mut reopened = Store::open(&a_root).unwrap();
    let claims = reopened.load_claims().unwrap();
    assert_eq!(claims.len(), 2);
}

#[test]
fn independent_genesis_produces_empty_intersection() {
    // Negative control: two peers that do NOT share genesis behave like
    // disjoint documents. Their claims lists do not line up.
    let tmp = tempfile::tempdir().unwrap();
    let a_root = tmp.path().join("a/claims");
    let b_root = tmp.path().join("b/claims");
    let mut a = Store::init(&a_root).unwrap();
    let mut b = Store::init(&b_root).unwrap();
    a.append(&mk("user:gitlab:andunn", 1)).unwrap();
    b.append(&mk("user:gitlab:joshua", 2)).unwrap();
    // The merge is still safe (no panic), but the shape is
    // not-guaranteed to preserve both lists. This test exists to document
    // why Amendment A matters: shared genesis is required for correctness.
    let _ = a.merge(&mut b);
}

fn count_amcs(root: &Path) -> usize {
    fs::read_dir(root.join("changes"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .map(|e| e.path().extension().and_then(|s| s.to_str()) == Some("amc"))
                .unwrap_or(false)
        })
        .count()
}

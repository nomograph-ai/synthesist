//! Dedup contract: appending the same claim content twice projects to
//! exactly one row. Mirrors 09g "minor finding".

use nomograph_claim::{Claim, ClaimType, Store, View};

fn mk() -> Claim {
    Claim::new(
        ClaimType::Disposition,
        serde_json::json!({ "topic": "roadmap", "stance": "favors" }),
        "user:gitlab:andunn",
    )
}

#[test]
fn duplicate_append_projects_single_row() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    let c = mk();
    store.append(&c).unwrap();
    store.append(&c).unwrap();
    store.append(&c).unwrap();
    let mut view = View::open(&root).unwrap();
    view.rebuild(&mut store).unwrap();
    let rows = view
        .query("SELECT id FROM claims WHERE id = ?1", &[&c.id])
        .unwrap();
    assert_eq!(rows.len(), 1, "INSERT OR IGNORE must collapse duplicates");
    assert_eq!(rows[0]["id"], serde_json::json!(c.id));
}

#[test]
fn total_rows_match_distinct_ids_after_redundant_work() {
    // Mix distinct claims with redundant re-appends, then verify that
    // the projection row count is the distinct id count — not the raw
    // append count.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    let a = Claim::new(
        ClaimType::Signal,
        serde_json::json!({ "k": "a" }),
        "user:gitlab:andunn",
    );
    let b = Claim::new(
        ClaimType::Signal,
        serde_json::json!({ "k": "b" }),
        "user:gitlab:andunn",
    );
    for _ in 0..3 {
        store.append(&a).unwrap();
    }
    store.append(&b).unwrap();
    for _ in 0..2 {
        store.append(&a).unwrap();
    }

    let mut view = View::open(&root).unwrap();
    view.rebuild(&mut store).unwrap();
    let rows = view.query("SELECT COUNT(*) AS n FROM claims", &[]).unwrap();
    assert_eq!(rows[0]["n"], serde_json::json!(2));
}

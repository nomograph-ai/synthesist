//! Integration tests for [`View::open`] and [`View::rebuild`] basics.

use nomograph_claim::{Claim, ClaimType, Error, Store, View};

fn mk(i: u64) -> Claim {
    Claim::new(
        ClaimType::Spec,
        serde_json::json!({ "goal": format!("spec-{i}") }),
        "user:gitlab:andunn",
    )
}

#[test]
fn open_creates_view_sqlite_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let _store = Store::init(&root).unwrap();
    let view = View::open(&root).unwrap();
    assert!(view.db_path().exists(), "open must create view.sqlite");
    assert_eq!(view.db_path().file_name().unwrap(), "view.sqlite");
}

#[test]
fn open_does_not_populate_rows() {
    // Constructing the View alone must NOT backfill; callers control
    // when the rebuild cost is paid.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    store.append(&mk(1)).unwrap();
    drop(store);
    let view = View::open(&root).unwrap();
    let rows = view.query("SELECT COUNT(*) AS n FROM claims", &[]).unwrap();
    assert_eq!(rows[0]["n"], serde_json::json!(0));
}

#[test]
fn rebuild_from_empty_store_produces_zero_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    let mut view = View::open(&root).unwrap();
    view.rebuild(&mut store).unwrap();
    let rows = view.query("SELECT COUNT(*) AS n FROM claims", &[]).unwrap();
    assert_eq!(rows[0]["n"], serde_json::json!(0));
}

#[test]
fn rebuild_after_append_surfaces_one_row() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    let claim = mk(1);
    store.append(&claim).unwrap();
    let mut view = View::open(&root).unwrap();
    view.rebuild(&mut store).unwrap();
    let rows = view
        .query(
            "SELECT id, claim_type, asserted_by FROM claims",
            &[],
        )
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], serde_json::json!(claim.id));
    assert_eq!(rows[0]["claim_type"], serde_json::json!("spec"));
    assert_eq!(rows[0]["asserted_by"], serde_json::json!("user:gitlab:andunn"));
}

#[test]
fn open_on_missing_dir_returns_typed_error() {
    // Blank-node: opening a View for a dir that doesn't exist must
    // surface a specific error variant, not panic.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("never-created");
    let err = match View::open(&root) {
        Ok(_) => panic!("expected View::open to fail on missing dir"),
        Err(e) => e,
    };
    match err {
        Error::MissingGenesis(path) => {
            assert!(path.contains("never-created"), "path was: {path}");
        }
        other => panic!("wrong error variant: {other}"),
    }
}

#[test]
fn open_on_file_instead_of_dir_is_corrupt_error() {
    let tmp = tempfile::tempdir().unwrap();
    let not_a_dir = tmp.path().join("claims");
    std::fs::write(&not_a_dir, b"not a directory").unwrap();
    let err = match View::open(&not_a_dir) {
        Ok(_) => panic!("expected View::open to reject non-directory"),
        Err(e) => e,
    };
    assert!(matches!(err, Error::Corrupt(_)), "err was: {err}");
}

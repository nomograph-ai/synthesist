//! Integration tests for the `Store` happy-path lifecycle.

use nomograph_claim::{Claim, ClaimType, Error, Store};
use std::fs;

fn mk(i: u64) -> Claim {
    Claim::new(
        ClaimType::Spec,
        serde_json::json!({ "goal": format!("spec-{i}") }),
        "user:gitlab:andunn",
    )
}

#[test]
fn init_creates_genesis_and_config() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let _store = Store::init(&root).unwrap();
    assert!(root.join("genesis.amc").exists());
    assert!(root.join("config.toml").exists());
    assert!(root.join("changes").is_dir());
    let cfg = fs::read_to_string(root.join("config.toml")).unwrap();
    assert!(
        cfg.contains("schema_version = \"0.1\""),
        "unexpected config: {cfg}"
    );
}

#[test]
fn append_writes_one_change_file_per_claim() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    for i in 0..5 {
        store.append(&mk(i)).unwrap();
    }
    let count = fs::read_dir(root.join("changes"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .map(|e| e.path().extension().and_then(|s| s.to_str()) == Some("amc"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(count, 5, "one .amc per append");
}

#[test]
fn reopen_preserves_claims() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    {
        let mut store = Store::init(&root).unwrap();
        for i in 0..7 {
            store.append(&mk(i)).unwrap();
        }
    }
    let mut reopened = Store::open(&root).unwrap();
    let claims = reopened.load_claims().unwrap();
    assert_eq!(claims.len(), 7);
}

#[test]
fn missing_genesis_surfaces_typed_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    fs::create_dir_all(&root).unwrap();
    let err = match Store::open(&root) {
        Ok(_) => panic!("expected open to fail"),
        Err(e) => e,
    };
    match err {
        Error::MissingGenesis(path) => {
            assert!(path.contains("genesis.amc"), "path was: {path}");
        }
        other => panic!("wrong error variant: {other}"),
    }
}

#[test]
fn claim_round_trips_all_optional_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    let claim = mk(42)
        .with_supersedes("prior-id".to_string())
        .with_parent_asserter("user:gitlab:delegator".to_string());
    store.append(&claim).unwrap();
    let loaded = store.load_claims().unwrap();
    assert_eq!(loaded.len(), 1);
    let got = &loaded[0];
    assert_eq!(got.id, claim.id);
    assert_eq!(got.claim_type, ClaimType::Spec);
    assert_eq!(got.supersedes.as_deref(), Some("prior-id"));
    assert_eq!(got.parent_asserter.as_deref(), Some("user:gitlab:delegator"));
}

#[test]
fn save_incremental_is_noop_when_nothing_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    store.append(&mk(1)).unwrap();
    let again = store.save_incremental().unwrap();
    assert!(again.is_none(), "expected no new change file");
}

#[test]
fn heads_advance_after_append() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();
    let h0 = store.heads();
    store.append(&mk(1)).unwrap();
    let h1 = store.heads();
    assert_ne!(h0, h1, "heads must advance after append");
}

#[test]
fn open_survives_corrupt_change_file() {
    // Regression for ADVERSARIAL-REVIEW HIGH #5. Write several claims,
    // then corrupt one `.amc` in `changes/`. Store::open must not fail;
    // it should log and skip the corrupt file.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    {
        let mut store = Store::init(&root).unwrap();
        store.append(&mk(1)).unwrap();
        store.append(&mk(2)).unwrap();
        store.append(&mk(3)).unwrap();
    }

    // Find one of the change files and truncate it to partial bytes.
    let changes = root.join("changes");
    let first: std::path::PathBuf = fs::read_dir(&changes)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|s| s.to_str()) == Some("amc"))
        .expect("at least one .amc in changes/");
    fs::write(&first, b"\x00\x01\x02\x03garbage").unwrap();

    // Reopen must succeed despite the corrupt file. The contract is
    // "open doesn't crash"; claim recovery is best-effort because
    // Automerge changes are a causal graph: if the corrupt change was
    // a dependency of later changes, those later changes may also fail
    // to apply. That loss is acceptable — the change wasn't yet on
    // beacon. The unacceptable outcome (fixed by HIGH #5) is an
    // unopenable store, and that's what this regression pins.
    let mut store = Store::open(&root).expect("reopen survives corrupt .amc");
    // load_claims() also must not panic.
    let _ = store.load_claims().unwrap();
}

#[test]
fn concurrent_appends_serialize_via_lock() {
    // Regression for ADVERSARIAL-REVIEW CRITICAL #2. Spawn N threads,
    // each opens a separate Store handle on the same claims/ and appends
    // M claims. Under the lockfile, every claim must persist. Without
    // the lockfile, the Automerge in-memory state could diverge and a
    // concurrent compact could wipe fresh changes.
    use std::sync::Arc;
    use std::thread;

    let tmp = tempfile::tempdir().unwrap();
    let root = Arc::new(tmp.path().join("claims"));
    // Seed genesis so everyone can just open().
    drop(Store::init(&*root).unwrap());

    const THREADS: usize = 4;
    const PER: u64 = 8;
    let mut handles = Vec::with_capacity(THREADS);
    for t in 0..THREADS {
        let root = Arc::clone(&root);
        handles.push(thread::spawn(move || {
            let mut store = Store::open(&*root).expect("open in thread");
            for i in 0..PER {
                let claim = Claim::new(
                    ClaimType::Spec,
                    serde_json::json!({"t": t, "i": i}),
                    format!("user:gitlab:thread-{t}"),
                );
                store.append(&claim).expect("append in thread");
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    // Reopen and verify: every (t, i) claim must be present.
    let mut store = Store::open(&*root).unwrap();
    let claims = store.load_claims().unwrap();
    assert_eq!(
        claims.len(),
        THREADS * PER as usize,
        "expected every concurrent append to persist"
    );
}

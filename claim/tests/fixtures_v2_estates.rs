//! Real-shape v2 estate fixture generator + v2 read-path validation
//! for the v2-to-v3 migration (issue #11).
//!
//! Issue #11: v3 migration detection reports REAL v2 estates as "fresh"
//! because it gates on `claims/changes/` existing. A production v2.5.2
//! estate is COMPACTED: `claims/` has genesis.amc + snapshot.amc +
//! config.toml and NO changes/ dir. The earlier migration tests only
//! ever built fixtures via Store::init+append, which ALWAYS create
//! changes/, so the compacted shape was never exercised.
//!
//! This file deterministically (re)generates the committed fixtures under
//! `claim/tests/fixtures/v2_estates/`:
//!
//!   - compacted/    genesis.amc + snapshot.amc + config.toml, NO changes/
//!   - with_changes/ genesis.amc + changes/*.amc + config.toml
//!
//! and then VALIDATES the v2 read path against the compacted estate: it
//! opens the compacted estate via `Store::open` and asserts `load_claims`
//! returns the FULL claim count. If it returns fewer, the shim drops
//! snapshot.amc content -- the suspected 4th failure.
//!
//! Determinism: all timestamps are FIXED (chrono `with_ymd_and_hms`), so the
//! content-hashed claim IDS are reproducible. The on-disk Automerge BYTES are
//! NOT byte-reproducible: `Store::init` mints a random Automerge actor id per
//! call, so `genesis.amc`, the `changes/<hash>.amc` filenames, and
//! `snapshot.amc` vary run to run. The committed fixtures are therefore the
//! source of truth and must NOT be rewritten by a routine `cargo test` --
//! doing so dirties the working tree with a non-deterministic diff and breaks
//! git-clean CI gates. Accordingly:
//!
//!   - The default tests are READ-ONLY: they open the COMMITTED fixtures
//!     through the production read path and assert shape + claim count. They
//!     never write into `fixtures_root()`.
//!   - To regenerate the committed fixtures (after an intentional fixture
//!     change), run with `REGEN_V2_FIXTURES=1`; the regenerator writes the
//!     fixtures in place. Commit the result deliberately.

#![allow(deprecated)]

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use nomograph_claim::{Claim, ClaimType, Store};
use serde_json::json;

/// `claims/config.toml` content written by v2.5.2.
const CONFIG_TOML: &str = "schema_version = \"0.1\"\n";

/// Absolute path to `claim/tests/fixtures/v2_estates/`.
fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("v2_estates")
}

/// Fixed timestamp helper.
fn ts(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
}

/// Build a claim with FIXED timestamps so the content hash (and thus the
/// on-disk bytes) is reproducible. Mirrors `Claim::new` but without
/// `Utc::now()`.
fn fixed_claim(
    claim_type: ClaimType,
    props: serde_json::Value,
    asserted_by: &str,
    at: DateTime<Utc>,
) -> Claim {
    let id = Claim::compute_id(&claim_type, &props, at, asserted_by, at);
    Claim {
        id,
        claim_type,
        props,
        valid_from: at,
        valid_until: None,
        supersedes: None,
        parent_asserter: None,
        asserted_by: asserted_by.to_string(),
        asserted_at: at,
    }
}

/// The canonical, deterministic claim set for the v2 fixtures: six claims
/// of MIXED synthesist-owned types (Task, Spec, Tree, Outcome, Session,
/// Discovery). NOT lattice types (Stakeholder/Topic/Signal/Disposition).
fn fixture_claims() -> Vec<Claim> {
    vec![
        fixed_claim(
            ClaimType::Tree,
            json!({ "name": "synthesist", "root": true }),
            "user:local:agd",
            ts(2026, 1, 2, 9, 0, 0),
        ),
        fixed_claim(
            ClaimType::Spec,
            json!({ "title": "v2->v3 migration", "status": "draft" }),
            "user:local:agd",
            ts(2026, 1, 2, 9, 5, 0),
        ),
        fixed_claim(
            ClaimType::Task,
            json!({ "summary": "detect compacted estates", "state": "open" }),
            "user:local:agd",
            ts(2026, 1, 2, 9, 10, 0),
        ),
        fixed_claim(
            ClaimType::Session,
            json!({ "label": "morning", "agent": "claude" }),
            "user:local:jkolb",
            ts(2026, 1, 2, 9, 15, 0),
        ),
        fixed_claim(
            ClaimType::Outcome,
            json!({ "result": "snapshot read validated", "ok": true }),
            "user:local:jkolb",
            ts(2026, 1, 2, 9, 20, 0),
        ),
        fixed_claim(
            ClaimType::Discovery,
            json!({ "finding": "changes/ absent after compaction" }),
            "user:local:agd",
            ts(2026, 1, 2, 9, 25, 0),
        ),
    ]
}

/// Build an un-compacted estate (genesis + changes/*.amc + config.toml)
/// at `root` from the fixed claim set. Returns the open Store.
fn build_with_changes(root: &Path) -> Store {
    if root.exists() {
        fs::remove_dir_all(root).unwrap();
    }
    let mut store = Store::init(root).unwrap();
    for claim in fixture_claims() {
        store.append(&claim).unwrap();
    }
    fs::write(root.join("config.toml"), CONFIG_TOML).unwrap();
    store
}

/// True when the regenerator should run (env opt-in only).
fn regen_requested() -> bool {
    std::env::var_os("REGEN_V2_FIXTURES").is_some()
}

/// READ-ONLY validation of the committed `with_changes` fixture. Opens it
/// through the production read path and asserts shape + claim count. Does NOT
/// write into the source tree (see module docs: bytes are not reproducible).
///
/// When `REGEN_V2_FIXTURES=1` it FIRST regenerates the committed fixture in
/// place, then validates -- the deliberate refresh path.
#[test]
fn with_changes_fixture_shape_and_roundtrip() {
    let root = fixtures_root().join("with_changes").join("claims");
    if regen_requested() {
        build_with_changes(&root);
    }

    // Shape assertions: genesis + changes/ + config.toml, no snapshot.
    assert!(root.join("genesis.amc").exists(), "genesis.amc present");
    assert!(root.join("changes").is_dir(), "changes/ present");
    assert!(root.join("config.toml").exists(), "config.toml present");
    assert!(
        !root.join("snapshot.amc").exists(),
        "un-compacted estate has no snapshot.amc"
    );
    let n_changes = fs::read_dir(root.join("changes")).unwrap().count();
    assert_eq!(n_changes, 6, "one change file per appended claim");

    // Round-trip: read back all six.
    let mut store = Store::open(&root).unwrap();
    let claims = store.load_claims().unwrap();
    assert_eq!(claims.len(), 6, "with_changes estate yields 6 claims");
}

/// Build a compacted estate at `root` (init+append the fixed data, then
/// compact). Used by the regenerator and by the read-path probe (which builds
/// into a tempdir). NEVER call this against `fixtures_root()` outside the
/// regen path -- it rewrites non-reproducible bytes.
fn build_compacted_at(root: &Path) {
    build_with_changes(root);
    let mut store = Store::open(root).unwrap();
    store.compact().unwrap();
}

/// READ-ONLY validation of the committed `compacted` fixture (the issue #11
/// shape: genesis + snapshot, NO changes/). Regenerates in place only under
/// `REGEN_V2_FIXTURES=1`.
#[test]
fn compacted_fixture_shape() {
    let root = fixtures_root().join("compacted").join("claims");
    if regen_requested() {
        build_compacted_at(&root);
    }

    // Shape assertions: genesis + snapshot + config.toml, NO changes/.
    assert!(root.join("genesis.amc").exists(), "genesis.amc present");
    assert!(root.join("snapshot.amc").exists(), "snapshot.amc present");
    assert!(root.join("config.toml").exists(), "config.toml present");
    assert!(
        !root.join("changes").exists(),
        "compacted estate has NO changes/ dir (the issue #11 shape)"
    );

    // Round-trip the COMMITTED snapshot bytes through the read path.
    let mut store = Store::open(&root).unwrap();
    let claims = store.load_claims().unwrap();
    assert_eq!(
        claims.len(),
        6,
        "committed compacted fixture must yield all 6 claims through Store::open"
    );
}

/// THE 4TH-FAILURE PROBE.
///
/// Open the COMPACTED estate (genesis + snapshot, no changes/) through the
/// production read path (`Store::open`) and assert it yields all six
/// claims. The compacted snapshot is written by `compact()` as
/// `self.doc.save()` (a FULL automerge document), then read back via
/// `Store::open` -> `doc.load_incremental(&snapshot_bytes)`. If
/// `load_incremental` rejects or partially applies a full `save()`
/// payload, `Store::open` silently falls back to genesis-only and
/// load_claims returns FEWER than six -> that is the 4th failure.
#[test]
fn compacted_estate_read_path_yields_all_claims() {
    // Build into an ISOLATED tempdir, not the committed fixture path, so
    // this read-path probe never races `generate_compacted_fixture` under
    // parallel test execution (both would otherwise rewrite the same dir).
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("claims");
    build_compacted_at(&root);
    assert!(!root.join("changes").exists());
    assert!(root.join("snapshot.amc").exists());

    let mut store = Store::open(&root).unwrap();
    let claims = store.load_claims().unwrap();
    eprintln!(
        "compacted read path: load_claims returned {} claims (expected 6)",
        claims.len()
    );
    for c in &claims {
        eprintln!("  - {} {} {}", c.claim_type.as_str(), c.id, c.asserted_by);
    }
    assert_eq!(
        claims.len(),
        6,
        "compacted estate (snapshot-only) must yield all 6 claims through Store::open; \
         fewer means the v2 shim drops snapshot.amc content (issue #11 4th failure)"
    );
}

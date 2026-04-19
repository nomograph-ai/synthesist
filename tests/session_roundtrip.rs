//! Session lifecycle integration test: start -> tag -> close -> list_live.

use nomograph_claim::session::{Session, SessionClaim};
use nomograph_claim::{Claim, ClaimType, Store};

#[test]
fn session_start_tag_close_yields_no_live_sessions() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("claims");
    let mut store = Store::init(&root).unwrap();

    let handle = Session::start(
        &mut store,
        "sess-rt-1",
        "user:gitlab:andunn",
        Some("keaton"),
        Some("sch-1"),
        Some("roundtrip"),
    )
    .expect("start");

    // list_live sees exactly this session while it's open.
    let live_before: Vec<SessionClaim> =
        Session::list_live(&mut store).expect("list_live open");
    assert_eq!(live_before.len(), 1, "one live session after start");
    assert_eq!(live_before[0].id, "sess-rt-1");
    assert_eq!(live_before[0].asserter_base, "user:gitlab:andunn");
    assert_eq!(live_before[0].tree.as_deref(), Some("keaton"));
    assert_eq!(live_before[0].spec.as_deref(), Some("sch-1"));
    assert_eq!(live_before[0].summary.as_deref(), Some("roundtrip"));

    // Tag a Spec claim and append it under the session's asserter.
    let raw = Claim::new(
        ClaimType::Spec,
        serde_json::json!({
            "tree": "keaton",
            "id": "sch-1",
            "goal": "demo session tagging",
            "status": "active",
            "topics": ["schema"],
        }),
        "user:gitlab:andunn",
    );
    let original_id = raw.id.clone();
    let tagged = handle.tag(raw);
    assert_eq!(tagged.asserted_by, "user:gitlab:andunn:sess-rt-1");
    assert_ne!(
        tagged.id, original_id,
        "tag must recompute id because asserted_by changed",
    );
    store.append(&tagged).expect("append tagged");

    // Close the session and confirm no live sessions remain.
    handle.close(&mut store).expect("close");
    let live_after: Vec<SessionClaim> =
        Session::list_live(&mut store).expect("list_live closed");
    assert!(
        live_after.is_empty(),
        "expected no live sessions after close, got {}",
        live_after.len()
    );
}

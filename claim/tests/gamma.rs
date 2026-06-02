//! Unit tests for the gamma index typed query surface (H1-H10).
//!
//! Each H-helper is exercised against a small in-code fixture written
//! through the real [`LogWriter`], indexed in memory, and queried via
//! the typed helper. The fixtures use the synthesist vocabulary the
//! helpers will see in production (compact `synthesist:` predicates),
//! but the helpers themselves are vocabulary-agnostic: the type and
//! predicate strings are passed in by the caller.

use nomograph_claim::gamma::Gamma;
use nomograph_claim::log::LogWriter;
use serde_json::{Value, json};
use tempfile::TempDir;

// -- vocabulary the fixtures use (synthesist's, passed to each helper) --
const TASK: &str = "synthesist:Task";
const SPEC: &str = "synthesist:Spec";
const SESSION: &str = "synthesist:Session";
const PHASE: &str = "synthesist:Phase";
const SUPERSEDES: &str = "synthesist:supersedes";
const SESSION_ID: &str = "synthesist:sessionId";
const AGREE: &str = "synthesist:agreeSnapshot";
const ACCEPTANCE: &str = "synthesist:acceptance";
const DEPENDS: &str = "synthesist:dependsOn";
const FILES: &str = "synthesist:files";

/// Build an in-memory gamma index over claims written to a temp dir.
struct Fixture {
    _tmp: TempDir,
    writer: LogWriter,
    claims_dir: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let claims_dir = tmp.path().to_path_buf();
        let writer = LogWriter::new(&claims_dir).unwrap();
        Self {
            _tmp: tmp,
            writer,
            claims_dir,
        }
    }

    fn write(&self, asserter: &str, doc: &Value) {
        self.writer.append(asserter, doc).unwrap();
    }

    /// Build a fresh in-memory index synced to the current log union.
    fn index(&self) -> Gamma {
        let mut g = Gamma::open_in_memory().unwrap();
        g.sync(&self.claims_dir).unwrap();
        g
    }
}

/// A minimal valid claim. `extra` merges in module-specific predicates.
fn claim(id: &str, ty: &str, at: &str, extra: Value) -> Value {
    let mut doc = json!({
        "@context": {
            "synthesist": "https://nomograph.org/synthesist/",
            "prov": "http://www.w3.org/ns/prov#",
            "prov:generatedAtTime": {"@type": "xsd:dateTime"},
            "prov:wasAttributedTo": {"@type": "@id"}
        },
        "@id": format!("synthesist:claim/{id}"),
        "@type": ty,
        "prov:generatedAtTime": at,
        "prov:wasAttributedTo": "asserter:user:local:agd",
    });
    if let Value::Object(extra_map) = extra {
        let obj = doc.as_object_mut().unwrap();
        for (k, v) in extra_map {
            obj.insert(k, v);
        }
    }
    doc
}

fn cid(id: &str) -> String {
    format!("synthesist:claim/{id}")
}

// ----------------------------------------------------------------------
// H1 -- count_by_type / count_total (NOT live-filtered).
// ----------------------------------------------------------------------

#[test]
fn h1_count_by_type_counts_all_including_superseded() {
    let f = Fixture::new();
    f.write("user:local:agd", &claim("t1", TASK, "2026-05-29T00:00:00.000Z", json!({})));
    f.write("user:local:agd", &claim("t2", TASK, "2026-05-29T00:00:01.000Z", json!({})));
    // t3 supersedes t1 -- still counted by H1 (not live-filtered).
    f.write(
        "user:local:agd",
        &claim("t3", TASK, "2026-05-29T00:00:02.000Z", json!({SUPERSEDES: cid("t1")})),
    );
    f.write("user:local:agd", &claim("s1", SPEC, "2026-05-29T00:00:03.000Z", json!({})));

    let g = f.index();
    assert_eq!(g.count_by_type(TASK).unwrap(), 3, "all 3 tasks, superseded included");
    assert_eq!(g.count_by_type(SPEC).unwrap(), 1);
    assert_eq!(g.count_total().unwrap(), 4);
}

// ----------------------------------------------------------------------
// H2 -- live_heads (the dominant live-head anti-join).
// ----------------------------------------------------------------------

#[test]
fn h2_live_heads_excludes_superseded() {
    let f = Fixture::new();
    f.write("user:local:agd", &claim("t1", TASK, "2026-05-29T00:00:00.000Z", json!({})));
    f.write("user:local:agd", &claim("t2", TASK, "2026-05-29T00:00:01.000Z", json!({})));
    f.write(
        "user:local:agd",
        &claim("t3", TASK, "2026-05-29T00:00:02.000Z", json!({SUPERSEDES: cid("t1")})),
    );

    let g = f.index();
    let live = g.live_heads(TASK, SUPERSEDES).unwrap();
    assert_eq!(live, vec![cid("t2"), cid("t3")], "t1 superseded by t3, dropped");
}

#[test]
fn h2_scalar_reads_status_via_pso() {
    let f = Fixture::new();
    f.write(
        "user:local:agd",
        &claim("t1", TASK, "2026-05-29T00:00:00.000Z", json!({"synthesist:status": "pending"})),
    );
    let g = f.index();
    assert_eq!(
        g.scalar(&cid("t1"), "synthesist:status").unwrap(),
        Some("pending".to_string())
    );
}

// ----------------------------------------------------------------------
// H3 -- live_tasks with native dependsOn / files vectors.
// ----------------------------------------------------------------------

#[test]
fn h3_live_tasks_returns_native_vectors() {
    let f = Fixture::new();
    f.write(
        "user:local:agd",
        &claim(
            "t1",
            TASK,
            "2026-05-29T00:00:00.000Z",
            json!({
                "synthesist:status": "ready",
                DEPENDS: [cid("t0"), cid("tx")],
                FILES: ["src/a.rs", "src/b.rs"]
            }),
        ),
    );
    // A superseded task must not appear.
    f.write("user:local:agd", &claim("told", TASK, "2026-05-29T00:00:00.500Z", json!({})));
    f.write(
        "user:local:agd",
        &claim("tnew", TASK, "2026-05-29T00:00:01.000Z", json!({SUPERSEDES: cid("told")})),
    );

    let g = f.index();
    let tasks = g.live_tasks(TASK, SUPERSEDES, "synthesist:status", DEPENDS, FILES).unwrap();
    let t1 = tasks.iter().find(|t| t.id == cid("t1")).expect("t1 present");
    assert_eq!(t1.status.as_deref(), Some("ready"));
    assert_eq!(t1.depends_on, vec![cid("t0"), cid("tx")]);
    assert_eq!(t1.files, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
    assert!(!tasks.iter().any(|t| t.id == cid("told")), "superseded task excluded");
}

// ----------------------------------------------------------------------
// H4 -- live_session_openers / session_opener_by_id (dual anti-join).
// ----------------------------------------------------------------------

#[test]
fn h4_session_openers_separates_opener_from_closer() {
    let f = Fixture::new();
    // opener: a Session claim with sessionId=s1, supersedes nothing.
    f.write(
        "user:local:agd",
        &claim("open1", SESSION, "2026-05-29T00:00:00.000Z", json!({SESSION_ID: "s1"})),
    );
    // closer: a Session claim with the same sessionId that supersedes the opener.
    f.write(
        "user:local:agd",
        &claim(
            "close1",
            SESSION,
            "2026-05-29T00:00:05.000Z",
            json!({SESSION_ID: "s1", SUPERSEDES: cid("open1")}),
        ),
    );
    // a live opener for a different, still-open session.
    f.write(
        "user:local:agd",
        &claim("open2", SESSION, "2026-05-29T00:00:06.000Z", json!({SESSION_ID: "s2"})),
    );

    let g = f.index();
    // open1 is superseded (closed); close1 supersedes (a closer); only open2 is a live opener.
    let openers = g.live_session_openers(SESSION, SUPERSEDES).unwrap();
    assert_eq!(openers, vec![cid("open2")]);

    assert_eq!(
        g.session_opener_by_id(SESSION, SUPERSEDES, SESSION_ID, "s2").unwrap(),
        Some(cid("open2"))
    );
    assert_eq!(
        g.session_opener_by_id(SESSION, SUPERSEDES, SESSION_ID, "s1").unwrap(),
        None,
        "s1 has no live opener (it was closed)"
    );
}

// ----------------------------------------------------------------------
// H5 -- session_is_live.
// ----------------------------------------------------------------------

#[test]
fn h5_session_is_live() {
    let f = Fixture::new();
    f.write(
        "user:local:agd",
        &claim("open1", SESSION, "2026-05-29T00:00:00.000Z", json!({SESSION_ID: "live"})),
    );
    f.write(
        "user:local:agd",
        &claim("open2", SESSION, "2026-05-29T00:00:01.000Z", json!({SESSION_ID: "dead"})),
    );
    f.write(
        "user:local:agd",
        &claim(
            "close2",
            SESSION,
            "2026-05-29T00:00:02.000Z",
            json!({SESSION_ID: "dead", SUPERSEDES: cid("open2")}),
        ),
    );

    let g = f.index();
    assert!(g.session_is_live(SESSION, SUPERSEDES, SESSION_ID, "live").unwrap());
    assert!(!g.session_is_live(SESSION, SUPERSEDES, SESSION_ID, "dead").unwrap());
    assert!(!g.session_is_live(SESSION, SUPERSEDES, SESSION_ID, "missing").unwrap());
}

// ----------------------------------------------------------------------
// H6 -- current_phase (head of the phase chain for a session).
// ----------------------------------------------------------------------

#[test]
fn h6_current_phase_walks_supersession_chain() {
    let f = Fixture::new();
    // phase chain for session "sx": p1 -> p2 -> p3 (p3 is the head).
    f.write(
        "user:local:agd",
        &claim("p1", PHASE, "2026-05-29T00:00:00.000Z", json!({SESSION_ID: "sx"})),
    );
    f.write(
        "user:local:agd",
        &claim(
            "p2",
            PHASE,
            "2026-05-29T00:00:01.000Z",
            json!({SESSION_ID: "sx", SUPERSEDES: cid("p1")}),
        ),
    );
    f.write(
        "user:local:agd",
        &claim(
            "p3",
            PHASE,
            "2026-05-29T00:00:02.000Z",
            json!({SESSION_ID: "sx", SUPERSEDES: cid("p2")}),
        ),
    );
    // a phase for a different session must not leak in.
    f.write(
        "user:local:agd",
        &claim("q1", PHASE, "2026-05-29T00:00:03.000Z", json!({SESSION_ID: "sy"})),
    );

    let g = f.index();
    assert_eq!(
        g.current_phase(PHASE, SUPERSEDES, SESSION_ID, "sx").unwrap(),
        Some(cid("p3"))
    );
    assert_eq!(
        g.current_phase(PHASE, SUPERSEDES, SESSION_ID, "sy").unwrap(),
        Some(cid("q1"))
    );
    assert_eq!(
        g.current_phase(PHASE, SUPERSEDES, SESSION_ID, "none").unwrap(),
        None
    );
}

// ----------------------------------------------------------------------
// H7 -- dangling_supersedes (orphan supersede targets).
// ----------------------------------------------------------------------

#[test]
fn h7_dangling_supersedes_flags_orphan_targets() {
    let f = Fixture::new();
    f.write("user:local:agd", &claim("t1", TASK, "2026-05-29T00:00:00.000Z", json!({})));
    // t2 supersedes an existing claim (t1): not dangling.
    f.write(
        "user:local:agd",
        &claim("t2", TASK, "2026-05-29T00:00:01.000Z", json!({SUPERSEDES: cid("t1")})),
    );
    // t3 supersedes a claim that was never written: dangling.
    f.write(
        "user:local:agd",
        &claim("t3", TASK, "2026-05-29T00:00:02.000Z", json!({SUPERSEDES: cid("ghost")})),
    );

    let g = f.index();
    let dangling = g.dangling_supersedes(SUPERSEDES).unwrap();
    assert_eq!(dangling.len(), 1);
    assert_eq!(dangling[0].superseder, cid("t3"));
    assert_eq!(dangling[0].target, cid("ghost"));
}

// ----------------------------------------------------------------------
// H8 -- task_acceptance (nested array fetched from the doc).
// ----------------------------------------------------------------------

#[test]
fn h8_task_acceptance_reads_nested_array_from_doc() {
    let f = Fixture::new();
    f.write(
        "user:local:agd",
        &claim(
            "t1",
            TASK,
            "2026-05-29T00:00:00.000Z",
            json!({
                ACCEPTANCE: [
                    {"criterion": "builds", "verifyCmd": "cargo build"},
                    {"criterion": "tests pass", "verifyCmd": "cargo test"}
                ]
            }),
        ),
    );

    let g = f.index();
    let crits = g.task_acceptance(&cid("t1"), ACCEPTANCE).unwrap();
    assert_eq!(crits.len(), 2, "both nested criteria read from the doc");
    assert_eq!(crits[0].criterion, "builds");
    assert_eq!(crits[0].verify_cmd, "cargo build");
    assert_eq!(crits[1].criterion, "tests pass");
    assert_eq!(crits[1].verify_cmd, "cargo test");

    // A task with no acceptance returns an empty Vec.
    f.write("user:local:agd", &claim("t2", TASK, "2026-05-29T00:00:01.000Z", json!({})));
    let g = f.index();
    assert!(g.task_acceptance(&cid("t2"), ACCEPTANCE).unwrap().is_empty());
}

// ----------------------------------------------------------------------
// H9 -- diamond_conflicts (>1 distinct live superseder of one prior).
// ----------------------------------------------------------------------

#[test]
fn h9_diamond_conflicts_flags_multiple_live_superseders() {
    let f = Fixture::new();
    f.write("user:local:agd", &claim("prior", TASK, "2026-05-29T00:00:00.000Z", json!({})));
    // two distinct, both-live claims supersede the same prior: a diamond.
    f.write(
        "user:local:agd",
        &claim("a", TASK, "2026-05-29T00:00:01.000Z", json!({SUPERSEDES: cid("prior")})),
    );
    f.write(
        "user:local:agd",
        &claim("b", TASK, "2026-05-29T00:00:02.000Z", json!({SUPERSEDES: cid("prior")})),
    );
    // a prior with a single superseder must NOT be flagged.
    f.write("user:local:agd", &claim("solo", TASK, "2026-05-29T00:00:03.000Z", json!({})));
    f.write(
        "user:local:agd",
        &claim("c", TASK, "2026-05-29T00:00:04.000Z", json!({SUPERSEDES: cid("solo")})),
    );

    let g = f.index();
    let conflicts = g.diamond_conflicts(SUPERSEDES).unwrap();
    assert_eq!(conflicts.len(), 1, "only the diamond prior is flagged");
    assert_eq!(conflicts[0].prior, cid("prior"));
    assert_eq!(conflicts[0].superseders, vec![cid("a"), cid("b")]);
}

#[test]
fn h9_diamond_ignores_dead_superseders() {
    let f = Fixture::new();
    f.write("user:local:agd", &claim("prior", TASK, "2026-05-29T00:00:00.000Z", json!({})));
    f.write(
        "user:local:agd",
        &claim("a", TASK, "2026-05-29T00:00:01.000Z", json!({SUPERSEDES: cid("prior")})),
    );
    f.write(
        "user:local:agd",
        &claim("b", TASK, "2026-05-29T00:00:02.000Z", json!({SUPERSEDES: cid("prior")})),
    );
    // b is itself superseded -> only one LIVE superseder of prior -> no diamond.
    f.write(
        "user:local:agd",
        &claim("b2", TASK, "2026-05-29T00:00:03.000Z", json!({SUPERSEDES: cid("b")})),
    );

    let g = f.index();
    assert!(
        g.diamond_conflicts(SUPERSEDES).unwrap().is_empty(),
        "a single live superseder is not a conflict"
    );
}

// ----------------------------------------------------------------------
// H10 -- plan_at_risk (the highest-risk multi-hop dateTime compare).
// ----------------------------------------------------------------------

#[test]
fn h10_plan_at_risk_flags_newer_superseder_of_snapshot_member() {
    let f = Fixture::new();
    // Member claim A captured into a Spec's agreeSnapshot at AGREE time.
    f.write("user:local:agd", &claim("A", TASK, "2026-05-29T00:00:00.000Z", json!({})));
    // The Spec agreed at 00:00:05, with agreeSnapshot = [A].
    f.write(
        "user:local:agd",
        &claim(
            "spec1",
            SPEC,
            "2026-05-29T00:00:05.000Z",
            json!({AGREE: [cid("A")]}),
        ),
    );
    // A' supersedes A with a generatedAtTime AFTER the spec's agreed time -> at risk.
    f.write(
        "user:local:agd",
        &claim("Aprime", TASK, "2026-05-29T00:00:10.000Z", json!({SUPERSEDES: cid("A")})),
    );

    let g = f.index();
    let hits = g.plan_at_risk(SPEC, AGREE, SUPERSEDES).unwrap();
    assert_eq!(hits.len(), 1, "spec1's snapshot member A was superseded later");
    assert_eq!(hits[0].spec, cid("spec1"));
    assert_eq!(hits[0].old_claim, cid("A"));
    assert_eq!(hits[0].new_claim, cid("Aprime"));
    assert_eq!(hits[0].new_at, "2026-05-29T00:00:10.000Z");
    assert_eq!(hits[0].stakeholder, "asserter:user:local:agd");
}

#[test]
fn h10_plan_at_risk_ignores_supersession_before_agree() {
    let f = Fixture::new();
    f.write("user:local:agd", &claim("A", TASK, "2026-05-29T00:00:00.000Z", json!({})));
    // A' supersedes A BEFORE the spec agreed -> the spec snapshotted the
    // already-superseded state knowingly, so it is NOT at risk.
    f.write(
        "user:local:agd",
        &claim("Aprime", TASK, "2026-05-29T00:00:02.000Z", json!({SUPERSEDES: cid("A")})),
    );
    f.write(
        "user:local:agd",
        &claim("spec1", SPEC, "2026-05-29T00:00:05.000Z", json!({AGREE: [cid("A")]})),
    );

    let g = f.index();
    assert!(
        g.plan_at_risk(SPEC, AGREE, SUPERSEDES).unwrap().is_empty(),
        "supersession predates AGREE -> not at risk"
    );
}

// ----------------------------------------------------------------------
// Incremental rebuild keyed on the heads signal.
// ----------------------------------------------------------------------

#[test]
fn sync_is_skipped_when_heads_unchanged_and_runs_when_changed() {
    let f = Fixture::new();
    f.write("user:local:agd", &claim("t1", TASK, "2026-05-29T00:00:00.000Z", json!({})));

    let mut g = Gamma::open_in_memory().unwrap();
    let first = g.sync(&f.claims_dir).unwrap();
    assert!(first.is_some(), "first sync rebuilds");
    assert_eq!(first.unwrap().claims_loaded, 1);

    // No log change -> sync is a no-op.
    let again = g.sync(&f.claims_dir).unwrap();
    assert!(again.is_none(), "unchanged heads -> rebuild skipped");

    // Append a claim -> heads move -> sync rebuilds.
    f.write("user:local:agd", &claim("t2", TASK, "2026-05-29T00:00:01.000Z", json!({})));
    let third = g.sync(&f.claims_dir).unwrap();
    assert!(third.is_some(), "changed heads -> rebuild runs");
    assert_eq!(third.unwrap().claims_loaded, 2);
    assert_eq!(g.count_by_type(TASK).unwrap(), 2);
}

#[test]
fn rebuild_reports_datetime_violations_but_still_indexes() {
    let f = Fixture::new();
    // A non-canonical timestamp (no millis): flagged, but still indexed.
    f.write("user:local:agd", &claim("t1", TASK, "2026-05-29T00:00:00Z", json!({})));

    let mut g = Gamma::open_in_memory().unwrap();
    let stats = g.sync(&f.claims_dir).unwrap().unwrap();
    assert_eq!(stats.datetime_violations, 1);
    assert_eq!(stats.claims_loaded, 1);
    assert_eq!(g.count_by_type(TASK).unwrap(), 1);
}

#[test]
fn on_disk_index_persists_and_reopens() {
    let f = Fixture::new();
    f.write("user:local:agd", &claim("t1", TASK, "2026-05-29T00:00:00.000Z", json!({})));

    let index_path = f.claims_dir.join("_view.gamma.redb");
    {
        let g = Gamma::open(&index_path, &f.claims_dir).unwrap();
        assert_eq!(g.count_by_type(TASK).unwrap(), 1);
        assert!(!g.is_in_memory());
    }
    // Reopen: heads unchanged, so no rebuild; data still present.
    let g = Gamma::open(&index_path, &f.claims_dir).unwrap();
    assert_eq!(g.count_by_type(TASK).unwrap(), 1);
}

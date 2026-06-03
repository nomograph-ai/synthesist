//! B.1 thesis validation: drive the plan-at-risk overlay through the
//! real synthesist binary.
//!
//! This test is the alpha thesis closing the loop. It does NOT
//! hand-craft JSON-LD fixtures (that is what the unit tests in
//! src/overlay/plan_at_risk.rs cover). It drives the binary end to
//! end and asserts the overlay fires.
//!
//! Scenario:
//!   1. init + session start + phase plan
//!   2. tree + spec + three tasks
//!   3. Capture the three Task @id IRIs from the v3 log
//!   4. phase agree + spec update --agree-snapshot <three IRIs>
//!   5. phase execute + task claim t1 (writes a superseder Task claim
//!      whose `synthesist:supersedes` points at t1's original IRI)
//!   6. overlay run plan-at-risk
//!   7. Assert the overlay reports at least one hit referencing the
//!      spec, with detail.old_claim pointing at the t1 IRI.

use std::fs;
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers (duplicated from v3_integration.rs; small footprint, no shared
// `mod common` infrastructure across the tests/ tree yet).
// ---------------------------------------------------------------------------

fn synth(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("synthesist").unwrap();
    cmd.current_dir(dir.path());
    cmd.env("SYNTHESIST_OFFLINE", "1");
    cmd.env_remove("SYNTHESIST_DIR");
    cmd.env_remove("SYNTHESIST_SESSION");
    // No surface is configured, so the default is the full v3 surface: every
    // command, including `overlay run`, is available without opting in.
    cmd.env_remove("SYNTHESIST_MANIFEST");
    cmd.env("USER", "b1test");
    cmd
}

fn synth_s(dir: &TempDir, session: &str) -> Command {
    let mut cmd = synth(dir);
    cmd.args(["--session", session]);
    cmd
}

fn asserter_dir(session: &str) -> String {
    format!("user-local-b1test-{session}")
}

#[test]
fn plan_at_risk_fires_end_to_end_through_cli() {
    let tmp = TempDir::new().unwrap();
    let session = "b1-thesis";

    // 1. init
    synth(&tmp).args(["init"]).assert().success();

    // 2. session start
    synth(&tmp)
        .args(["session", "start", session])
        .assert()
        .success();

    // 3. phase set plan (required before tree/spec/task writes)
    synth_s(&tmp, session)
        .args(["phase", "set", "plan"])
        .assert()
        .success();

    // 4. tree + spec
    synth_s(&tmp, session)
        .args(["tree", "add", "alpha", "--description", "thesis tree"])
        .assert()
        .success();

    synth_s(&tmp, session)
        .args([
            "spec",
            "add",
            "alpha/release-v3",
            "--goal",
            "ship v3.0.0-pre.1",
        ])
        .assert()
        .success();

    // 5. three tasks
    for t in ["t1", "t2", "t3"] {
        synth_s(&tmp, session)
            .args([
                "task",
                "add",
                "alpha/release-v3",
                &format!("Do {t}"),
                "--id",
                t,
            ])
            .assert()
            .success();
    }

    // The snapshot must be pinned while still in PLAN phase: AGREE
    // explicitly rejects edits with "phase violation (agree): no
    // operations in AGREE phase". The semantic flow is "operator
    // locks the snapshot, then transitions to AGREE."
    //
    // 5a. Extract Task @id IRIs from the v3 log.
    let log_path = tmp
        .path()
        .join("claims")
        .join(asserter_dir(session))
        .join("log.jsonl");
    let log_text = fs::read_to_string(&log_path).expect("v3 log must exist after task add");
    let task_iris: Vec<String> = log_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|doc| doc["@type"].as_str() == Some("synthesist:Task"))
        .map(|doc| {
            doc["@id"]
                .as_str()
                .expect("task claim must have @id")
                .to_string()
        })
        .collect();
    assert_eq!(
        task_iris.len(),
        3,
        "expected 3 Task @id IRIs in v3 log, found {}: {:?}",
        task_iris.len(),
        task_iris
    );

    // 5b. spec update --agree-snapshot <three IRIs> (still PLAN phase).
    let snapshot_arg = task_iris.join(",");
    synth_s(&tmp, session)
        .args([
            "spec",
            "update",
            "alpha/release-v3",
            "--agree-snapshot",
            &snapshot_arg,
        ])
        .assert()
        .success();

    // 6. phase agree (operator records the plan is locked).
    synth_s(&tmp, session)
        .args(["phase", "set", "agree"])
        .assert()
        .success();

    // 7. phase execute (required before task claim).
    synth_s(&tmp, session)
        .args(["phase", "set", "execute"])
        .assert()
        .success();

    // 10. Supersede t1 via task claim (writes a Task claim whose
    //     `synthesist:supersedes` IRI points at the original t1's @id).
    synth_s(&tmp, session)
        .args(["task", "claim", "alpha/release-v3", "t1"])
        .assert()
        .success();

    // 11. Run the plan-at-risk overlay.
    let out = synth(&tmp)
        .args(["overlay", "run", "plan-at-risk"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_str = String::from_utf8(out).expect("overlay stdout must be utf8");
    let result: serde_json::Value =
        serde_json::from_str(&out_str).expect("overlay output must be valid JSON");

    // 12. Assert at least one hit references our spec and that
    //     detail.old_claim points at the t1 IRI we superseded.
    let hits = result["hits"]
        .as_array()
        .or_else(|| result.as_array())
        .expect("overlay output must surface a 'hits' array (or be one)");

    assert!(
        !hits.is_empty(),
        "expected at least one plan-at-risk hit, got empty result: {result}"
    );

    // The overlay emits expanded IRIs (the SPARQL prefix expanded
    // synthesist: to https://nomograph.org/synthesist/), while the
    // log's @id values are the compact form. Compare on the shared
    // hash suffix instead.
    let t1_iri = &task_iris[0];
    let t1_hash = t1_iri
        .strip_prefix("synthesist:claim/")
        .expect("@id must be in synthesist:claim/<hash> form");
    let matching_hit = hits.iter().find(|h| {
        let old = h
            .get("detail")
            .and_then(|d| d.get("old_claim"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        old.ends_with(t1_hash)
    });
    assert!(
        matching_hit.is_some(),
        "no hit's detail.old_claim matched the t1 hash {t1_hash}; hits: {hits:?}"
    );

    // Sanity: the subject is the spec, the object is the superseder.
    let hit = matching_hit.unwrap();
    assert_eq!(
        hit.get("predicate").and_then(|v| v.as_str()).unwrap_or(""),
        "synthesist:planAtRisk"
    );
    let spec_id_in_detail = hit
        .get("detail")
        .and_then(|d| d.get("spec_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(spec_id_in_detail, "release-v3");
}

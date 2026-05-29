//! End-to-end test for the v3 query layer.
//!
//! Exercises the full path: append claims via [`LogWriter`],
//! rebuild the [`GraphView`] from the log union, run synthesist-shaped
//! queries through [`select`] and [`ask`], and verify the
//! [`heads`] file detects staleness correctly.
//!
//! Mirrors the queries that the spike at
//! `keaton/research/graph-primitive/spike-v3-oxigraph/` exercised
//! against storr's real data: count by type, asserter audit,
//! filter by status.

use std::time::Instant;

use nomograph_claim::{
    graph_view::{GraphView, Term, ask, rebuild, select},
    heads::{heads_match, write_heads},
    log::LogWriter,
};
use serde_json::{Value, json};
use tempfile::TempDir;

fn make_synth_task(id_suffix: &str, asserter_iri: &str, status: &str) -> Value {
    json!({
        "@context": {
            "synthesist": "https://nomograph.org/synthesist/",
            "prov":      "http://www.w3.org/ns/prov#",
            "xsd":       "http://www.w3.org/2001/XMLSchema#",
            "prov:generatedAtTime": {"@type": "xsd:dateTime"},
            "prov:wasAttributedTo": {"@type": "@id"}
        },
        "@id": format!("synthesist:claim/{}", id_suffix),
        "@type": "synthesist:Task",
        "prov:generatedAtTime": "2026-05-29T01:00:00.000Z",
        "prov:wasAttributedTo": asserter_iri,
        "synthesist:status": status,
        "synthesist:summary": format!("Test task {}", id_suffix),
    })
}

#[test]
fn build_and_query_status_shape() {
    let tmp = TempDir::new().unwrap();
    let claims_dir = tmp.path();
    let writer = LogWriter::new(claims_dir).unwrap();

    // Write 30 claims across two asserters with mixed statuses.
    for i in 0..15 {
        writer
            .append(
                "user:local:agd",
                &make_synth_task(&format!("t{:02}", i), "asserter:user:local:agd", "pending"),
            )
            .unwrap();
    }
    for i in 0..10 {
        writer
            .append(
                "user:local:agd",
                &make_synth_task(&format!("d{:02}", i), "asserter:user:local:agd", "done"),
            )
            .unwrap();
    }
    for i in 0..5 {
        writer
            .append(
                "user:local:jkolb",
                &make_synth_task(
                    &format!("j{:02}", i),
                    "asserter:user:local:jkolb",
                    "pending",
                ),
            )
            .unwrap();
    }

    let view = GraphView::open_in_memory().unwrap();
    let stats = rebuild(&view, claims_dir).unwrap();
    assert_eq!(stats.claims_loaded, 30);

    //
    // Query 1: count by type (the synthesist `status` shape).
    //
    let q_by_type = r#"
        PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
        SELECT ?type (COUNT(?c) AS ?n)
        WHERE { GRAPH ?g { ?c rdf:type ?type } }
        GROUP BY ?type
    "#;
    let results = select(&view, q_by_type).unwrap();
    assert_eq!(results.rows.len(), 1, "one type: synthesist:Task");
    if let Term::Iri(s) = &results.rows[0][0] {
        assert!(s.ends_with("synthesist/Task"));
    } else {
        panic!("expected IRI for type");
    }

    //
    // Query 2: count pending tasks (the synthesist `task list --status pending` shape).
    //
    let q_pending = r#"
        PREFIX rdf:   <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
        PREFIX synthesist: <https://nomograph.org/synthesist/>
        SELECT (COUNT(?c) AS ?n)
        WHERE { GRAPH ?g { ?c rdf:type synthesist:Task . ?c synthesist:status "pending" } }
    "#;
    let results = select(&view, q_pending).unwrap();
    assert_eq!(results.rows.len(), 1);
    if let Term::Literal { value, .. } = &results.rows[0][0] {
        assert_eq!(value, "20", "15 + 5 = 20 pending tasks");
    }

    //
    // Query 3: asserter audit (the synthesist `session list` shape).
    //
    let q_by_asserter = r#"
        PREFIX prov: <http://www.w3.org/ns/prov#>
        SELECT ?a (COUNT(?c) AS ?n)
        WHERE { GRAPH ?g { ?c prov:wasAttributedTo ?a } }
        GROUP BY ?a
    "#;
    let results = select(&view, q_by_asserter).unwrap();
    assert_eq!(results.rows.len(), 2, "two distinct asserters");
}

#[test]
fn ask_query_against_populated_view() {
    let tmp = TempDir::new().unwrap();
    let claims_dir = tmp.path();
    let writer = LogWriter::new(claims_dir).unwrap();

    writer
        .append(
            "user:local:agd",
            &make_synth_task("only", "asserter:user:local:agd", "pending"),
        )
        .unwrap();

    let view = GraphView::open_in_memory().unwrap();
    rebuild(&view, claims_dir).unwrap();

    let q_yes = r#"
        PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
        PREFIX synthesist: <https://nomograph.org/synthesist/>
        ASK { GRAPH ?g { ?c rdf:type synthesist:Task } }
    "#;
    assert_eq!(ask(&view, q_yes).unwrap(), true);

    let q_no = r#"
        PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
        PREFIX synthesist: <https://nomograph.org/synthesist/>
        ASK { GRAPH ?g { ?c rdf:type synthesist:Discovery } }
    "#;
    assert_eq!(ask(&view, q_no).unwrap(), false);
}

#[test]
fn heads_detect_staleness_and_clear_after_rewrite() {
    let tmp = TempDir::new().unwrap();
    let claims_dir = tmp.path();
    let view_dir = claims_dir.join("_view");
    let writer = LogWriter::new(claims_dir).unwrap();

    for i in 0..5 {
        writer
            .append(
                "user:local:agd",
                &make_synth_task(&format!("h{}", i), "asserter:user:local:agd", "pending"),
            )
            .unwrap();
    }

    // First rebuild: write heads.
    let view = GraphView::open_in_memory().unwrap();
    rebuild(&view, claims_dir).unwrap();
    write_heads(&view_dir, claims_dir).unwrap();
    assert_eq!(heads_match(&view_dir, claims_dir).unwrap(), true);

    // Append a claim: heads should detect staleness.
    writer
        .append(
            "user:local:agd",
            &make_synth_task("new", "asserter:user:local:agd", "pending"),
        )
        .unwrap();
    assert_eq!(heads_match(&view_dir, claims_dir).unwrap(), false);

    // Rebuild and rewrite heads: match again.
    rebuild(&view, claims_dir).unwrap();
    write_heads(&view_dir, claims_dir).unwrap();
    assert_eq!(heads_match(&view_dir, claims_dir).unwrap(), true);
}

#[test]
fn end_to_end_runs_under_10_seconds() {
    // Sanity test for performance: at the test-data scale used in
    // this integration test, all of build + rebuild + query should
    // complete well under 10 seconds.
    let start = Instant::now();

    let tmp = TempDir::new().unwrap();
    let claims_dir = tmp.path();
    let writer = LogWriter::new(claims_dir).unwrap();

    for i in 0..200 {
        writer
            .append(
                "user:local:agd",
                &make_synth_task(
                    &format!("p{:03}", i),
                    "asserter:user:local:agd",
                    if i % 2 == 0 { "pending" } else { "done" },
                ),
            )
            .unwrap();
    }

    let view = GraphView::open_in_memory().unwrap();
    rebuild(&view, claims_dir).unwrap();

    let q = r#"
        PREFIX rdf:   <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
        PREFIX synthesist: <https://nomograph.org/synthesist/>
        SELECT (COUNT(?c) AS ?n)
        WHERE { GRAPH ?g { ?c rdf:type synthesist:Task . ?c synthesist:status "pending" } }
    "#;
    let _ = select(&view, q).unwrap();

    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 10,
        "end-to-end build + rebuild + query should complete under 10s, took {:?}",
        elapsed
    );
}

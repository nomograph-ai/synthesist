//! Integration test for the emit-shacl binary.
//!
//! Runs the binary, captures stdout, validates the Turtle is syntactically
//! correct via oxttl, and spot-checks key load-bearing properties.

use std::process::Command;

use assert_cmd::prelude::*;
use oxttl::TurtleParser;

/// Run emit-shacl and return its stdout as a String.
fn run_emitter() -> String {
    let output = Command::cargo_bin("emit-shacl")
        .expect("emit-shacl binary not found")
        .output()
        .expect("failed to run emit-shacl");
    assert!(
        output.status.success(),
        "emit-shacl exited with non-zero status: {:?}",
        output.status
    );
    String::from_utf8(output.stdout).expect("emit-shacl output is not valid UTF-8")
}

#[test]
fn emitter_produces_valid_turtle() {
    let ttl = run_emitter();
    let bytes = ttl.as_bytes();
    let parser = TurtleParser::new().for_slice(bytes);
    let triples: Vec<_> = parser
        .collect::<Result<Vec<_>, _>>()
        .expect("Turtle emitted by emit-shacl failed to parse");
    // Should have many triples; a rough lower bound to catch empty output.
    assert!(
        triples.len() > 20,
        "expected many triples, got {}",
        triples.len()
    );
}

#[test]
fn all_eight_shapes_are_present() {
    let ttl = run_emitter();
    for shape in &[
        "synthesist:TreeShape",
        "synthesist:SpecShape",
        "synthesist:TaskShape",
        "synthesist:DiscoveryShape",
        "synthesist:SessionShape",
        "synthesist:PhaseShape",
        "synthesist:CampaignShape",
        "synthesist:OutcomeShape",
    ] {
        assert!(
            ttl.contains(shape),
            "missing shape {} in emitter output",
            shape
        );
    }
}

#[test]
fn spec_shape_has_topics_with_min_count() {
    let ttl = run_emitter();
    // topics must appear with sh:minCount 1 (required non-empty array).
    assert!(
        ttl.contains("sh:path synthesist:topics"),
        "spec topics path missing"
    );
    // The topics block must contain sh:minCount 1.
    // We find the topics block by locating its content between delimiters.
    let idx = ttl
        .find("sh:path synthesist:topics")
        .expect("topics not found");
    let block = &ttl[idx..idx + 200];
    assert!(
        block.contains("sh:minCount 1"),
        "topics property should have sh:minCount 1; block: {}",
        block
    );
    assert!(
        !block.contains("sh:maxCount"),
        "topics property should not have sh:maxCount; block: {}",
        block
    );
}

#[test]
fn spec_shape_has_agree_snapshot_as_iri() {
    let ttl = run_emitter();
    let idx = ttl
        .find("sh:path synthesist:agree_snapshot")
        .expect("agree_snapshot not found");
    let block = &ttl[idx..idx + 200];
    assert!(
        block.contains("sh:nodeKind sh:IRI"),
        "agree_snapshot should be sh:nodeKind sh:IRI; block: {}",
        block
    );
}

#[test]
fn spec_statuses_correct() {
    let ttl = run_emitter();
    assert!(
        ttl.contains("\"draft\" \"active\" \"done\" \"superseded\""),
        "spec STATUSES not found in expected order"
    );
}

#[test]
fn task_statuses_correct() {
    let ttl = run_emitter();
    assert!(
        ttl.contains("\"pending\" \"in_progress\" \"done\" \"blocked\" \"waiting\" \"cancelled\""),
        "task STATUSES not found in expected order"
    );
}

#[test]
fn task_gate_is_optional_enum() {
    let ttl = run_emitter();
    let idx = ttl.find("sh:path synthesist:gate").expect("gate not found");
    let block = &ttl[idx..idx + 200];
    assert!(
        block.contains("sh:maxCount 1"),
        "gate should be optional (sh:maxCount 1); block: {}",
        block
    );
    assert!(
        block.contains("\"human\""),
        "gate sh:in should contain \"human\"; block: {}",
        block
    );
    assert!(
        !block.contains("sh:minCount"),
        "gate should not have sh:minCount; block: {}",
        block
    );
}

#[test]
fn phase_names_correct() {
    let ttl = run_emitter();
    assert!(
        ttl.contains("\"orient\" \"plan\" \"agree\" \"execute\" \"reflect\" \"replan\" \"report\""),
        "phase NAMES not found in expected order"
    );
}

#[test]
fn outcome_statuses_correct() {
    let ttl = run_emitter();
    assert!(
        ttl.contains("\"completed\" \"abandoned\" \"deferred\" \"superseded_by\""),
        "outcome STATUSES not found in expected order"
    );
}

#[test]
fn campaign_kinds_correct() {
    let ttl = run_emitter();
    assert!(
        ttl.contains("\"active\" \"backlog\""),
        "campaign KINDS not found in expected order"
    );
}

#[test]
fn prefixes_declared() {
    let ttl = run_emitter();
    assert!(ttl.contains("@prefix synthesist: <https://nomograph.org/synthesist/>"));
    assert!(ttl.contains("@prefix sh: <http://www.w3.org/ns/shacl#>"));
    assert!(ttl.contains("@prefix xsd: <http://www.w3.org/2001/XMLSchema#>"));
}

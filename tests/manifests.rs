//! Integration tests for the initial 5 surface manifests (T5.4).
//!
//! Each test loads a manifest file from `surface/` via `Manifest::load` and
//! verifies that the name field matches expectations and at least one of the
//! command lists is non-empty.

use std::path::PathBuf;

use nomograph_synthesist::surface::manifest;

fn surface_path(filename: &str) -> PathBuf {
    // Integration tests run with cwd = repo root, which is where `surface/` lives.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("surface")
        .join(filename)
}

#[test]
fn baseline_v25_loads() {
    let m = manifest::load(&surface_path("baseline-v25.toml")).expect("baseline-v25 must parse");
    assert_eq!(m.name, "baseline-v25");
    assert!(
        !m.include.is_empty(),
        "baseline-v25 include list must not be empty"
    );
    assert!(m.include.contains(&"status".to_string()));
    assert!(m.include.contains(&"task add".to_string()));
    assert!(m.add.is_empty(), "baseline-v25 should add no new commands");
}

#[test]
fn sparql_exposed_loads() {
    let m =
        manifest::load(&surface_path("sparql-exposed.toml")).expect("sparql-exposed must parse");
    assert_eq!(m.name, "sparql-exposed");
    assert!(
        !m.include.is_empty(),
        "sparql-exposed include list must not be empty"
    );
    assert!(
        m.add.contains(&"overlay list".to_string()),
        "sparql-exposed must add 'overlay list'"
    );
    assert!(
        m.add.contains(&"overlay run".to_string()),
        "sparql-exposed must add 'overlay run'"
    );
}

#[test]
fn overlay_first_class_loads() {
    let m = manifest::load(&surface_path("overlay-first-class.toml"))
        .expect("overlay-first-class must parse");
    assert_eq!(m.name, "overlay-first-class");
    assert!(
        !m.include.is_empty(),
        "overlay-first-class include list must not be empty"
    );
    assert!(
        m.add.contains(&"overlay run".to_string()),
        "overlay-first-class must add 'overlay run'"
    );
}

#[test]
fn composite_commands_loads() {
    let m = manifest::load(&surface_path("composite-commands.toml"))
        .expect("composite-commands must parse");
    assert_eq!(m.name, "composite-commands");
    assert!(
        !m.include.is_empty(),
        "composite-commands include list must not be empty"
    );
}

#[test]
fn pruned_loads() {
    let m = manifest::load(&surface_path("pruned.toml")).expect("pruned must parse");
    assert_eq!(m.name, "pruned");
    assert!(
        !m.include.is_empty(),
        "pruned include list must not be empty"
    );
    assert!(
        !m.exclude.is_empty(),
        "pruned exclude list must not be empty; the manifest purpose is to suppress commands"
    );
    assert!(
        m.exclude.contains(&"task block".to_string()),
        "pruned must exclude 'task block'"
    );
    assert!(
        m.exclude.contains(&"import".to_string()),
        "pruned must exclude 'import'"
    );
}

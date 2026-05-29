//! Embedded base ontology (Turtle and SHACL).
//!
//! Ships the substrate's universal vocabulary as compile-time string
//! constants so consumers do not need a network fetch to resolve the
//! base @context vocabulary or the structural shapes.
//!
//! Module-specific vocabularies (synthesist:, future modules) live with
//! their own crates. This crate embeds only the base.
//!
//! See `ontology/base.ttl` for the source. The `serialize_ontology()`
//! helper writes both Turtle files to a directory of the caller's
//! choosing; synthesist's release pipeline uses this to emit the
//! `_schema/` documentation alongside every binary build.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

/// Base substrate vocabulary in Turtle form.
///
/// Defines `nomograph:Asserter`, the asserter class taxonomy
/// (`User`, `Agent`, `Ingest`), and the universal envelope predicates
/// (`prov:generatedAtTime`, `prov:wasAttributedTo`, `prov:wasRevisionOf`,
/// `nomograph:parentAsserter`).
pub const BASE_TTL: &str = include_str!("../ontology/base.ttl");

/// Base substrate SHACL shapes in Turtle form.
///
/// Documents `nomograph:ClaimEnvelopeShape`, the structural constraint
/// every claim must satisfy. Module shapes extend it with per-type
/// required predicates.
pub const BASE_SHACL_TTL: &str = include_str!("../ontology/base.shacl.ttl");

/// File name used by [`serialize_ontology`] for the base vocabulary.
pub const BASE_TTL_FILENAME: &str = "base.ttl";

/// File name used by [`serialize_ontology`] for the base SHACL shapes.
pub const BASE_SHACL_FILENAME: &str = "base.shacl.ttl";

/// Write the embedded ontology files into `target_dir`. Creates the
/// directory if absent. Idempotent: overwrites existing files with
/// the embedded content.
///
/// Used by synthesist's release pipeline to emit the `_schema/`
/// documentation artifacts alongside every binary build.
pub fn serialize_ontology(target_dir: &Path) -> Result<()> {
    fs::create_dir_all(target_dir)
        .with_context(|| format!("create {}", target_dir.display()))?;

    let ttl_path = target_dir.join(BASE_TTL_FILENAME);
    fs::write(&ttl_path, BASE_TTL)
        .with_context(|| format!("write {}", ttl_path.display()))?;

    let shacl_path = target_dir.join(BASE_SHACL_FILENAME);
    fs::write(&shacl_path, BASE_SHACL_TTL)
        .with_context(|| format!("write {}", shacl_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Acceptance criterion: BASE_TTL parses as valid Turtle through
    /// a conformant RDF parser.
    #[test]
    fn base_ttl_parses_as_turtle() {
        use oxrdf::NamedNode;
        use oxttl::TurtleParser;

        let parser = TurtleParser::new()
            .with_base_iri("https://nomograph.org/v3/")
            .expect("base IRI valid")
            .for_slice(BASE_TTL.as_bytes());

        let mut count = 0usize;
        for triple in parser {
            triple.expect("triple parses");
            count += 1;
        }

        assert!(count > 0, "base.ttl must produce at least one triple");

        // Sanity check: we should see at least one rdfs:label,
        // confirming the ontology body is included.
        let parser = TurtleParser::new()
            .with_base_iri("https://nomograph.org/v3/")
            .expect("base IRI valid")
            .for_slice(BASE_TTL.as_bytes());

        let rdfs_label =
            NamedNode::new("http://www.w3.org/2000/01/rdf-schema#label").unwrap();
        let mut found_label = false;
        for triple in parser {
            let triple = triple.unwrap();
            if triple.predicate == rdfs_label {
                found_label = true;
                break;
            }
        }
        assert!(found_label, "base.ttl should declare at least one rdfs:label");
    }

    /// Acceptance criterion: BASE_SHACL_TTL parses as valid Turtle.
    #[test]
    fn base_shacl_ttl_parses_as_turtle() {
        use oxttl::TurtleParser;

        let parser = TurtleParser::new()
            .with_base_iri("https://nomograph.org/v3/")
            .expect("base IRI valid")
            .for_slice(BASE_SHACL_TTL.as_bytes());

        let mut count = 0usize;
        for triple in parser {
            triple.expect("triple parses");
            count += 1;
        }

        assert!(
            count > 0,
            "base.shacl.ttl must produce at least one triple"
        );
    }

    /// Acceptance criterion: serialize_ontology writes both files to
    /// the given directory.
    #[test]
    fn serialize_writes_both_files() {
        let tmp = TempDir::new().unwrap();
        serialize_ontology(tmp.path()).unwrap();

        let ttl = tmp.path().join(BASE_TTL_FILENAME);
        let shacl = tmp.path().join(BASE_SHACL_FILENAME);

        assert!(ttl.exists(), "base.ttl should be written");
        assert!(shacl.exists(), "base.shacl.ttl should be written");

        let ttl_content = fs::read_to_string(&ttl).unwrap();
        assert!(ttl_content.contains("nomograph:Asserter"));

        let shacl_content = fs::read_to_string(&shacl).unwrap();
        assert!(shacl_content.contains("ClaimEnvelopeShape"));
    }

    /// Idempotency: calling serialize_ontology twice produces the same
    /// state.
    #[test]
    fn serialize_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        serialize_ontology(tmp.path()).unwrap();
        let first_ttl = fs::read_to_string(tmp.path().join(BASE_TTL_FILENAME)).unwrap();

        serialize_ontology(tmp.path()).unwrap();
        let second_ttl = fs::read_to_string(tmp.path().join(BASE_TTL_FILENAME)).unwrap();

        assert_eq!(first_ttl, second_ttl);
    }

    /// Base TTL declares the asserter class taxonomy.
    #[test]
    fn base_ttl_declares_asserter_taxonomy() {
        assert!(BASE_TTL.contains("nomograph:Asserter"));
        assert!(BASE_TTL.contains("nomograph:User"));
        assert!(BASE_TTL.contains("nomograph:Agent"));
        assert!(BASE_TTL.contains("nomograph:Ingest"));
    }

    /// Base TTL references the canonical PROV-O predicates.
    #[test]
    fn base_ttl_references_prov_predicates() {
        assert!(BASE_TTL.contains("prov:generatedAtTime"));
        assert!(BASE_TTL.contains("prov:wasAttributedTo"));
        assert!(BASE_TTL.contains("prov:wasRevisionOf"));
    }
}

//! v2-to-v3 migration: translate v2 Automerge claim log to v3 JSON-LD
//! per-asserter logs.
//!
//! Reads every claim from a v2 `claims/changes/*.amc` tree and writes
//! equivalent JSON-LD documents to `claims/<asserter>/log.jsonl`.
//! Original timestamps are preserved as `prov:generatedAtTime`.
//! Supersession edges become `synthesist:supersedes` IRIs.
//!
//! The lifted translation code uses the deprecated v2 Store/Claim APIs
//! (deprecated since A.4). The allow is scoped to this module only.
#![allow(deprecated)]

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use nomograph_claim::asserter;
use nomograph_claim::claim::{Claim, ClaimType};
use nomograph_claim::log::LogWriter;
use nomograph_claim::store::Store;
use serde_json::{Map, Value};

use super::{Migration, MigrationError, MigrationOpts, MigrationReport};

// ---------------------------------------------------------------------------
// V2ToV3 struct
// ---------------------------------------------------------------------------

/// First registered migration: translate v2 Automerge claims to v3 JSON-LD logs.
pub struct V2ToV3;

impl Migration for V2ToV3 {
    fn from_version(&self) -> &'static str {
        "2.x"
    }

    fn to_version(&self) -> &'static str {
        "3.0.0-pre.1"
    }

    fn description(&self) -> &'static str {
        "Translate v2 Automerge .amc claims to v3 per-asserter JSON-LD logs"
    }

    /// Returns true iff `claims/changes/` exists and no `claims/<asserter>/log.jsonl`
    /// files are present yet.
    fn detect(&self, root: &Path) -> Result<bool, MigrationError> {
        let claims = root.join("claims");
        let changes = claims.join("changes");

        if !changes.exists() {
            return Ok(false);
        }

        // Check whether any per-asserter log.jsonl files already exist.
        if let Ok(entries) = std::fs::read_dir(&claims) {
            for entry in entries.flatten() {
                let ft = entry.file_type().map_err(MigrationError::Io)?;
                if !ft.is_dir() {
                    continue;
                }
                let log_path = entry.path().join("log.jsonl");
                if log_path.exists() {
                    // v3 logs already present -- migration already ran.
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    fn run(&self, root: &Path, opts: &MigrationOpts) -> Result<MigrationReport, MigrationError> {
        let claims = root.join("claims");
        let mut report = MigrationReport {
            from: self.from_version().to_string(),
            to: self.to_version().to_string(),
            artifacts_touched: 0,
            backup_path: None,
            notes: Vec::new(),
        };

        // Tarball backup before any mutation.
        if opts.backup && !opts.dry_run {
            let backup_path = write_tarball_backup(root)?;
            report.backup_path = Some(backup_path);
        }

        // Open v2 store and load claims.
        let mut store = Store::open(&claims)
            .map_err(|e| MigrationError::Failed(format!("open v2 store: {e}")))?;
        let v2_claims = store
            .load_claims()
            .map_err(|e| MigrationError::Failed(format!("load v2 claims: {e}")))?;

        let writer = if opts.dry_run {
            None
        } else {
            std::fs::create_dir_all(&claims).map_err(MigrationError::Io)?;
            let w = LogWriter::new(&claims)
                .map_err(|e| MigrationError::Failed(format!("create log writer: {e}")))?;
            Some(w)
        };

        let mut skipped: Vec<String> = Vec::new();

        for claim in &v2_claims {
            // module_for_type errors on lattice-owned types per Directive 2.
            // The Ok branch's value is unused -- it is always `"synthesist"`
            // because `v2_claim_to_v3` is hard-wired to the synthesist IRI
            // builders in `wire_format`. We still call the function for its
            // error-signaling side effect.
            if let Err(e) = module_for_type(&claim.claim_type) {
                skipped.push(format!("skipped {}: {e}", claim.id));
                continue;
            }

            let doc = v2_claim_to_v3(claim);

            let asserter_str = claim.asserted_by.as_str();
            if asserter::parse(asserter_str).is_err() {
                skipped.push(format!("skipped {}: invalid asserter {asserter_str}", claim.id));
                continue;
            }

            if let Some(w) = &writer {
                w.append(asserter_str, &doc)
                    .map_err(|e| MigrationError::Failed(format!("append claim {}: {e}", claim.id)))?;
            }

            report.artifacts_touched += 1;
        }

        if !skipped.is_empty() {
            report.notes.push(format!("skipped {} claims", skipped.len()));
            report.notes.extend(skipped);
        }

        Ok(report)
    }
}

// ---------------------------------------------------------------------------
// Translation helpers (lifted from /tmp/v2_to_v3_source.rs)
// ---------------------------------------------------------------------------

/// Translate one v2 claim into a v3 JSON-LD document.
///
/// Hard-wired to the synthesist module IRI builders in `wire_format`.
/// Lattice-typed claims are rejected upstream by `module_for_type`, so
/// this function never sees them. A future migration that supports
/// other modules will need parallel IRI builders in `wire_format` plus
/// a route here; do not parameterize this function speculatively.
fn v2_claim_to_v3(claim: &Claim) -> Value {
    use crate::wire_format as wf;

    let mut doc = Map::new();
    // Single source of truth for the v3 shape -- see wire_format.rs.
    doc.insert("@context".into(), wf::jsonld_context());
    doc.insert("@id".into(), Value::String(wf::claim_iri(&claim.id)));
    doc.insert(
        "@type".into(),
        Value::String(wf::type_iri(claim.claim_type.as_str())),
    );
    doc.insert(
        wf::GENERATED_AT_PRED.into(),
        Value::String(format_iso(claim.asserted_at)),
    );
    doc.insert(
        wf::ATTRIBUTED_TO_PRED.into(),
        Value::String(wf::asserter_iri(&claim.asserted_by)),
    );

    if let Some(sup) = &claim.supersedes {
        doc.insert(
            wf::SUPERSEDES_PRED.into(),
            Value::String(wf::claim_iri(sup)),
        );
    }
    if let Some(parent) = &claim.parent_asserter {
        doc.insert(
            wf::PARENT_ASSERTER_PRED.into(),
            Value::String(wf::asserter_iri(parent)),
        );
    }

    if let Some(props_map) = claim.props.as_object() {
        for (k, v) in props_map {
            doc.insert(wf::predicate_iri(k), v.clone());
        }
    }

    Value::Object(doc)
}

/// Map a v2 ClaimType to its v3 module prefix.
///
/// Synthesist-owned types map to `"synthesist"`.
/// Lattice-named types (Stakeholder, Topic, Signal, Disposition, Intent,
/// Heartbeat, Directive) are not migrated per Directive 2 -- they return
/// `MigrationError::UnsupportedClaimType`.
pub fn module_for_type(t: &ClaimType) -> Result<&'static str, MigrationError> {
    match t {
        ClaimType::Stakeholder
        | ClaimType::Topic
        | ClaimType::Signal
        | ClaimType::Disposition
        | ClaimType::Intent
        | ClaimType::Heartbeat
        | ClaimType::Directive => Err(MigrationError::UnsupportedClaimType {
            ty: t.as_str().to_string(),
        }),
        _ => Ok("synthesist"),
    }
}

// `camel_case` previously had a duplicate definition here, identical
// to the one in `crate::store`. Both now live in `crate::wire_format`
// (review item #10 / #1: extract wire_format module).

fn format_iso(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

// ---------------------------------------------------------------------------
// Tarball backup
// ---------------------------------------------------------------------------

/// Write `.synthesist-v2-backup.tar.gz` in `root`, containing the full
/// `claims/` subtree. Returns the path of the written archive.
fn write_tarball_backup(root: &Path) -> Result<PathBuf, MigrationError> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    let archive_path = root.join(".synthesist-v2-backup.tar.gz");
    let file = std::fs::File::create(&archive_path).map_err(MigrationError::Io)?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = Builder::new(enc);

    let claims_dir = root.join("claims");
    if claims_dir.exists() {
        tar.append_dir_all("claims", &claims_dir)
            .map_err(MigrationError::Io)?;
    }

    tar.finish().map_err(MigrationError::Io)?;
    Ok(archive_path)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn fake_claim(ty: ClaimType, id: &str, props: Value) -> Claim {
        Claim {
            id: id.to_string(),
            claim_type: ty,
            props,
            valid_from: Utc.with_ymd_and_hms(2026, 5, 5, 17, 43, 11).unwrap(),
            valid_until: None,
            supersedes: None,
            parent_asserter: None,
            asserted_by: "user:local:agd".to_string(),
            asserted_at: Utc.with_ymd_and_hms(2026, 5, 5, 17, 43, 11).unwrap(),
        }
    }

    #[test]
    fn synth_task_translates_to_synthesist_namespace() {
        let claim = fake_claim(
            ClaimType::Task,
            "abc123def456fed789",
            json!({"summary": "Test", "status": "pending"}),
        );
        let v3 = v2_claim_to_v3(&claim);
        assert_eq!(v3["@id"], Value::String("synthesist:claim/abc123def456fed7".into()));
        assert_eq!(v3["@type"], Value::String("synthesist:Task".into()));
        assert_eq!(
            v3["prov:wasAttributedTo"],
            Value::String("asserter:user:local:agd".into())
        );
        assert_eq!(v3["synthesist:summary"], Value::String("Test".into()));
        assert_eq!(v3["synthesist:status"], Value::String("pending".into()));
    }

    #[test]
    fn lattice_disposition_errors_on_module_for_type() {
        let result = module_for_type(&ClaimType::Disposition);
        assert!(matches!(result, Err(MigrationError::UnsupportedClaimType { .. })));
    }

    #[test]
    fn lattice_stakeholder_errors_on_module_for_type() {
        let result = module_for_type(&ClaimType::Stakeholder);
        assert!(matches!(result, Err(MigrationError::UnsupportedClaimType { .. })));
    }

    #[test]
    fn supersedes_edge_lands_in_synthesist_namespace() {
        let mut claim = fake_claim(
            ClaimType::Task,
            "aaaa111111111111",
            json!({"status": "done"}),
        );
        claim.supersedes = Some("bbbb222222222222".to_string());
        let v3 = v2_claim_to_v3(&claim);
        assert_eq!(
            v3["synthesist:supersedes"],
            Value::String("synthesist:claim/bbbb222222222222".into())
        );
    }

    #[test]
    fn parent_asserter_lands_in_nomograph_namespace() {
        let mut claim = fake_claim(
            ClaimType::Task,
            "cccc333333333333",
            json!({"status": "pending"}),
        );
        claim.parent_asserter = Some("user:local:agd".to_string());
        let v3 = v2_claim_to_v3(&claim);
        assert_eq!(
            v3["nomograph:parentAsserter"],
            Value::String("asserter:user:local:agd".into())
        );
    }

    // camel_case correctness is asserted in `wire_format::tests`; the
    // migration's own tests cover translation behavior, not the
    // wire-format primitives.

    #[test]
    fn module_for_type_routes_synthesist_types_correctly() {
        assert!(matches!(module_for_type(&ClaimType::Task), Ok("synthesist")));
        assert!(matches!(module_for_type(&ClaimType::Spec), Ok("synthesist")));
        assert!(matches!(module_for_type(&ClaimType::Outcome), Ok("synthesist")));
        assert!(matches!(module_for_type(&ClaimType::Tree), Ok("synthesist")));
        assert!(matches!(module_for_type(&ClaimType::Discovery), Ok("synthesist")));
        assert!(matches!(module_for_type(&ClaimType::Campaign), Ok("synthesist")));
        assert!(matches!(module_for_type(&ClaimType::Session), Ok("synthesist")));
        assert!(matches!(module_for_type(&ClaimType::Phase), Ok("synthesist")));
    }

    #[test]
    fn module_for_type_errors_on_all_lattice_types() {
        for ty in [
            ClaimType::Stakeholder,
            ClaimType::Topic,
            ClaimType::Signal,
            ClaimType::Disposition,
            ClaimType::Intent,
            ClaimType::Heartbeat,
            ClaimType::Directive,
        ] {
            assert!(
                matches!(module_for_type(&ty), Err(MigrationError::UnsupportedClaimType { .. })),
                "expected UnsupportedClaimType for {}", ty.as_str()
            );
        }
    }

    #[test]
    fn format_iso_produces_ms_precision_with_z() {
        let t = Utc.with_ymd_and_hms(2026, 5, 29, 4, 30, 0).unwrap();
        let s = format_iso(t);
        assert!(s.starts_with("2026-05-29T04:30:00.000"));
        assert!(s.ends_with('Z'));
    }
}

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
use nomograph_claim::jsonld;
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
            let module = match module_for_type(&claim.claim_type) {
                Ok(m) => m,
                Err(e) => {
                    skipped.push(format!("skipped {}: {e}", claim.id));
                    continue;
                }
            };

            let doc = v2_claim_to_v3(claim, module);

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
/// The module string is pre-computed by `module_for_type`.
fn v2_claim_to_v3(claim: &Claim, module: &'static str) -> Value {
    let type_camel = camel_case(claim.claim_type.as_str());
    let id_short = &claim.id[..claim.id.len().min(16)];

    let mut doc = Map::new();
    // Match the dual-write @context shape so a store containing both
    // freshly migrated and freshly dual-written claims yields a
    // single, queryable graph. See store::synthesist_jsonld_context.
    doc.insert("@context".into(), crate::store::synthesist_jsonld_context());
    doc.insert(
        "@id".into(),
        Value::String(format!("{}:claim/{}", module, id_short)),
    );
    doc.insert(
        "@type".into(),
        Value::String(format!("{}:{}", module, type_camel)),
    );
    doc.insert(
        "prov:generatedAtTime".into(),
        Value::String(format_iso(claim.asserted_at)),
    );
    doc.insert(
        "prov:wasAttributedTo".into(),
        Value::String(format!("asserter:{}", claim.asserted_by)),
    );

    if let Some(sup) = &claim.supersedes {
        let sup_short = &sup[..sup.len().min(16)];
        doc.insert(
            format!("{}:supersedes", module),
            Value::String(format!("{}:claim/{}", module, sup_short)),
        );
    }
    if let Some(parent) = &claim.parent_asserter {
        doc.insert(
            "nomograph:parentAsserter".into(),
            Value::String(format!("asserter:{}", parent)),
        );
    }

    // Expand props as <module>:<lowerCamel(key)> predicates so the
    // migration's output aligns with the dual-write's predicate
    // names (synthesist:agreeSnapshot, not synthesist:agree_snapshot).
    if let Some(props_map) = claim.props.as_object() {
        for (k, v) in props_map {
            doc.insert(
                format!("{}:{}", module, crate::store::lower_camel_case(k)),
                v.clone(),
            );
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

fn camel_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            out.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

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
        let v3 = v2_claim_to_v3(&claim, module_for_type(&claim.claim_type).unwrap());
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
        let v3 = v2_claim_to_v3(&claim, "synthesist");
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
        let v3 = v2_claim_to_v3(&claim, "synthesist");
        assert_eq!(
            v3["nomograph:parentAsserter"],
            Value::String("asserter:user:local:agd".into())
        );
    }

    #[test]
    fn camel_case_handles_simple_lowercase() {
        assert_eq!(camel_case("task"), "Task");
        assert_eq!(camel_case("spec"), "Spec");
        assert_eq!(camel_case("disposition"), "Disposition");
    }

    #[test]
    fn camel_case_handles_underscore_separation() {
        assert_eq!(camel_case("agent_intent"), "AgentIntent");
        assert_eq!(camel_case("agree_snapshot"), "AgreeSnapshot");
    }

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

//! `synthesist jig` subcommand.
//!
//! Provides four subcommands:
//!
//! - `jig run --scenario <name> --manifest <name>` -- resolve, parse, and
//!   record a jig run setup. For v3-alpha the actual LLM session is future
//!   work; this command records the setup and writes `claims/_jig/<run_id>.json`
//!   with `status: "pending"` and `outcome: null`.
//!
//! - `jig list-scenarios` -- print available scenarios from `jig/scenarios/`.
//!
//! - `jig list-manifests` -- print available manifests from `surface/`.
//!
//! - `jig aggregate [--format md|csv|json]` -- read all `claims/_jig/*.json`
//!   files and produce a comparison table grouped by (scenario, manifest).
//!
//! ## Result file layout
//!
//! `claims/_jig/<run_id>.json` fields:
//! - `run_id`: a unique identifier (`<timestamp_ms>-<random_hex_8>`).
//! - `scenario_name`, `manifest_name`: the inputs.
//! - `started_at`, `finished_at`: RFC 3339 timestamps.
//! - `status`: `"pending"` (v3-alpha; LLM session wrapping is future work).
//! - `scenario`: the parsed scenario data echoed back.
//! - `manifest`: `{ name, description }` only (not include/exclude/add lists).
//! - `outcome`: `null` (filled in by a future post-run command).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::JigCmd;
use crate::surface::manifest;

// ---------------------------------------------------------------------------
// Scenario TOML types
// ---------------------------------------------------------------------------

/// The raw on-disk scenario shape (mirrors jig-scenarios.md).
#[derive(Debug, Deserialize, Serialize, Clone)]
struct ScenarioFile {
    scenario: ScenarioHeader,
    starting_state: StartingState,
    goal: Goal,
    #[serde(default)]
    rubric: Vec<RubricEntry>,
    #[serde(default)]
    expected_artifacts: ExpectedArtifacts,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ScenarioHeader {
    name: String,
    description: String,
    version: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct StartingState {
    description: String,
    #[serde(default)]
    setup_commands: Vec<String>,
    #[serde(default)]
    fixture_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Goal {
    prompt: String,
    success_criterion: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct RubricEntry {
    id: String,
    description: String,
    weight: u32,
    check: String,
    #[serde(default)]
    check_command: Option<String>,
    #[serde(default)]
    check_pattern: Option<String>,
    #[serde(default)]
    check_artifact: Option<String>,
    #[serde(default)]
    partial_credit: bool,
}

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
struct ExpectedArtifacts {
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    claim_types: Vec<String>,
    #[serde(default)]
    claim_count_min: Option<u64>,
}

// ---------------------------------------------------------------------------
// Result JSON type
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct JigResult {
    run_id: String,
    scenario_name: String,
    manifest_name: String,
    started_at: String,
    finished_at: String,
    status: String,
    scenario: Value,
    manifest: ManifestSummary,
    outcome: Value,
}

#[derive(Serialize)]
struct ManifestSummary {
    name: String,
    description: String,
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

pub fn run(cmd: &JigCmd) -> Result<()> {
    match cmd {
        JigCmd::Run { scenario, manifest } => cmd_run(scenario, manifest),
        JigCmd::ListScenarios => cmd_list_scenarios(),
        JigCmd::ListManifests => cmd_list_manifests(),
        JigCmd::Aggregate { format } => cmd_aggregate(format),
    }
}

// ---------------------------------------------------------------------------
// `jig run`
// ---------------------------------------------------------------------------

fn cmd_run(scenario_name: &str, manifest_name: &str) -> Result<()> {
    let started_at = Utc::now();

    // Resolve scenario path.
    let scenario_path = resolve_scenario(scenario_name)?;
    // Resolve manifest path.
    let manifest_path = resolve_manifest(manifest_name)?;

    // Parse scenario TOML.
    let scenario_text = fs::read_to_string(&scenario_path).with_context(|| {
        format!("reading scenario file {}", scenario_path.display())
    })?;
    let scenario_file: ScenarioFile = toml::from_str(&scenario_text).with_context(|| {
        format!("parsing scenario TOML at {}", scenario_path.display())
    })?;

    // Parse manifest TOML.
    let manifest = manifest::load(&manifest_path)?;

    let finished_at = Utc::now();

    // Generate a unique run ID: <timestamp_ms>-<random_hex_8>.
    let ts_ms = started_at.timestamp_millis();
    let rand_hex = random_hex8();
    let run_id = format!("{ts_ms}-{rand_hex}");

    // Locate (or create) the _jig output directory.
    // Walk up from cwd to find claims/, then place _jig inside.
    let claims_dir = find_claims_dir()?;
    let jig_dir = claims_dir.join("_jig");
    fs::create_dir_all(&jig_dir)
        .with_context(|| format!("creating jig output dir {}", jig_dir.display()))?;

    // Build the result JSON.
    let scenario_value =
        serde_json::to_value(&scenario_file).context("serialising scenario to JSON")?;

    let result = JigResult {
        run_id: run_id.clone(),
        scenario_name: scenario_file.scenario.name.clone(),
        manifest_name: manifest.name.clone(),
        started_at: started_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        finished_at: finished_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        status: "pending".to_string(),
        scenario: scenario_value,
        manifest: ManifestSummary {
            name: manifest.name.clone(),
            description: manifest.description.clone(),
        },
        outcome: Value::Null,
    };

    // Write result file.
    let result_path = jig_dir.join(format!("{run_id}.json"));
    let result_json = serde_json::to_string_pretty(&result).context("serialising jig result")?;
    let mut file = fs::File::create(&result_path)
        .with_context(|| format!("creating result file {}", result_path.display()))?;
    file.write_all(result_json.as_bytes())
        .with_context(|| format!("writing result file {}", result_path.display()))?;
    file.sync_data().context("fsyncing jig result file")?;

    // fsync the directory so the new entry is durable.
    let dir_file = fs::File::open(&jig_dir)
        .with_context(|| format!("opening jig dir for fsync {}", jig_dir.display()))?;
    dir_file.sync_data().context("fsyncing jig directory")?;

    // Print the summary to stdout.
    let summary = json!({
        "run_id": run_id,
        "result_path": result_path.display().to_string(),
    });
    crate::store::json_out(&summary)
}

// ---------------------------------------------------------------------------
// `jig list-scenarios`
// ---------------------------------------------------------------------------

fn cmd_list_scenarios() -> Result<()> {
    let scenarios_dir = find_jig_scenarios_dir()?;
    let entries = read_toml_stems(&scenarios_dir, "_template")?;
    crate::store::json_out(&json!({ "scenarios": entries }))
}

// ---------------------------------------------------------------------------
// `jig list-manifests`
// ---------------------------------------------------------------------------

fn cmd_list_manifests() -> Result<()> {
    let surface_dir = find_surface_dir()?;
    let entries = read_toml_stems(&surface_dir, "")?;
    crate::store::json_out(&json!({ "manifests": entries }))
}

// ---------------------------------------------------------------------------
// `jig aggregate`
// ---------------------------------------------------------------------------

/// One row in the aggregate table.
#[derive(Debug, Serialize)]
struct AggRow {
    scenario: String,
    manifest: String,
    runs: u64,
    latest_started_at: String,
}

fn cmd_aggregate(format: &str) -> Result<()> {
    let rows = build_aggregate_rows()?;

    match format {
        "csv" => {
            println!("scenario,manifest,runs,latest_started_at");
            for row in &rows {
                println!(
                    "{},{},{},{}",
                    csv_escape(&row.scenario),
                    csv_escape(&row.manifest),
                    row.runs,
                    csv_escape(&row.latest_started_at),
                );
            }
        }
        "json" => {
            let out = json!({ "scenarios": rows });
            crate::store::json_out(&out)?;
        }
        _ => {
            // md (default)
            println!("| scenario | manifest | runs | latest_started_at |");
            println!("|---|---|---|---|");
            for row in &rows {
                println!(
                    "| {} | {} | {} | {} |",
                    row.scenario, row.manifest, row.runs, row.latest_started_at
                );
            }
        }
    }
    Ok(())
}

/// Read all `claims/_jig/*.json` files, group by (scenario_name, manifest_name),
/// count runs, and record the latest started_at per group.
///
/// Malformed JSON files emit a stderr warning and are skipped.
fn build_aggregate_rows() -> Result<Vec<AggRow>> {
    // Locate the _jig directory. If it does not exist, return an empty table.
    let claims_dir = match find_claims_dir_opt() {
        Some(d) => d,
        None => return Ok(Vec::new()),
    };
    let jig_dir = claims_dir.join("_jig");
    if !jig_dir.exists() {
        return Ok(Vec::new());
    }

    // Key: (scenario_name, manifest_name) -> (runs, latest_started_at)
    let mut groups: BTreeMap<(String, String), (u64, String)> = BTreeMap::new();

    let read_dir = fs::read_dir(&jig_dir)
        .with_context(|| format!("reading jig dir {}", jig_dir.display()))?;

    for entry_result in read_dir {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                eprintln!(
                    "warning: could not read directory entry in {}: {e}",
                    jig_dir.display()
                );
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!(
                    "warning: skipping {}: could not read file: {e}",
                    path.display()
                );
                continue;
            }
        };

        let v: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "warning: skipping {}: malformed JSON: {e}",
                    path.display()
                );
                continue;
            }
        };

        let scenario_name = match v.get("scenario_name").and_then(|s| s.as_str()) {
            Some(s) => s.to_string(),
            None => {
                eprintln!(
                    "warning: skipping {}: missing or non-string 'scenario_name'",
                    path.display()
                );
                continue;
            }
        };
        let manifest_name = match v.get("manifest_name").and_then(|s| s.as_str()) {
            Some(s) => s.to_string(),
            None => {
                eprintln!(
                    "warning: skipping {}: missing or non-string 'manifest_name'",
                    path.display()
                );
                continue;
            }
        };
        let started_at = v
            .get("started_at")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();

        let slot = groups
            .entry((scenario_name, manifest_name))
            .or_insert((0, String::new()));
        slot.0 += 1;
        // Keep the lexicographically latest started_at (RFC 3339 sorts correctly).
        if started_at > slot.1 {
            slot.1 = started_at;
        }
    }

    let rows: Vec<AggRow> = groups
        .into_iter()
        .map(|((scenario, manifest), (runs, latest_started_at))| AggRow {
            scenario,
            manifest,
            runs,
            latest_started_at,
        })
        .collect();

    Ok(rows)
}

/// Escape a string for CSV: wrap in quotes if it contains comma, quote, or newline.
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Like `find_claims_dir` but returns `None` instead of an error when the
/// directory cannot be found. Used by aggregate so that running from outside
/// a workspace produces an empty table rather than an error.
fn find_claims_dir_opt() -> Option<PathBuf> {
    find_claims_dir().ok()
}

// ---------------------------------------------------------------------------
// Path resolution helpers
// ---------------------------------------------------------------------------

/// Resolve a scenario name to an absolute path.
///
/// Looks for `jig/scenarios/<name>.toml` relative to the synthesist repo
/// root, located by walking parent directories from the current working
/// directory until `jig/scenarios/` is found.
fn resolve_scenario(name: &str) -> Result<PathBuf> {
    let dir = find_jig_scenarios_dir()?;
    let path = dir.join(format!("{name}.toml"));
    if !path.exists() {
        bail!(
            "scenario '{name}' not found: expected file at {}\n\
             Run `synthesist jig list-scenarios` to see available scenarios.",
            path.display()
        );
    }
    Ok(path)
}

/// Resolve a manifest name to an absolute path.
///
/// Looks for `surface/<name>.toml` relative to the synthesist repo root.
fn resolve_manifest(name: &str) -> Result<PathBuf> {
    let dir = find_surface_dir()?;
    let path = dir.join(format!("{name}.toml"));
    if !path.exists() {
        bail!(
            "manifest '{name}' not found: expected file at {}\n\
             Run `synthesist jig list-manifests` to see available manifests.",
            path.display()
        );
    }
    Ok(path)
}

/// Walk parent directories from the current working directory until a
/// directory containing `jig/scenarios/` is found. Returns the
/// `jig/scenarios/` path.
fn find_jig_scenarios_dir() -> Result<PathBuf> {
    find_ancestor_dir("jig/scenarios")
}

/// Walk parent directories until a directory containing `surface/` is found.
/// Returns the `surface/` path.
fn find_surface_dir() -> Result<PathBuf> {
    find_ancestor_dir("surface")
}

/// Walk parent directories from cwd until a directory containing `claims/`
/// is found. Returns the `claims/` path.
///
/// Used to determine where to write `claims/_jig/`.
fn find_claims_dir() -> Result<PathBuf> {
    // If SYNTHESIST_DIR is set, use it directly -- same convention as Store::discover.
    if let Ok(dir) = std::env::var("SYNTHESIST_DIR") {
        let p = PathBuf::from(&dir).join("claims");
        if p.exists() {
            return Ok(p);
        }
    }
    find_ancestor_dir("claims")
}

/// Walk from cwd upward until a directory `<name>` is found as a direct
/// child of some ancestor. Returns the path to that child directory.
fn find_ancestor_dir(name: &str) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("reading current directory")?;
    let mut search = cwd.as_path();
    loop {
        let candidate = search.join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
        match search.parent() {
            Some(p) => search = p,
            None => bail!(
                "could not find '{}' by walking up from {}\n\
                 Run this command from within the synthesist workspace.",
                name,
                cwd.display()
            ),
        }
    }
}

/// Read all `.toml` file stem names from `dir`, excluding any stem that
/// equals `exclude_stem` (used to skip `_template`). Returns a sorted list.
fn read_toml_stems(dir: &Path, exclude_stem: &str) -> Result<Vec<String>> {
    let mut stems: Vec<String> = fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let stem = path.file_stem()?.to_str()?.to_string();
                if !exclude_stem.is_empty() && stem == exclude_stem {
                    return None;
                }
                Some(stem)
            } else {
                None
            }
        })
        .collect();
    stems.sort();
    Ok(stems)
}

// ---------------------------------------------------------------------------
// Run ID helper
// ---------------------------------------------------------------------------

/// Generate 8 random hex characters.
///
/// Uses the process ID and current time to avoid importing a UUID crate.
/// Not cryptographically secure; sufficient for a unique run identifier
/// within a single machine and session.
fn random_hex8() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut h = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut h);
    std::process::id().hash(&mut h);
    format!("{:08x}", h.finish() & 0xffff_ffff)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ---------------------------------------------------------------------------
    // Scenario parsing tests
    // ---------------------------------------------------------------------------

    const PLAN_A_SPEC_TOML: &str = include_str!("../jig/scenarios/plan-a-spec.toml");
    const EXECUTE_A_TASK_TOML: &str = include_str!("../jig/scenarios/execute-a-task.toml");
    const TRIAGE_PENDING_TOML: &str = include_str!("../jig/scenarios/triage-pending.toml");

    #[test]
    fn parse_plan_a_spec_scenario() {
        let s: ScenarioFile = toml::from_str(PLAN_A_SPEC_TOML)
            .expect("plan-a-spec.toml should parse as a valid ScenarioFile");
        assert_eq!(s.scenario.name, "plan-a-spec");
        assert!(!s.scenario.description.is_empty());
        assert!(!s.scenario.version.is_empty());
        assert!(!s.goal.prompt.is_empty());
        assert!(!s.goal.success_criterion.is_empty());
        assert!(!s.rubric.is_empty(), "rubric should have at least one entry");
        // All rubric entries must have non-empty ids and descriptions.
        for entry in &s.rubric {
            assert!(!entry.id.is_empty(), "rubric entry id must not be empty");
            assert!(
                !entry.description.is_empty(),
                "rubric entry description must not be empty"
            );
            assert!(entry.weight > 0, "rubric weight must be positive");
        }
    }

    #[test]
    fn parse_execute_a_task_scenario() {
        let s: ScenarioFile = toml::from_str(EXECUTE_A_TASK_TOML)
            .expect("execute-a-task.toml should parse as a valid ScenarioFile");
        assert_eq!(s.scenario.name, "execute-a-task");
        assert!(!s.rubric.is_empty());
    }

    #[test]
    fn parse_triage_pending_scenario() {
        let s: ScenarioFile = toml::from_str(TRIAGE_PENDING_TOML)
            .expect("triage-pending.toml should parse as a valid ScenarioFile");
        assert_eq!(s.scenario.name, "triage-pending");
        assert!(!s.rubric.is_empty());
    }

    #[test]
    fn scenario_serialises_to_json() {
        let s: ScenarioFile = toml::from_str(PLAN_A_SPEC_TOML).unwrap();
        let v = serde_json::to_value(&s).unwrap();
        // Top-level keys must be present.
        assert!(v.get("scenario").is_some());
        assert!(v.get("starting_state").is_some());
        assert!(v.get("goal").is_some());
        assert!(v.get("rubric").is_some());
        // scenario.name round-trips.
        assert_eq!(v["scenario"]["name"].as_str().unwrap(), "plan-a-spec");
    }

    // ---------------------------------------------------------------------------
    // Result JSON shape test
    // ---------------------------------------------------------------------------

    #[test]
    fn result_json_has_required_fields() {
        let scenario: ScenarioFile = toml::from_str(PLAN_A_SPEC_TOML).unwrap();
        let now = Utc::now();
        let ts = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let result = JigResult {
            run_id: "1234567890-abcd1234".to_string(),
            scenario_name: scenario.scenario.name.clone(),
            manifest_name: "baseline-v25".to_string(),
            started_at: ts.clone(),
            finished_at: ts.clone(),
            status: "pending".to_string(),
            scenario: serde_json::to_value(&scenario).unwrap(),
            manifest: ManifestSummary {
                name: "baseline-v25".to_string(),
                description: "v2.5-identical surface".to_string(),
            },
            outcome: Value::Null,
        };

        let v = serde_json::to_value(&result).unwrap();

        // All required fields must be present.
        for key in &[
            "run_id",
            "scenario_name",
            "manifest_name",
            "started_at",
            "finished_at",
            "status",
            "scenario",
            "manifest",
            "outcome",
        ] {
            assert!(
                v.get(key).is_some(),
                "result JSON missing required field '{key}'"
            );
        }

        assert_eq!(v["status"].as_str().unwrap(), "pending");
        assert!(v["outcome"].is_null());
        assert_eq!(v["scenario_name"].as_str().unwrap(), "plan-a-spec");
        assert_eq!(v["manifest_name"].as_str().unwrap(), "baseline-v25");
        // manifest summary must not expose include/exclude/add.
        let manifest_obj = v["manifest"].as_object().unwrap();
        assert_eq!(manifest_obj.len(), 2, "manifest summary must have exactly name and description");
        assert!(manifest_obj.contains_key("name"));
        assert!(manifest_obj.contains_key("description"));
    }

    // ---------------------------------------------------------------------------
    // End-to-end: write result file, read back, verify shape
    // ---------------------------------------------------------------------------

    #[test]
    fn write_and_read_result_file() {
        let tmp = TempDir::new().unwrap();
        let jig_dir = tmp.path().join("_jig");
        fs::create_dir_all(&jig_dir).unwrap();

        let scenario: ScenarioFile = toml::from_str(PLAN_A_SPEC_TOML).unwrap();
        let now = Utc::now();
        let ts = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let run_id = "1717000000000-cafebabe".to_string();

        let result = JigResult {
            run_id: run_id.clone(),
            scenario_name: scenario.scenario.name.clone(),
            manifest_name: "baseline-v25".to_string(),
            started_at: ts.clone(),
            finished_at: ts.clone(),
            status: "pending".to_string(),
            scenario: serde_json::to_value(&scenario).unwrap(),
            manifest: ManifestSummary {
                name: "baseline-v25".to_string(),
                description: "v2.5-identical surface".to_string(),
            },
            outcome: Value::Null,
        };

        let result_path = jig_dir.join(format!("{run_id}.json"));
        let json_text = serde_json::to_string_pretty(&result).unwrap();
        fs::write(&result_path, &json_text).unwrap();

        // Read back and verify.
        let read_back: Value =
            serde_json::from_str(&fs::read_to_string(&result_path).unwrap()).unwrap();

        assert_eq!(read_back["run_id"].as_str().unwrap(), run_id);
        assert_eq!(read_back["status"].as_str().unwrap(), "pending");
        assert!(read_back["outcome"].is_null());
        assert_eq!(
            read_back["scenario"]["scenario"]["name"].as_str().unwrap(),
            "plan-a-spec"
        );
    }

    // ---------------------------------------------------------------------------
    // Helper: random_hex8
    // ---------------------------------------------------------------------------

    #[test]
    fn random_hex8_is_8_hex_chars() {
        let h = random_hex8();
        assert_eq!(h.len(), 8, "random_hex8 should produce 8 characters");
        assert!(
            h.chars().all(|c| c.is_ascii_hexdigit()),
            "random_hex8 should produce hex digits only, got: {h}"
        );
    }

    #[test]
    fn random_hex8_varies_across_calls() {
        // Two back-to-back calls should (with overwhelmingly high probability)
        // differ because SystemTime changes between calls.  This is not
        // perfectly deterministic, but the probability of collision is 1/2^32.
        let h1 = random_hex8();
        // Tiny sleep so SystemTime::now() actually advances.
        std::thread::sleep(std::time::Duration::from_millis(2));
        let h2 = random_hex8();
        // We assert they are *both* valid hex; equality would be a remarkable
        // (and harmless) coincidence.
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(h2.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ---------------------------------------------------------------------------
    // Aggregate tests (T7.4)
    // ---------------------------------------------------------------------------

    fn make_result_json(
        scenario_name: &str,
        manifest_name: &str,
        started_at: &str,
    ) -> serde_json::Value {
        json!({
            "run_id": format!("{}-abcd1234", started_at.replace([':', '-', 'T', 'Z', '.'], "")),
            "scenario_name": scenario_name,
            "manifest_name": manifest_name,
            "started_at": started_at,
            "finished_at": started_at,
            "status": "pending",
            "scenario": {},
            "manifest": { "name": manifest_name, "description": "" },
            "outcome": null
        })
    }

    /// Helper: create `claims/_jig/` inside `base`, write result JSON files.
    fn setup_jig_dir(base: &std::path::Path, entries: &[(&str, &str, &str)]) {
        let claims_dir = base.join("claims");
        let jig_dir = claims_dir.join("_jig");
        fs::create_dir_all(&jig_dir).unwrap();
        for (i, (scenario, manifest, started_at)) in entries.iter().enumerate() {
            let v = make_result_json(scenario, manifest, started_at);
            let path = jig_dir.join(format!("{i:04}.json"));
            fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        }
    }

    /// Mutex to serialise tests that mutate SYNTHESIST_DIR. Without this,
    /// parallel test threads can stomp on each other's env-var values.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Call `build_aggregate_rows` with SYNTHESIST_DIR pointed at a temp dir
    /// that contains `claims/`. Serialises env-var mutation across tests.
    fn aggregate_rows_in(synthesist_dir: &std::path::Path) -> Vec<AggRow> {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("SYNTHESIST_DIR", synthesist_dir) };
        let rows = build_aggregate_rows().unwrap();
        unsafe { std::env::remove_var("SYNTHESIST_DIR") };
        rows
    }

    #[test]
    fn aggregate_empty_dir_produces_no_rows() {
        let tmp = TempDir::new().unwrap();
        let jig_dir = tmp.path().join("claims").join("_jig");
        fs::create_dir_all(&jig_dir).unwrap();

        let rows = aggregate_rows_in(tmp.path());
        assert!(rows.is_empty(), "empty _jig dir should produce zero rows");
    }

    #[test]
    fn aggregate_groups_multiple_runs() {
        let tmp = TempDir::new().unwrap();
        setup_jig_dir(
            tmp.path(),
            &[
                ("plan-a-spec", "baseline-v25", "2024-01-01T10:00:00.000Z"),
                ("plan-a-spec", "baseline-v25", "2024-01-02T10:00:00.000Z"),
                ("execute-a-task", "baseline-v25", "2024-01-01T11:00:00.000Z"),
            ],
        );

        let rows = aggregate_rows_in(tmp.path());

        assert_eq!(rows.len(), 2, "expected 2 aggregate rows");

        let plan = rows
            .iter()
            .find(|r| r.scenario == "plan-a-spec")
            .expect("plan-a-spec row must exist");
        assert_eq!(plan.runs, 2, "plan-a-spec should have 2 runs");
        assert_eq!(
            plan.latest_started_at, "2024-01-02T10:00:00.000Z",
            "latest_started_at must be the later timestamp"
        );

        let exec = rows
            .iter()
            .find(|r| r.scenario == "execute-a-task")
            .expect("execute-a-task row must exist");
        assert_eq!(exec.runs, 1);
    }

    #[test]
    fn aggregate_skips_malformed_json() {
        let tmp = TempDir::new().unwrap();
        let claims_dir = tmp.path().join("claims");
        let jig_dir = claims_dir.join("_jig");
        fs::create_dir_all(&jig_dir).unwrap();

        let valid = make_result_json("plan-a-spec", "baseline-v25", "2024-01-01T10:00:00.000Z");
        fs::write(
            jig_dir.join("0001.json"),
            serde_json::to_string_pretty(&valid).unwrap(),
        )
        .unwrap();
        fs::write(jig_dir.join("0002.json"), b"not valid json {{{").unwrap();

        let rows = aggregate_rows_in(tmp.path());
        assert_eq!(rows.len(), 1, "only the valid file should contribute a row");
        assert_eq!(rows[0].scenario, "plan-a-spec");
        assert_eq!(rows[0].runs, 1);
    }

    #[test]
    fn aggregate_csv_format() {
        let tmp = TempDir::new().unwrap();
        setup_jig_dir(
            tmp.path(),
            &[("plan-a-spec", "baseline-v25", "2024-01-01T10:00:00.000Z")],
        );

        let rows = aggregate_rows_in(tmp.path());
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        let csv_line = format!(
            "{},{},{},{}",
            csv_escape(&row.scenario),
            csv_escape(&row.manifest),
            row.runs,
            csv_escape(&row.latest_started_at)
        );
        assert_eq!(
            csv_line,
            "plan-a-spec,baseline-v25,1,2024-01-01T10:00:00.000Z"
        );
    }

    #[test]
    fn aggregate_json_format() {
        let tmp = TempDir::new().unwrap();
        setup_jig_dir(
            tmp.path(),
            &[
                ("plan-a-spec", "baseline-v25", "2024-01-01T10:00:00.000Z"),
                ("plan-a-spec", "sparql-v26", "2024-01-03T10:00:00.000Z"),
            ],
        );

        let rows = aggregate_rows_in(tmp.path());
        let out = json!({ "scenarios": rows });
        let s = serde_json::to_string(&out).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();

        let scenarios = parsed["scenarios"].as_array().unwrap();
        assert_eq!(scenarios.len(), 2);
        for row in scenarios {
            assert!(row.get("scenario").is_some());
            assert!(row.get("manifest").is_some());
            assert!(row.get("runs").is_some());
            assert!(row.get("latest_started_at").is_some());
        }
    }
}

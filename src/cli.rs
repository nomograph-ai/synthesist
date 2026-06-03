//! CLI type definitions (clap derive) and manifest-filtered builder.
//!
//! Every argument and option has a help string. These descriptions are the
//! LLM's first contact with the tool when it runs `--help`.
//!
//! # Manifest-filtered builder
//!
//! `build_app(manifest)` constructs an equivalent `clap::Command` tree using
//! the builder API. Each subcommand is registered with a manifest key (the
//! string used in `[commands] include = [...]` in surface manifest TOML files).
//! Commands not permitted by the manifest are omitted from the returned
//! `clap::Command`.
//!
//! The derive-based `Cli` / `Command` enums are kept so that `Cli::parse()`
//! (used in `main.rs`) continues to compile and dispatch correctly. The
//! manifest-aware builder is for skill emission and manifest introspection.

use std::path::PathBuf;

use clap::{Arg, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "synthesist",
    version = env!("CARGO_PKG_VERSION"),
    about = "Specification graph manager for AI-augmented projects",
    after_help = "All output is JSON. Writes append typed claims to per-asserter logs under claims/; reads query a disposable redb gamma index rebuilt from them.\nRun 'synthesist skill' for the full behavioral contract and worked examples."
)]
pub struct Cli {
    /// Session ID for write operations. Sets the asserter class on every
    /// appended claim so work is attributable. Required for all write ops.
    #[arg(long, env = "SYNTHESIST_SESSION", global = true)]
    pub session: Option<String>,

    /// Path to the synthesist data directory (contains claims/).
    /// Overrides SYNTHESIST_DIR and the parent-directory walk.
    /// Use in worktrees or detached checkouts to point at the main data dir.
    #[arg(long, global = true, value_name = "PATH")]
    pub data_dir: Option<PathBuf>,

    /// Skip phase enforcement and phase transition validation.
    #[arg(long, global = true)]
    pub force: bool,

    /// One-shot active surface override: a builtin manifest name (see
    /// `surface list`) or a path to a manifest TOML file. Takes precedence
    /// over `SYNTHESIST_MANIFEST` and the sticky `surface use` setting for
    /// this invocation only. Governs which commands the runtime permits.
    #[arg(long, global = true, value_name = "NAME-OR-PATH")]
    pub manifest: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize synthesist in the current directory. Creates the claims/ directory.
    Init,
    /// Estate overview: trees, task counts, ready tasks, sessions, phase.
    Status,
    /// Validate referential integrity across all tables.
    Check,
    /// List prior claims superseded by more than one live successor
    /// (diamond conflicts). Read-only; needs no session.
    Conflicts,

    /// Manage trees (top-level project domains).
    Tree {
        #[command(subcommand)]
        cmd: TreeCmd,
    },
    /// Manage specs (units of work within a tree).
    Spec {
        #[command(subcommand)]
        cmd: SpecCmd,
    },
    /// Manage tasks (atomic work items forming a dependency DAG).
    Task {
        #[command(subcommand)]
        cmd: TaskCmd,
    },
    /// Record findings (institutional memory).
    Discovery {
        #[command(subcommand)]
        cmd: DiscoveryCmd,
    },
    /// Moved to `lattice` in v2. Any invocation prints a pointer to the
    /// lattice install path; args are swallowed.
    Stakeholder {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        #[allow(dead_code)]
        args: Vec<String>,
    },
    /// Moved to `lattice` in v2. Any invocation prints a pointer to the
    /// lattice install path; args are swallowed.
    Disposition {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        #[allow(dead_code)]
        args: Vec<String>,
    },
    /// Moved to `lattice` in v2. Any invocation prints a pointer to the
    /// lattice install path; args are swallowed.
    Signal {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        #[allow(dead_code)]
        args: Vec<String>,
    },
    /// Moved to `lattice` in v2. Any invocation prints a pointer to the
    /// lattice install path; args are swallowed.
    Stance {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        #[allow(dead_code)]
        args: Vec<String>,
    },
    /// Manage campaigns (cross-tree spec coordination).
    Campaign {
        #[command(subcommand)]
        cmd: CampaignCmd,
    },
    /// Manage sessions (claim-scoped asserter namespaces for concurrent work).
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },
    /// Manage the 7-phase workflow state machine.
    Phase {
        #[command(subcommand)]
        cmd: PhaseCmd,
    },
    /// List, inspect, and run schema migrations (v2-to-v3 and future transitions).
    Migrate {
        #[command(subcommand)]
        cmd: MigrateCmd,
    },
    /// Export all tables as JSON (for backup or migration).
    Export,
    /// Import tables from JSON (stdin if no file given).
    Import {
        /// Path to JSON file. Reads stdin if omitted.
        file: Option<String>,
    },
    /// Emit the full skill file (behavioral contract + command reference).
    ///
    /// Without `--manifest`, the baseline v2.5 surface is used. With
    /// `--manifest <path>`, the generated skill document reflects the surface
    /// declared by that manifest.
    Skill {
        /// Path to a surface manifest TOML file. If absent, the baseline
        /// v2.5 surface is used and the output is identical to the default
        /// `synthesist skill` output.
        #[arg(long, value_name = "PATH")]
        manifest: Option<PathBuf>,
    },
    /// Show version and check for updates from GitLab releases.
    Version {
        /// Skip the network check for latest version.
        #[arg(long)]
        offline: bool,
    },
    /// Record what happened to a spec (completed, abandoned, deferred,
    /// or absorbed by another spec). Distinct from Spec status, which
    /// expresses the spec's current state. Each Outcome is an
    /// independent claim with its own asserter and timestamp.
    Outcome {
        #[command(subcommand)]
        cmd: OutcomeCmd,
    },
    /// Named analysis passes over the redb gamma index. Each overlay
    /// runs a typed pass and returns structured hits. Read-only; no
    /// session or phase gate applies.
    Overlay {
        #[command(subcommand)]
        cmd: OverlayCmd,
    },
    /// Run canonical scenarios under surface manifests and record results.
    /// Results land in `claims/_jig/<run_id>.json`. Read-only; no session
    /// or phase gate applies.
    Jig {
        #[command(subcommand)]
        cmd: JigCmd,
    },
    /// Inspect and switch the active surface manifest.
    ///
    /// The active surface governs which commands the runtime permits:
    /// invoking a command the active manifest does not expose is rejected
    /// before dispatch. `surface` itself is always permitted, regardless of
    /// the active manifest, so an operator can never lock themselves out.
    Surface {
        #[command(subcommand)]
        cmd: SurfaceCmd,
    },
}

// --- Surface ---

#[derive(Subcommand)]
pub enum SurfaceCmd {
    /// Persist the active surface manifest for this estate (sticky setting).
    /// Accepts a builtin manifest name (see `surface list`) or a path to a
    /// manifest TOML file. Emits JSON `{"ok":true,"active":"<name>"}`.
    Use {
        /// Builtin manifest name or path to a manifest TOML file.
        name: String,
    },
    /// List the builtin manifest names and mark which surface is active.
    List,
    /// Show the active manifest name and the command keys it enables.
    Show,
}

// --- Outcome ---

#[derive(Subcommand)]
pub enum OutcomeCmd {
    /// Record a new Outcome claim against a spec.
    Add {
        /// Path in tree/spec format.
        tree_spec: String,
        /// Outcome status: completed, abandoned, deferred, superseded_by.
        #[arg(
            long,
            value_parser = clap::builder::PossibleValuesParser::new(crate::schema::outcome::STATUSES)
        )]
        status: String,
        /// Optional note explaining the outcome.
        #[arg(long)]
        note: Option<String>,
        /// Required when --status is `superseded_by`. Names the
        /// absorbing spec (tree/id form). Schema rejects the claim
        /// if missing for that status; harmless for other statuses.
        #[arg(long, required_if_eq("status", "superseded_by"))]
        linked_spec: Option<String>,
        /// ISO date (YYYY-MM-DD); defaults to today.
        #[arg(long)]
        date: Option<String>,
    },
    /// List Outcome claims recorded against a spec.
    List {
        /// Path in tree/spec format.
        tree_spec: String,
    },
}

// --- Tree ---

#[derive(Subcommand)]
pub enum TreeCmd {
    /// Add a tree (e.g. "upstream", "harness", "lever").
    Add {
        /// Tree name. Short, lowercase, no spaces.
        name: String,
        /// Human-readable description.
        #[arg(long, default_value = "")]
        description: String,
        /// Tree status (e.g. "active" or "closed").
        #[arg(long, default_value = "active")]
        status: String,
    },
    /// List all trees in the estate. Hides closed trees by default.
    List {
        /// Include trees whose latest claim has status "closed".
        #[arg(long)]
        include_closed: bool,
    },
    /// Show a single tree's metadata: name, description, spec count.
    Show {
        /// Tree name.
        name: String,
    },
    /// Close a tree. Appends a superseding `Tree` claim marking the
    /// tree `closed`. Non-destructive: prior claims, specs, and
    /// sessions remain in the log. Hidden from `tree list` by default;
    /// use `tree list --include-closed` to surface.
    Close {
        /// Tree name to close.
        name: String,
        /// Disambiguate by start_id (the claim hash of the original
        /// `tree add` claim) when multiple trees share `name`. Accepts
        /// a full 64-char hex hash or any unambiguous prefix.
        #[arg(long)]
        start_id: Option<String>,
    },
}

// --- Spec ---

#[derive(Subcommand)]
pub enum SpecCmd {
    /// Add a spec (e.g. "upstream/auth-migration").
    Add {
        /// Path in tree/spec format (e.g. "upstream/auth").
        tree_spec: String,
        /// What this spec aims to achieve.
        #[arg(long)]
        goal: Option<String>,
        /// Boundaries and invariants.
        #[arg(long)]
        constraints: Option<String>,
        /// Key decisions already made.
        #[arg(long)]
        decisions: Option<String>,
    },
    /// Show full spec detail (goal, constraints, decisions, status, outcome).
    Show {
        /// Path in tree/spec format.
        tree_spec: String,
    },
    /// Update spec fields.
    Update {
        /// Path in tree/spec format.
        tree_spec: String,
        /// What this spec aims to achieve.
        #[arg(long)]
        goal: Option<String>,
        /// Boundaries and invariants.
        #[arg(long)]
        constraints: Option<String>,
        /// Key decisions already made.
        #[arg(long)]
        decisions: Option<String>,
        /// Spec status. Allowed values come from the same constant
        /// the schema validator uses, so CLI accepts iff schema
        /// accepts. To record completed / abandoned / deferred,
        /// use `synthesist outcome add`; those are Outcome claim
        /// values, not Spec status values.
        #[arg(long, value_parser = parse_spec_status)]
        status: Option<String>,
        /// What happened (set when completing or archiving).
        #[arg(long)]
        outcome: Option<String>,
        /// Pin the agree-time plan snapshot. Comma-separated claim
        /// IDs of the Task claims that constitute the agreed plan.
        /// Drives the `plan-at-risk` overlay: when any of these
        /// claims is later superseded, the spec is flagged. Pass an
        /// empty string to clear. Pin this while still in PLAN, before
        /// running `phase set agree`: AGREE forbids writes, so the
        /// snapshot cannot be pinned once the phase has transitioned.
        #[arg(long, value_delimiter = ',')]
        agree_snapshot: Option<Vec<String>>,
    },
    /// List all specs in a tree. Tree may be passed as positional or
    /// via `--tree <name>`. Agents reach for both shapes; jig surfaced
    /// the flag form as a frequent invention.
    List {
        /// Tree name (positional form).
        #[arg(value_name = "TREE")]
        tree: Option<String>,
        /// Tree name (flag form, equivalent to positional).
        #[arg(
            long = "tree",
            value_name = "TREE",
            conflicts_with = "tree",
            id = "tree_flag"
        )]
        tree_flag: Option<String>,
    },
}

// --- Task ---

#[derive(Subcommand)]
pub enum TaskCmd {
    /// Add a task to a spec's DAG. IDs auto-generate as t1, t2, ... or use --id.
    Add {
        /// Path in tree/spec format (e.g. "upstream/auth").
        tree_spec: String,
        /// What this task accomplishes. One sentence.
        summary: String,
        /// Custom task ID (default: auto-increment t1, t2, ...).
        #[arg(long)]
        id: Option<String>,
        /// Comma-separated IDs of tasks this depends on (e.g. "t1,t2").
        #[arg(long, value_delimiter = ',')]
        depends_on: Vec<String>,
        /// Gate type. Use "human" for tasks requiring human approval.
        #[arg(long)]
        gate: Option<String>,
        /// Comma-separated file paths this task touches.
        #[arg(long, value_delimiter = ',')]
        files: Vec<String>,
        /// Detailed description of the task.
        #[arg(long)]
        description: Option<String>,
    },
    /// List all tasks in a spec.
    List {
        /// Path in tree/spec format.
        tree_spec: String,
        /// Human-readable table output.
        #[arg(long)]
        human: bool,
        /// Hide cancelled tasks.
        #[arg(long)]
        active: bool,
    },
    /// Show full task detail including dependencies, files, and acceptance criteria.
    Show {
        /// Path in tree/spec format.
        tree_spec: String,
        /// Task ID (e.g. "t1").
        task_id: String,
    },
    /// Update task summary, description, files, or dependencies.
    Update {
        /// Path in tree/spec format.
        tree_spec: String,
        /// Task ID.
        task_id: String,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Replace file list (comma-separated).
        #[arg(long, value_delimiter = ',')]
        files: Option<Vec<String>>,
        /// Replace dependency list (comma-separated task IDs in the
        /// same spec). Pass an empty string to clear deps. Validates:
        /// no self-dependency, no cycles, every referenced ID must
        /// exist in the same spec. A new dep that is itself in
        /// `cancelled` status is allowed (rewiring away from
        /// cancelled predecessors is the use case) and surfaces as a
        /// warning in the JSON output.
        #[arg(long, value_delimiter = ',')]
        depends_on: Option<Vec<String>>,
    },
    /// Claim a task: pending -> in_progress. Sets owner to session ID.
    Claim {
        /// Path in tree/spec format.
        tree_spec: String,
        /// Task ID.
        task_id: String,
    },
    /// Complete a task: in_progress -> done. Runs acceptance criteria if any.
    Done {
        /// Path in tree/spec format.
        tree_spec: String,
        /// Task ID.
        task_id: String,
        /// Skip running acceptance criteria verify commands.
        #[arg(long)]
        skip_verify: bool,
    },
    /// Reset orphaned task: in_progress -> pending. For crash recovery.
    Reset {
        /// Path in tree/spec format (omit for --session bulk reset).
        tree_spec: Option<String>,
        /// Task ID (omit for --session bulk reset).
        task_id: Option<String>,
        /// Reset all in_progress tasks owned by this session.
        #[arg(long)]
        session: Option<String>,
        /// Why the task is being reset.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Block a task: pending or in_progress -> blocked.
    Block {
        /// Path in tree/spec format.
        tree_spec: String,
        /// Task ID.
        task_id: String,
    },
    /// Set a task to waiting with a reason.
    Wait {
        /// Path in tree/spec format.
        tree_spec: String,
        /// Task ID.
        task_id: String,
        /// Why this task is waiting (e.g. "waiting on MR !123").
        #[arg(long)]
        reason: String,
    },
    /// Cancel a task. Cannot cancel done tasks.
    Cancel {
        /// Path in tree/spec format.
        tree_spec: String,
        /// Task ID.
        task_id: String,
        /// Why this task is being cancelled.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Show tasks that are pending with all dependencies done.
    Ready {
        /// Path in tree/spec format.
        tree_spec: String,
    },
    /// Add an acceptance criterion with a shell command to verify it.
    Acceptance {
        /// Path in tree/spec format.
        tree_spec: String,
        /// Task ID.
        task_id: String,
        /// What must be true (e.g. "all tests pass").
        #[arg(long)]
        criterion: String,
        /// Shell command to verify (e.g. "cargo test").
        #[arg(long)]
        verify: String,
    },
}

// --- Discovery ---

#[derive(Subcommand)]
pub enum DiscoveryCmd {
    /// Record a finding. Append-only institutional memory.
    Add {
        /// Path in tree/spec format.
        tree_spec: String,
        /// What was discovered. One or two sentences.
        #[arg(long)]
        finding: String,
        /// How significant this is (e.g. "high", "changes approach").
        #[arg(long)]
        impact: Option<String>,
        /// What action was taken in response.
        #[arg(long)]
        action: Option<String>,
        /// Who made this discovery.
        #[arg(long)]
        author: Option<String>,
        /// Date of discovery (YYYY-MM-DD, default: today).
        #[arg(long)]
        date: Option<String>,
    },
    /// List all discoveries for a spec.
    List {
        /// Path in tree/spec format.
        tree_spec: String,
    },
}

// Stakeholder / Disposition / Signal / Stance subcommand enums were
// removed in v2.1 -- the families moved to `lattice` entirely. The
// top-level commands now swallow any args via `trailing_var_arg` so
// clap parse succeeds and `moved_to_lattice` in main.rs can print the
// pointer message instead of a cryptic clap error.

// --- Campaign ---

#[derive(Subcommand)]
pub enum CampaignCmd {
    /// Add a spec to a campaign (active or backlog).
    Add {
        /// Tree name.
        tree: String,
        /// Spec ID to add to campaign.
        spec_id: String,
        /// Campaign summary for this spec.
        #[arg(long, default_value = "")]
        summary: String,
        /// Add to backlog instead of active list.
        #[arg(long)]
        backlog: bool,
        /// Title (for backlog items).
        #[arg(long)]
        title: Option<String>,
        /// Comma-separated spec IDs that block this one.
        #[arg(long, value_delimiter = ',')]
        blocked_by: Vec<String>,
    },
    /// List campaign specs (active and backlog).
    List {
        /// Tree name.
        tree: String,
    },
}

// --- Session ---

#[derive(Subcommand)]
pub enum SessionCmd {
    /// Start a session. Claims written with --session=<id> carry the
    /// session in their asserter (user:local:<user>:<id>), providing
    /// attribution and concurrent-work namespacing.
    Start {
        /// Session ID. Short, unique (e.g. "research", "factory-01").
        id: String,
        /// Tree this session is working on (metadata only).
        #[arg(long)]
        tree: Option<String>,
        /// Spec this session is working on (metadata only).
        #[arg(long)]
        spec: Option<String>,
        /// What this session is doing.
        #[arg(long)]
        summary: Option<String>,
    },
    /// Removed in v2. Bails with a pointer to `session close` and
    /// `synthesist conflicts`; retained so v1 muscle memory gets a
    /// specific error instead of clap's "unrecognized subcommand".
    Merge {
        /// Session ID to merge.
        id: String,
        /// Show what would change without applying.
        #[arg(long)]
        dry_run: bool,
        /// On conflict, keep main's values (discard session changes for conflicting rows).
        #[arg(long)]
        ours: bool,
        /// On conflict, keep session's values (overwrite main for conflicting rows).
        #[arg(long)]
        theirs: bool,
    },
    /// List all sessions (active, merged, discarded).
    List,
    /// Show what changed in a session vs main (three-way diff summary).
    Status {
        /// Session ID.
        id: String,
    },
    /// Removed in v2. Bails with a pointer to `session close`; v1-era
    /// discard semantics do not exist on an append-only log.
    Discard {
        /// Session ID.
        id: String,
    },
    /// Close a session. Appends a superseding `Session` claim marking
    /// the session `closed`. Non-destructive: prior work stays in the log.
    Close {
        /// Session ID to close.
        id: String,
        /// Disambiguate by start_id (the claim hash of the opening
        /// `Session` claim) when multiple sessions share the same
        /// display id. Accepts a full 64-char hex hash or any
        /// unambiguous prefix. Without this flag, behavior is
        /// unchanged: the most recent live opener for `id` is closed.
        #[arg(long)]
        start_id: Option<String>,
    },
}

// --- Migrate ---

#[derive(Subcommand)]
pub enum MigrateCmd {
    /// List all registered migrations (chain order).
    List,
    /// Show current schema version and pending migrations.
    Status,
    /// Run migrations from current schema version to target (or latest).
    Run {
        /// Target schema version. Default: latest registered.
        #[arg(long)]
        target: Option<String>,
        /// Plan the chain and report without writing.
        #[arg(long)]
        dry_run: bool,
        /// Skip the tarball backup. Default: backup is written.
        #[arg(long)]
        no_backup: bool,
    },
    /// Convenience shortcut for the v2-to-v3 migration.
    #[command(name = "v2-to-v3")]
    V2ToV3 {
        #[arg(long)]
        dry_run: bool,
    },
}

// --- Phase ---

#[derive(Subcommand)]
pub enum PhaseCmd {
    /// Set the workflow phase. Transitions are validated against the state machine.
    /// Valid: orient->plan->agree->execute->reflect->replan->report.
    /// Use --force to override transition validation.
    Set {
        /// Phase name: orient, plan, agree, execute, reflect, replan, report.
        name: String,
    },
    /// Show the current workflow phase. `phase get` is an alias for
    /// agents that reach for the get/set verb pairing.
    #[command(alias = "get")]
    Show,
}

// --- Overlay ---

#[derive(Subcommand)]
pub enum OverlayCmd {
    /// List all registered overlays with their names and descriptions.
    List,
    /// Run a named overlay against the current graph view and print hits as JSON.
    Run {
        /// Overlay name (see `overlay list` for available names).
        name: String,
    },
}

// --- Jig ---

#[derive(Subcommand)]
pub enum JigCmd {
    /// Run a named scenario under a named manifest and write a result JSON
    /// to `claims/_jig/<run_id>.json`. For v3-alpha the LLM session is not
    /// invoked; this command records the setup with `status: "pending"`.
    Run {
        /// Scenario name (file stem under `jig/scenarios/<name>.toml`).
        #[arg(long, value_name = "NAME")]
        scenario: String,
        /// Manifest name (file stem under `surface/<name>.toml`).
        #[arg(long, value_name = "NAME")]
        manifest: String,
    },
    /// List available scenarios found in `jig/scenarios/`.
    #[command(name = "list-scenarios")]
    ListScenarios,
    /// List available manifests found in `surface/`.
    #[command(name = "list-manifests")]
    ListManifests,
    /// Aggregate all `claims/_jig/*.json` result files into a comparison
    /// table grouped by (scenario, manifest). Counts runs and records the
    /// latest started_at per group. Pure read: no result files are modified.
    /// Malformed JSON files emit a stderr warning and are skipped.
    Aggregate {
        /// Output format: md (default), csv, or json.
        #[arg(long, default_value = "md", value_parser = ["md", "csv", "json"])]
        format: String,
    },
}

// ---------------------------------------------------------------------------
// Manifest-filtered builder
// ---------------------------------------------------------------------------

/// All manifest keys recognised by this registry.
///
/// Each entry is a `(key, is_v25_baseline)` pair. The `key` matches the
/// strings used in surface manifest TOML files (`[commands] include = [...]`).
/// Keys not in the baseline require an explicit `add` entry in the manifest.
///
/// Sub-commands that were retired or are internal-only (stakeholder,
/// disposition, signal, stance) are intentionally absent: they were removed
/// from the user-facing surface in v2.1 and are not registered in manifests.
const REGISTRY: &[(&str, bool)] = &[
    // --- top-level read/utility ---
    ("status",             true),
    ("check",              true),
    ("conflicts",          true),
    ("init",               true),
    ("export",             true),
    ("import",             true),
    ("skill",              true),
    ("version",            true),
    // --- tree ---
    ("tree add",           true),
    ("tree list",          true),
    ("tree show",          true),
    ("tree close",         true),
    // --- spec ---
    ("spec add",           true),
    ("spec show",          true),
    ("spec update",        true),
    ("spec list",          true),
    // --- task ---
    ("task add",           true),
    ("task list",          true),
    ("task show",          true),
    ("task update",        true),
    ("task claim",         true),
    ("task done",          true),
    ("task reset",         true),
    ("task block",         true),
    ("task wait",          true),
    ("task cancel",        true),
    ("task ready",         true),
    ("task acceptance",    true),
    // --- discovery ---
    ("discovery add",      true),
    ("discovery list",     true),
    // --- campaign ---
    ("campaign add",       true),
    ("campaign list",      true),
    // --- session ---
    ("session start",      true),
    ("session close",      true),
    ("session list",       true),
    ("session status",     true),
    // --- phase ---
    ("phase show",         true),
    ("phase set",          true),
    // --- migrate ---
    ("migrate list",       true),
    ("migrate status",     true),
    ("migrate run",        true),
    ("migrate v2-to-v3",   true),
    // --- outcome ---
    ("outcome add",        true),
    ("outcome list",       true),
    // --- v3-alpha additions (not in v2.5 baseline) ---
    ("overlay list",       false),
    ("overlay run",        false),
    ("jig run",            false),
    ("jig list-scenarios", false),
    ("jig list-manifests", false),
    // --- surface (always permitted; never blocked by a manifest) ---
    ("surface use",        true),
    ("surface list",       true),
    ("surface show",       true),
];

/// Registry keys that are ALWAYS permitted, regardless of the active surface
/// manifest. These commands must never be blockable, so an operator can
/// always recover from a restrictive surface: `surface` lets them switch
/// surfaces, and `version` always reports the build.
///
/// A key whose top-level command is in this set is allowed even when the
/// active manifest would otherwise exclude it.
const ALWAYS_ALLOWED_TOP: &[&str] = &["surface", "version", "init", "skill"];

/// True when `key` is always permitted regardless of the active manifest.
pub fn always_allowed(key: &str) -> bool {
    let top = key.split_whitespace().next().unwrap_or(key);
    ALWAYS_ALLOWED_TOP.contains(&top)
}

/// Map a parsed [`Command`] (including its subcommand) to its REGISTRY key.
///
/// Returns `None` for commands that carry no registry key (e.g. the
/// landscape family that moved to `lattice`, or `session merge`/`discard`
/// removed-in-v2 stubs). Callers treat `None` as "allowed": the rejection
/// layer only blocks commands it can positively identify in the registry.
pub fn command_key(cmd: &Command) -> Option<&'static str> {
    Some(match cmd {
        Command::Init => "init",
        Command::Status => "status",
        Command::Check => "check",
        Command::Conflicts => "conflicts",
        Command::Export => "export",
        Command::Import { .. } => "import",
        Command::Skill { .. } => "skill",
        Command::Version { .. } => "version",
        Command::Tree { cmd } => match cmd {
            TreeCmd::Add { .. } => "tree add",
            TreeCmd::List { .. } => "tree list",
            TreeCmd::Show { .. } => "tree show",
            TreeCmd::Close { .. } => "tree close",
        },
        Command::Spec { cmd } => match cmd {
            SpecCmd::Add { .. } => "spec add",
            SpecCmd::Show { .. } => "spec show",
            SpecCmd::Update { .. } => "spec update",
            SpecCmd::List { .. } => "spec list",
        },
        Command::Task { cmd } => match cmd {
            TaskCmd::Add { .. } => "task add",
            TaskCmd::List { .. } => "task list",
            TaskCmd::Show { .. } => "task show",
            TaskCmd::Update { .. } => "task update",
            TaskCmd::Claim { .. } => "task claim",
            TaskCmd::Done { .. } => "task done",
            TaskCmd::Reset { .. } => "task reset",
            TaskCmd::Block { .. } => "task block",
            TaskCmd::Wait { .. } => "task wait",
            TaskCmd::Cancel { .. } => "task cancel",
            TaskCmd::Ready { .. } => "task ready",
            TaskCmd::Acceptance { .. } => "task acceptance",
        },
        Command::Discovery { cmd } => match cmd {
            DiscoveryCmd::Add { .. } => "discovery add",
            DiscoveryCmd::List { .. } => "discovery list",
        },
        Command::Campaign { cmd } => match cmd {
            CampaignCmd::Add { .. } => "campaign add",
            CampaignCmd::List { .. } => "campaign list",
        },
        Command::Session { cmd } => match cmd {
            SessionCmd::Start { .. } => "session start",
            SessionCmd::Close { .. } => "session close",
            SessionCmd::List => "session list",
            SessionCmd::Status { .. } => "session status",
            // merge/discard were removed in v2 and short-circuit before the
            // rejection layer; they carry no registry key.
            SessionCmd::Merge { .. } | SessionCmd::Discard { .. } => return None,
        },
        Command::Phase { cmd } => match cmd {
            PhaseCmd::Show => "phase show",
            PhaseCmd::Set { .. } => "phase set",
        },
        Command::Migrate { cmd } => match cmd {
            MigrateCmd::List => "migrate list",
            MigrateCmd::Status => "migrate status",
            MigrateCmd::Run { .. } => "migrate run",
            MigrateCmd::V2ToV3 { .. } => "migrate v2-to-v3",
        },
        Command::Outcome { cmd } => match cmd {
            OutcomeCmd::Add { .. } => "outcome add",
            OutcomeCmd::List { .. } => "outcome list",
        },
        Command::Overlay { cmd } => match cmd {
            OverlayCmd::List => "overlay list",
            OverlayCmd::Run { .. } => "overlay run",
        },
        Command::Jig { cmd } => match cmd {
            JigCmd::Run { .. } => "jig run",
            JigCmd::ListScenarios => "jig list-scenarios",
            JigCmd::ListManifests => "jig list-manifests",
            // `jig aggregate` has no registry key; treat as allowed.
            JigCmd::Aggregate { .. } => return None,
        },
        Command::Surface { cmd } => match cmd {
            SurfaceCmd::Use { .. } => "surface use",
            SurfaceCmd::List => "surface list",
            SurfaceCmd::Show => "surface show",
        },
        // Landscape family moved to `lattice`: no registry key.
        Command::Stakeholder { .. }
        | Command::Disposition { .. }
        | Command::Signal { .. }
        | Command::Stance { .. } => return None,
    })
}

/// The set of registry keys permitted by `manifest`, in registry order.
///
/// Used by `surface show` to report the active surface's enabled commands.
pub fn permitted_keys(manifest: &crate::surface::manifest::Manifest) -> Vec<&'static str> {
    REGISTRY
        .iter()
        .filter(|(key, _)| key_permitted(key, manifest))
        .map(|(key, _)| *key)
        .collect()
}

/// Every registry key, in registry order.
///
/// Used by `surface show` when no surface is configured (unfiltered): the full
/// v3 surface is the entire registry.
pub fn all_command_keys() -> Vec<&'static str> {
    REGISTRY.iter().map(|(key, _)| *key).collect()
}

/// Determine whether a command key is permitted by the manifest.
///
/// Logic (applied in order):
/// 1. If the key is in `exclude`, it is always hidden.
/// 2. If `include` is non-empty, the key must appear in `include` OR in `add`.
/// 3. If `include` is empty, the key is permitted when it is a v2.5 baseline
///    command OR it appears in `add`.
pub fn key_permitted(key: &str, manifest: &crate::surface::manifest::Manifest) -> bool {
    // Rule 1: explicit exclusion always wins.
    if manifest.exclude.iter().any(|e| e == key) {
        return false;
    }

    // Rule 2/3: check include / add.
    let in_add = manifest.add.iter().any(|a| a == key);
    if !manifest.include.is_empty() {
        manifest.include.iter().any(|i| i == key) || in_add
    } else {
        // Empty include means "all baseline commands" plus anything in add.
        let is_baseline = REGISTRY
            .iter()
            .any(|(k, baseline)| *k == key && *baseline);
        is_baseline || in_add
    }
}

/// Derive the top-level command name from a registry key.
///
/// `"task add"` -> `"task"`, `"status"` -> `"status"`.
#[allow(dead_code)]
fn top_cmd(key: &str) -> &str {
    key.split_whitespace().next().unwrap_or(key)
}

/// Build a manifest-filtered `clap::Command`.
///
/// The returned `clap::Command` contains only the subcommands (and sub-
/// subcommands) that the manifest permits. Argument definitions are
/// intentionally minimal: they are sufficient for `--help` inspection and
/// for presence/absence tests; full argument schemas remain in the derive-
/// based `Cli` / `Command` enums used by `main.rs`.
///
/// # Example
///
/// ```rust,ignore
/// let manifest = surface::manifest::load(Path::new("surface/baseline-v25.toml"))?;
/// let app = cli::build_app(&manifest);
/// // app.find_subcommand("task") is Some(_) for the baseline manifest.
/// ```
pub fn build_app(manifest: &crate::surface::manifest::Manifest) -> clap::Command {
    // Collect permitted keys for quick lookup.
    let permitted: Vec<&str> = REGISTRY
        .iter()
        .filter(|(key, _)| key_permitted(key, manifest))
        .map(|(key, _)| *key)
        .collect();

    // Group sub-sub-commands by their parent.
    // e.g. "task add", "task list" -> parent "task", children ["add", "list"].
    use std::collections::BTreeMap;
    let mut parents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for key in &permitted {
        let mut parts = key.splitn(2, ' ');
        let parent = parts.next().unwrap();
        match parts.next() {
            Some(child) => parents.entry(parent).or_default().push(child),
            None => {
                // Top-level (no parent): insert with empty child list if absent.
                parents.entry(parent).or_default();
            }
        }
    }

    let mut app = clap::Command::new("synthesist")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Specification graph manager for AI-augmented projects")
        .after_help(
            "All output is JSON. Run 'synthesist skill' for the full behavioral contract.",
        )
        .arg(
            Arg::new("session")
                .long("session")
                .env("SYNTHESIST_SESSION")
                .global(true)
                .value_name("SESSION")
                .help("Session ID for write operations"),
        )
        .arg(
            Arg::new("data_dir")
                .long("data-dir")
                .global(true)
                .value_name("PATH")
                .help("Path to synthesist data directory"),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .global(true)
                .action(clap::ArgAction::SetTrue)
                .help("Skip phase enforcement and transition validation"),
        );

    for (parent, children) in &parents {
        let mut sub = clap::Command::new(*parent);
        for child in children {
            sub = sub.subcommand(clap::Command::new(*child));
        }
        app = app.subcommand(sub);
    }

    app
}

// ---------------------------------------------------------------------------
// Custom value parser for `spec update --status`
// ---------------------------------------------------------------------------

/// Custom value parser for `spec update --status`.
///
/// Optimized for LLM ergonomics: when an agent passes a value that
/// is conceptually a spec disposition (`completed`, `abandoned`,
/// `deferred`) but isn't in the Spec status enum, the rejection
/// message names the right surface (`synthesist outcome add`)
/// inline so the agent can recover without round-tripping through
/// docs. Other invalid values get the standard expected-set message.
///
/// Strict-on-write: synthesist's API boundary rejects everything
/// not in `crate::schema::spec::STATUSES`. The single source of
/// truth is the const referenced here and by the validator.
fn parse_spec_status(s: &str) -> Result<String, String> {
    if crate::schema::spec::STATUSES.contains(&s) {
        return Ok(s.to_string());
    }
    let msg = match s {
        "completed" | "abandoned" | "deferred" => format!(
            "`{s}` is an Outcome value, not a Spec status. To record this disposition, run \
             `synthesist outcome add <tree>/<spec> --status {s} [--note \"...\"]`. Spec status \
             accepts: {}",
            crate::schema::spec::STATUSES.join(", ")
        ),
        "superseded_by" => format!(
            "`superseded_by` is an Outcome status (use `synthesist outcome add ... --status \
             superseded_by --linked-spec <tree>/<spec>`). Spec status accepts: {}",
            crate::schema::spec::STATUSES.join(", ")
        ),
        _ => format!(
            "`{s}` is not a valid Spec status. Accepts: {}",
            crate::schema::spec::STATUSES.join(", ")
        ),
    };
    Err(msg)
}

// ---------------------------------------------------------------------------
// Tests for build_app
// ---------------------------------------------------------------------------

#[cfg(test)]
mod build_app_tests {
    use super::*;

    /// Helper: collect every subcommand name (and sub-subcommand name as
    /// "parent sub") from the built `clap::Command`.
    fn collect_keys(app: &clap::Command) -> Vec<String> {
        let mut keys = Vec::new();
        for sub in app.get_subcommands() {
            let parent = sub.get_name();
            let has_children = sub.get_subcommands().next().is_some();
            if has_children {
                for child in sub.get_subcommands() {
                    keys.push(format!("{} {}", parent, child.get_name()));
                }
            } else {
                keys.push(parent.to_string());
            }
        }
        keys
    }

    /// Baseline manifest: include list with all v2.5 commands, no exclude, no add.
    fn baseline_manifest() -> crate::surface::manifest::Manifest {
        // Parse the actual baseline-v25.toml content inline so tests do not
        // depend on the file path being present at test time.
        let toml = r#"
[manifest]
name        = "baseline-v25"
description = "v2.5-identical surface; all commands that shipped in synthesist 2.5.x"

[commands]
include = [
    "status",
    "tree add", "tree list", "tree show", "tree close",
    "spec add", "spec show", "spec update", "spec list",
    "task add", "task list", "task show", "task update",
    "task claim", "task done", "task reset", "task block",
    "task wait", "task cancel", "task ready", "task acceptance",
    "discovery add", "discovery list",
    "campaign add", "campaign list",
    "session start", "session close", "session list", "session status",
    "phase show", "phase set",
    "export", "import",
    "conflicts", "migrate list", "migrate status", "migrate run", "migrate v2-to-v3",
    "init", "check", "version", "skill",
    "outcome add", "outcome list",
]
exclude = []
add     = []
"#;
        crate::surface::manifest::parse_str(toml, "<test:baseline>").unwrap()
    }

    #[test]
    fn baseline_manifest_has_all_v25_commands() {
        let manifest = baseline_manifest();
        let app = build_app(&manifest);
        let keys = collect_keys(&app);

        // Spot-check a representative cross-section.
        let expected = [
            "status",
            "init",
            "check",
            "conflicts",
            "export",
            "import",
            "skill",
            "version",
            "tree add",
            "tree list",
            "tree show",
            "tree close",
            "spec add",
            "spec show",
            "spec update",
            "spec list",
            "task add",
            "task list",
            "task show",
            "task update",
            "task claim",
            "task done",
            "task reset",
            "task block",
            "task wait",
            "task cancel",
            "task ready",
            "task acceptance",
            "discovery add",
            "discovery list",
            "campaign add",
            "campaign list",
            "session start",
            "session close",
            "session list",
            "session status",
            "phase show",
            "phase set",
            "migrate list",
            "migrate status",
            "migrate run",
            "migrate v2-to-v3",
            "outcome add",
            "outcome list",
        ];

        for cmd in &expected {
            assert!(
                keys.iter().any(|k| k == *cmd),
                "baseline manifest: expected command '{cmd}' but it was not present; got: {keys:?}"
            );
        }
    }

    #[test]
    fn baseline_manifest_excludes_non_baseline_commands() {
        // Commands like "overlay list", "overlay run", "jig run" are NOT
        // in the v2.5 baseline and must not appear when add is empty.
        let manifest = baseline_manifest();
        let app = build_app(&manifest);
        let keys = collect_keys(&app);

        let non_baseline = ["overlay list", "overlay run", "jig run"];
        for cmd in &non_baseline {
            assert!(
                !keys.iter().any(|k| k == *cmd),
                "baseline manifest should not expose '{cmd}' but it was present"
            );
        }
    }

    #[test]
    fn pruned_manifest_omits_excluded_commands() {
        // A pruned manifest that removes task wait, task block, and task cancel.
        let toml = r#"
[manifest]
name        = "pruned"
description = "minimal surface for constrained agents"

[commands]
include = [
    "status", "init", "check",
    "task add", "task list", "task show", "task done", "task ready",
    "spec add", "spec show",
    "session start", "session close",
    "phase show", "phase set",
    "skill",
]
exclude = ["task wait", "task block", "task cancel"]
add     = []
"#;
        let manifest = crate::surface::manifest::parse_str(toml, "<test:pruned>").unwrap();
        let app = build_app(&manifest);
        let keys = collect_keys(&app);

        // These must be absent.
        let excluded = ["task wait", "task block", "task cancel"];
        for cmd in &excluded {
            assert!(
                !keys.iter().any(|k| k == *cmd),
                "pruned manifest should not expose '{cmd}' but it was present"
            );
        }

        // Basic commands must still be present.
        let required = ["status", "task add", "task ready", "task done"];
        for cmd in &required {
            assert!(
                keys.iter().any(|k| k == *cmd),
                "pruned manifest must expose '{cmd}' but it was absent"
            );
        }
    }

    #[test]
    fn add_list_enables_non_baseline_commands() {
        // A manifest with an empty include (all baseline present) plus
        // a non-baseline add list.
        let toml = r#"
[manifest]
name        = "overlay-exposed"
description = "baseline plus overlay surface"

[commands]
include = []
exclude = []
add     = ["overlay run", "overlay list"]
"#;
        let manifest = crate::surface::manifest::parse_str(toml, "<test:overlay>").unwrap();
        let app = build_app(&manifest);
        let keys = collect_keys(&app);

        // Added commands must be present.
        for cmd in &["overlay run", "overlay list"] {
            assert!(
                keys.iter().any(|k| k == *cmd),
                "overlay manifest must expose '{cmd}' but it was absent"
            );
        }

        // Baseline commands must also be present (include is empty = all baseline).
        for cmd in &["status", "task add", "task ready"] {
            assert!(
                keys.iter().any(|k| k == *cmd),
                "baseline command '{cmd}' should be present with empty include, but was absent"
            );
        }
    }

    #[test]
    fn exclude_always_wins_over_include_and_add() {
        let toml = r#"
[manifest]
name        = "conflict-test"
description = "exclude beats include"

[commands]
include = ["status", "overlay run", "task add"]
exclude = ["overlay run", "task add"]
add     = ["overlay run"]
"#;
        let manifest = crate::surface::manifest::parse_str(toml, "<test:conflict>").unwrap();
        let app = build_app(&manifest);
        let keys = collect_keys(&app);

        // "overlay run" is in include, add, AND exclude -- exclude wins.
        assert!(
            !keys.iter().any(|k| k == "overlay run"),
            "'overlay run' is excluded and must not appear even though it is also in include and add"
        );
        // "task add" is excluded.
        assert!(
            !keys.iter().any(|k| k == "task add"),
            "'task add' is excluded and must not appear"
        );
        // "status" is only in include, not excluded.
        assert!(
            keys.iter().any(|k| k == "status"),
            "'status' is in include and not excluded, must be present"
        );
    }

    #[test]
    fn empty_manifest_exposes_only_baseline() {
        // A manifest with all lists empty means: all v2.5 baseline commands,
        // nothing extra.
        let toml = r#"
[manifest]
name        = "empty-lists"
description = "all lists empty"
"#;
        let manifest = crate::surface::manifest::parse_str(toml, "<test:empty>").unwrap();
        let app = build_app(&manifest);
        let keys = collect_keys(&app);

        // Baseline commands present.
        assert!(keys.iter().any(|k| k == "status"));
        assert!(keys.iter().any(|k| k == "task add"));

        // Non-baseline absent.
        assert!(!keys.iter().any(|k| k == "overlay list"));
        assert!(!keys.iter().any(|k| k == "overlay run"));
    }
}

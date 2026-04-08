//! CLI type definitions (clap derive).
//!
//! Every argument and option has a help string. These descriptions are the
//! LLM's first contact with the tool when it runs `--help`.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "synthesist",
    about = "Specification graph manager for AI-augmented projects",
    after_help = "All output is JSON. Reads use main.db; writes use the session .db if one exists.\nRun 'synthesist skill' for the full behavioral contract and worked examples."
)]
pub struct Cli {
    /// Session ID for write operations. Writes go to the session's .db file.
    /// Reads always use main.db regardless of this flag.
    #[arg(long, env = "SYNTHESIST_SESSION", global = true)]
    pub session: Option<String>,

    /// Skip phase enforcement and phase transition validation.
    #[arg(long, global = true)]
    pub force: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize synthesist in the current directory. Creates synthesist/main.db.
    Init,
    /// Estate overview: trees, task counts, ready tasks, sessions, phase.
    Status,
    /// Validate referential integrity across all tables.
    Check,

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
    /// Manage stakeholders (people relevant to the work).
    Stakeholder {
        #[command(subcommand)]
        cmd: StakeholderCmd,
    },
    /// Manage dispositions (assessed stakeholder stances on topics).
    Disposition {
        #[command(subcommand)]
        cmd: DispositionCmd,
    },
    /// Record signals (observable evidence from stakeholders).
    Signal {
        #[command(subcommand)]
        cmd: SignalCmd,
    },
    /// Query a stakeholder's current dispositions. Reads session .db if active, else main.db.
    Stance {
        /// Stakeholder ID (e.g. "mwilson").
        stakeholder: String,
        /// Filter to dispositions matching this topic substring.
        topic: Option<String>,
    },
    /// Manage campaigns (cross-tree spec coordination).
    Campaign {
        #[command(subcommand)]
        cmd: CampaignCmd,
    },
    /// Manage sessions (isolated database copies for concurrent work).
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },
    /// Manage the 7-phase workflow state machine.
    Phase {
        #[command(subcommand)]
        cmd: PhaseCmd,
    },
    /// Export all tables as JSON (for backup or migration).
    Export,
    /// Import tables from JSON (stdin if no file given).
    Import {
        /// Path to JSON file. Reads stdin if omitted.
        file: Option<String>,
    },
    /// Emit the full skill file (behavioral contract + command reference).
    Skill,
    /// Show version and check for updates from GitLab releases.
    Version {
        /// Skip the network check for latest version.
        #[arg(long)]
        offline: bool,
    },
    /// Run a read-only SQL query against main.db (SELECT, EXPLAIN, PRAGMA, WITH).
    Sql {
        /// SQL query to execute. Only read-only queries are allowed.
        query: String,
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
        /// Tree status (default: "active").
        #[arg(long, default_value = "active")]
        status: String,
    },
    /// List all trees in the estate.
    List,
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
        #[arg(long)]
        goal: Option<String>,
        #[arg(long)]
        constraints: Option<String>,
        #[arg(long)]
        decisions: Option<String>,
        /// Spec status: active, completed, abandoned, superseded, deferred.
        #[arg(long)]
        status: Option<String>,
        /// What happened (set when completing or archiving).
        #[arg(long)]
        outcome: Option<String>,
    },
    /// List all specs in a tree.
    List {
        /// Tree name.
        tree: String,
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
    /// Update task summary, description, or files.
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

// --- Stakeholder ---

#[derive(Subcommand)]
pub enum StakeholderCmd {
    /// Add a stakeholder. Scoped to a tree (not tree/spec).
    Add {
        /// Tree name (e.g. "upstream"). Note: tree only, not tree/spec.
        tree: String,
        /// Stakeholder ID (e.g. "mwilson"). Short, unique within tree.
        id: String,
        /// Role and relevance (e.g. "lead maintainer, auth team").
        #[arg(long)]
        context: String,
        /// Display name (e.g. "M. Wilson").
        #[arg(long)]
        name: Option<String>,
        /// Comma-separated organizations.
        #[arg(long, value_delimiter = ',')]
        orgs: Vec<String>,
    },
    /// List all stakeholders in a tree.
    List {
        /// Tree name.
        tree: String,
    },
}

// --- Disposition ---

#[derive(Subcommand)]
pub enum DispositionCmd {
    /// Add a disposition (assessed stance on a topic).
    Add {
        /// Path in tree/spec format.
        tree_spec: String,
        /// Stakeholder ID (must exist in the tree).
        stakeholder: String,
        /// Technical topic (e.g. "API versioning", "internal vs external tooling").
        #[arg(long)]
        topic: String,
        /// Stance: supportive, cautious, opposed, neutral, unknown.
        #[arg(long)]
        stance: String,
        /// Confidence: documented, verified, inferred, speculative.
        #[arg(long)]
        confidence: String,
        /// What approach they prefer (e.g. "incremental migration over breaking rewrite").
        #[arg(long)]
        preferred: Option<String>,
        /// Additional detail or nuance.
        #[arg(long)]
        detail: Option<String>,
    },
    /// List all dispositions for a spec (current and superseded).
    List {
        /// Path in tree/spec format.
        tree_spec: String,
    },
    /// Supersede an existing disposition with updated stance.
    Supersede {
        /// Path in tree/spec format.
        tree_spec: String,
        /// ID of the disposition to supersede (e.g. "disp1").
        old_id: String,
        /// New stance: supportive, cautious, opposed, neutral, unknown.
        #[arg(long)]
        stance: String,
        /// New confidence: documented, verified, inferred, speculative.
        #[arg(long)]
        confidence: String,
        /// Updated preferred approach.
        #[arg(long)]
        preferred: Option<String>,
        /// Updated detail.
        #[arg(long)]
        detail: Option<String>,
    },
}

// --- Signal ---

#[derive(Subcommand)]
pub enum SignalCmd {
    /// Record a signal (observable evidence from a stakeholder).
    Add {
        /// Path in tree/spec format.
        tree_spec: String,
        /// Stakeholder ID who produced the signal.
        stakeholder: String,
        /// Where the signal was observed (e.g. URL, document name).
        #[arg(long)]
        source: String,
        /// Type: pr_comment, issue_comment, review, commit_message, chat, meeting, email, other.
        #[arg(long)]
        source_type: String,
        /// What the stakeholder said or did.
        #[arg(long)]
        content: String,
        /// What this signal means for our work.
        #[arg(long)]
        interpretation: Option<String>,
        /// What we did that prompted this signal.
        #[arg(long)]
        our_action: Option<String>,
        /// When the signal occurred (YYYY-MM-DD, default: today).
        #[arg(long)]
        date: Option<String>,
    },
    /// List all signals for a spec.
    List {
        /// Path in tree/spec format.
        tree_spec: String,
    },
}

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
        /// Current phase of this spec in the campaign.
        #[arg(long)]
        phase: Option<String>,
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
    /// Start a session. Creates an isolated copy of main.db.
    /// Writes with --session=<id> go to this copy. Reads always use main.db.
    /// Merge the session to make changes visible.
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
    /// Merge session changes into main.db using three-way diff.
    /// Reports per-table adds/mods/deletes/conflicts.
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
    /// Discard a session. Deletes the session .db file. Changes are lost.
    Discard {
        /// Session ID.
        id: String,
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
    /// Show the current workflow phase.
    Show,
}

//! Skill file emission for LLM agents.
//!
//! The skill file is the primary interface between synthesist and LLM agents.
//! It must be execution-system agnostic (works with Claude Code, Cursor, etc.).
//!
//! # Manifest-filtered emission
//!
//! `cmd_skill(Some(path))` loads the manifest at `path`, builds the
//! manifest-filtered command tree via `cli::build_app`, and emits a skill
//! document that lists only the commands present in that tree.
//!
//! `cmd_skill(None)` (default) prints `SKILL_CONTENT` unchanged.  The output
//! is byte-equivalent to the v3 baseline.

use std::path::Path;

use anyhow::Result;

pub fn cmd_skill(manifest_path: Option<&Path>) -> Result<()> {
    match manifest_path {
        None => {
            // Default: baseline behavior, byte-equivalent to the v3
            // baseline. Do not filter or transform.
            print!("{SKILL_CONTENT}");
        }
        Some(path) => {
            let manifest = crate::surface::manifest::load(path)?;
            let skill = generate_skill_for_manifest(&manifest);
            print!("{skill}");
        }
    }
    Ok(())
}

/// Generate a skill document filtered to the commands permitted by `manifest`.
///
/// The structure follows `SKILL_CONTENT` exactly. Sections in the Command
/// Reference are included only when at least one command from that section
/// appears in the manifest's permitted set. The "Behavioral Contract",
/// "Worked Example", "Data Model", "Display Conventions", "Error Handling",
/// "Storage", and "Schema" sections are always included because they document
/// invariants that apply regardless of surface.
///
/// Additional command groups available in the manifest but absent from the
/// baseline skill content (e.g. `overlay run`) are appended to the
/// Command Reference under their own headings.
fn generate_skill_for_manifest(manifest: &crate::surface::manifest::Manifest) -> String {
    // Build the set of permitted command keys for this manifest.
    let app = crate::cli::build_app(manifest);
    let permitted = collect_app_keys(&app);

    // Start from the baseline content and selectively rebuild the Command
    // Reference section, then append any non-baseline sections.
    //
    // Strategy: split SKILL_CONTENT at "## Command Reference" and re-emit
    // only the permitted groups, then re-append the trailing non-command
    // sections ("## Display Conventions" onward).

    let cmd_ref_marker = "## Command Reference\n";
    let after_cmd_ref_marker = "## Display Conventions\n";

    let before_cmd_ref = match SKILL_CONTENT.find(cmd_ref_marker) {
        Some(idx) => &SKILL_CONTENT[..idx],
        None => return SKILL_CONTENT.to_string(),
    };

    let after_sections = match SKILL_CONTENT.find(after_cmd_ref_marker) {
        Some(idx) => &SKILL_CONTENT[idx..],
        None => "",
    };

    let mut out = String::with_capacity(SKILL_CONTENT.len() + 1024);
    out.push_str(before_cmd_ref);
    out.push_str(cmd_ref_marker);
    out.push('\n');

    // Emit each baseline command group only when it has permitted commands.
    emit_section_if_permitted(
        &mut out,
        &permitted,
        &["status", "init", "check", "conflicts", "version", "skill", "export", "import"],
        ESTATE_SECTION,
    );
    emit_section_if_permitted(
        &mut out,
        &permitted,
        &["tree add", "tree list", "tree show", "tree close"],
        TREES_SECTION,
    );
    emit_section_if_permitted(
        &mut out,
        &permitted,
        &["spec add", "spec show", "spec update", "spec list"],
        SPECS_SECTION,
    );
    emit_section_if_permitted(
        &mut out,
        &permitted,
        &[
            "task add", "task list", "task show", "task update", "task claim",
            "task done", "task reset", "task block", "task wait", "task cancel",
            "task ready", "task acceptance",
        ],
        TASKS_SECTION,
    );
    emit_section_if_permitted(
        &mut out,
        &permitted,
        &["discovery add", "discovery list"],
        DISCOVERIES_SECTION,
    );
    emit_section_if_permitted(
        &mut out,
        &permitted,
        &["outcome add", "outcome list"],
        OUTCOMES_SECTION,
    );
    emit_section_if_permitted(
        &mut out,
        &permitted,
        &["campaign add", "campaign list"],
        CAMPAIGNS_SECTION,
    );
    emit_section_if_permitted(
        &mut out,
        &permitted,
        &["session start", "session close", "session list", "session status"],
        SESSIONS_SECTION,
    );
    emit_section_if_permitted(
        &mut out,
        &permitted,
        &["phase show", "phase set"],
        PHASE_SECTION,
    );
    emit_section_if_permitted(
        &mut out,
        &permitted,
        &["export", "import", "migrate status", "migrate v2-to-v3"],
        DATA_MANAGEMENT_SECTION,
    );

    // Non-baseline additions: emit only when the manifest permits them.
    if permitted.iter().any(|k| k == "overlay list" || k == "overlay run") {
        out.push_str(OVERLAY_SECTION);
        out.push('\n');
    }
    if permitted.iter().any(|k| k.starts_with("jig")) {
        out.push_str(JIG_SECTION);
        out.push('\n');
    }

    out.push_str(after_sections);
    out
}

/// Emit `section_content` only when at least one of `required_keys` is in
/// `permitted`. The heading is included in `section_content`.
fn emit_section_if_permitted(
    out: &mut String,
    permitted: &[String],
    required_keys: &[&str],
    section_content: &str,
) {
    if required_keys
        .iter()
        .any(|k| permitted.iter().any(|p| p == k))
    {
        out.push_str(section_content);
        out.push('\n');
    }
}

/// Collect every command key from a built `clap::Command` tree in "parent
/// sub" form (e.g. `"task add"`, `"status"`).
fn collect_app_keys(app: &clap::Command) -> Vec<String> {
    let mut keys = Vec::new();
    for sub in app.get_subcommands() {
        let parent = sub.get_name();
        let mut has_children = false;
        for child in sub.get_subcommands() {
            keys.push(format!("{parent} {}", child.get_name()));
            has_children = true;
        }
        if !has_children {
            keys.push(parent.to_string());
        }
    }
    keys
}

// ---------------------------------------------------------------------------
// Per-section content fragments used in manifest-filtered emission.
// Each fragment begins with its ### heading and ends before the next heading.
// ---------------------------------------------------------------------------

const ESTATE_SECTION: &str = "### Estate
```
synthesist init                                  # creates the claims/ directory
synthesist status                                # trees, task counts, ready tasks, sessions
synthesist check                                 # referential integrity validation
synthesist conflicts                             # list diamond conflicts (same prior superseded by >1 live successor)
synthesist version                               # version + update check
synthesist skill                                 # this file
```";

const TREES_SECTION: &str = r#"### Trees
```
synthesist tree add <name> --description TEXT     # e.g. tree add upstream --description "GitLab"
synthesist tree list                              # hides closed trees
synthesist tree list --include-closed             # include trees superseded with status=closed
synthesist tree show <name>                       # name, description, spec_count, session_count
synthesist tree close <name>                      # supersede with status=closed (non-destructive)
synthesist tree close <name> --start-id <hash>    # disambiguate when multiple trees share <name>
```"#;

const SPECS_SECTION: &str = r#"### Specs
```
synthesist spec add <tree/spec> --goal TEXT        # e.g. spec add upstream/auth --goal "Migrate v2->v3"
synthesist spec show <tree/spec>
synthesist spec update <tree/spec> --status done   # work delivered; spec moves to terminal state
synthesist spec list <tree>                       # positional form, e.g. spec list upstream
synthesist spec list --tree <name>                # flag form (same effect)
```
Status values: `draft`, `active`, `done`, `superseded`. To record
how a spec was disposed of (`completed`, `abandoned`, `deferred`,
`superseded_by`), use `synthesist outcome add` -- those are
Outcome claim values, not Spec status values, and the CLI rejects
them at parse time with a redirect message."#;

const TASKS_SECTION: &str = r#"### Tasks
IDs auto-generate as t1, t2, ... unless --id is provided.
```
synthesist task add <tree/spec> "summary" --depends-on t1,t2 --gate human --files src/auth.rs
synthesist task list <tree/spec> --active          # hide cancelled tasks
synthesist task show <tree/spec> <id>              # full detail with deps, files, criteria
synthesist task update <tree/spec> <id> --summary "revised summary"
synthesist task update <tree/spec> <id> --depends-on t4,t5   # replace dep list (validates cycle/self/unknown)
synthesist task update <tree/spec> <id> --depends-on ""      # clear dep list
synthesist task claim <tree/spec> <id>             # pending -> in_progress (sets owner)
synthesist task done <tree/spec> <id>              # in_progress -> done (runs acceptance criteria)
synthesist task reset <tree/spec> <id>             # in_progress -> pending (crash recovery)
synthesist task reset --session <dead-session>     # bulk reset all tasks owned by dead session
synthesist task block <tree/spec> <id>             # pending/in_progress -> blocked
synthesist task wait <tree/spec> <id> --reason "waiting on MR !123"
synthesist task cancel <tree/spec> <id> --reason "approach changed"
synthesist task ready <tree/spec>                  # pending tasks with all deps done
synthesist task acceptance <tree/spec> <id> --criterion "tests pass" --verify "cargo test"
```"#;

const DISCOVERIES_SECTION: &str = r#"### Discoveries
```
synthesist discovery add <tree/spec> --finding "SQLite outperforms DuckDB for this workload" --impact high
synthesist discovery list <tree/spec>
```"#;

const OUTCOMES_SECTION: &str = r#"### Outcomes
Outcome claims express *what happened* to a spec (distinct from
Spec status, which expresses *what state the spec is in*). Each
Outcome is its own claim with its own asserter and timestamp;
multiple Outcomes against the same spec form a history.
```
synthesist outcome add <tree/spec> --status completed --note "shipped in MR !500"
synthesist outcome add <tree/spec> --status abandoned --note "scope folded into auth-v3"
synthesist outcome add <tree/spec> --status deferred --note "blocked by upstream"
synthesist outcome add <tree/spec> --status superseded_by --linked-spec other/spec --note "absorbed"
synthesist outcome list <tree/spec>
```
Status values: `completed`, `abandoned`, `deferred`,
`superseded_by`. The `superseded_by` status requires
`--linked-spec` (the schema rejects it without)."#;

const CAMPAIGNS_SECTION: &str = r#"### Campaigns
```
synthesist campaign add <tree> <spec-id> --summary "Auth migration"
synthesist campaign add <tree> <spec-id> --backlog --title "Future: OAuth2 support"
synthesist campaign list <tree>
```"#;

const SESSIONS_SECTION: &str = r#"### Sessions
```
synthesist session start <id> --tree upstream --spec auth --summary "Auth work"
synthesist session close <id>                     # append a closing supersession
synthesist session close <id> --start-id <hash>   # disambiguate when multiple openers share <id>
synthesist session list                           # show all sessions
synthesist session status <id>                    # claims written in this session
```

Multi-user writes merge automatically via CRDT. Run
`synthesist conflicts` to surface unresolved supersessions."#;

const PHASE_SECTION: &str = r#"### Phase
Phase is per-session in v3. Both `phase show` and `phase set`
require `--session=<id>` or `SYNTHESIST_SESSION`. To see every
live session's phase at once, use `synthesist status`.
```
synthesist phase show --session=<id>               # current phase for one session
synthesist phase show                              # ERROR if SYNTHESIST_SESSION unset
synthesist phase set plan --session=<id>           # orient -> plan (validated)
synthesist phase set execute --session=<id>        # fails if not in agree
synthesist --force phase set execute --session=<id> # override transition validation
```"#;

const DATA_MANAGEMENT_SECTION: &str = r#"### Data Management
```
synthesist export                                                                  # full JSON backup (claim log export)
synthesist import backup.json                                                      # restore from backup
synthesist migrate status                                                          # report schema version + pending migrations
synthesist migrate v2-to-v3 --dry-run                                              # plan the v2-to-v3 migration without writing
```

Migration is an integrated subcommand. Pass `--dry-run` first to
plan the chain, then re-run without it. See synthesist/MIGRATION.md
for the operator runbook.

Observation commands (`stakeholder`, `disposition`, `signal`,
`topic`, `stance`, `landscape`) have moved to the `lattice` tool.
Running them here prints a pointer to the replacement."#;

// Non-baseline sections: included only when the manifest permits them.

const OVERLAY_SECTION: &str = "### Overlays
Named analysis passes over the gamma index. Each overlay runs a
typed query and returns structured hits. Read-only.
```
synthesist overlay list                            # list registered overlays
synthesist overlay run <name>                      # run overlay, print hits as JSON
```";

const JIG_SECTION: &str = "### Jig
Run canonical scenarios under surface manifests and record results.
Results land in `claims/_jig/<run_id>.json`.
```
synthesist jig run --scenario <name> --manifest <name>
synthesist jig list-scenarios                      # list scenarios in jig/scenarios/
synthesist jig list-manifests                      # list manifests in surface/
```";

const SKILL_CONTENT: &str = r#"# Synthesist -- Specification Graph Manager (v3)

## Data Model

Estate > Trees > Specs > Tasks.

- **Tree**: a project domain (e.g. "upstream", "lever", "sysml")
- **Spec**: a unit of work within a tree (e.g. "auth-migration", "gkg-bench")
- **Task**: an atomic work item in a dependency DAG within a spec
- **Discovery**: a timestamped finding recorded during work
- **Campaign**: an umbrella grouping of specs pursuing a shared outcome
- **Session**: a tag applied to writes, identifying who and when
- **Phase**: the current point in the workflow state machine (per session)

Path format: `tree/spec` (e.g. "upstream/auth", "lever/ci-containers").

Every entity is a **claim** in an append-only, content-addressed log
stored in `claims/`. Updates are recorded as *supersessions* -- the
previous claim is not overwritten, the full history is preserved.
Multi-user writes merge automatically via CRDT.

Observation-layer data -- stakeholders, dispositions, signals,
topics -- has moved to the `lattice` tool. It is no longer part of
synthesist.

## Worked Example: Full Session Lifecycle

```bash
# 1. Initialize and start a session
synthesist init                                      # creates the claims/ directory
synthesist session start research --tree upstream --spec auth \
  --summary "Auth migration research"

# 2. Set phase and create the work plan
export SYNTHESIST_SESSION=research
synthesist phase set plan                            # orient -> plan for this session
synthesist tree add upstream --description "GitLab upstream project"
synthesist spec add upstream/auth --goal "Migrate auth API from v2 to v3"
synthesist task add upstream/auth "Research API versioning strategy"
synthesist task add upstream/auth "Implement token refresh migration" --depends-on t1
synthesist task add upstream/auth "Write integration tests" --depends-on t2 --gate human

# 3. Pin the agree snapshot, then present plan to human (AGREE phase)
synthesist spec update upstream/auth --agree-snapshot <claim-iris>
synthesist phase set agree
# Present: 3 tasks, t3 has human gate.
# Human approves. Human says: "proceed"

# 4. Execute
synthesist phase set execute
synthesist task claim upstream/auth t1
# ... do the research work ...
synthesist task done upstream/auth t1

# 5. Reflect -- check what's ready
synthesist phase set reflect
synthesist task ready upstream/auth
# Returns: [{"id": "t2", "summary": "Implement token refresh migration"}]
synthesist discovery add upstream/auth \
  --finding "v3 API supports token refresh natively, no custom implementation needed" \
  --impact "high -- simplifies t2 significantly"

# 6. Continue executing
synthesist phase set execute
synthesist task claim upstream/auth t2
synthesist task done upstream/auth t2
# t3 has gate=human, so present to human before claiming

# 7. Report and close
synthesist phase set report
synthesist discovery add upstream/auth \
  --finding "Migration completed using incremental approach"
synthesist session close research
```

## Behavioral Contract

### Workflow State Machine

7-phase cycle. Phase enforcement is algorithmic -- the CLI rejects
operations that violate the current phase. Transitions are validated.

| Phase | Allowed | Transitions to |
|-------|---------|---------------|
| ORIENT | Read status, read discoveries. No writes. | plan |
| PLAN | Add tasks/specs, add dependencies. Pin the agree snapshot with `spec update --agree-snapshot <claim-iris>` before leaving PLAN. No task claims. | agree |
| AGREE | Present plan. No writes. Block until human approves. | execute |
| EXECUTE | Claim tasks, complete tasks. No task creation/cancellation. | reflect, report |
| REFLECT | Assess plan validity, record discoveries. No claims. | execute, replan, report |
| REPLAN | Modify task tree, add/remove tasks. | agree |
| REPORT | Summarize outcomes, record discoveries. Session close. | (end) |

Use `--force` to override phase enforcement when necessary.
Every fresh session starts in ORIENT and must transition to PLAN
before its first write. The orient -> plan transition is valid by
default, so `synthesist phase set plan --session=<id>` (or with
`SYNTHESIST_SESSION` set) works without `--force`.

**Phase is per-session.** Each Phase claim is scoped to the
session id of the writing session. There is no estate-wide phase;
concurrent sessions can sit in different phases without
interfering. `synthesist phase show` and `synthesist phase set`
both require `--session=<id>` (or `SYNTHESIST_SESSION`); a missing
session fails with a discoverable error pointing at the
resolution.

To inspect every live session's phase at once, use
`synthesist status` (each entry in `sessions[]` carries its
current phase).

### Session Protocol

All write operations require a session. Writes are tagged with the
session identifier so the origin of every claim is recoverable.

```bash
synthesist session start my-session           # appends a Session claim
export SYNTHESIST_SESSION=my-session          # or use --session=my-session
synthesist --force task add tree/spec "task"  # writes tagged with the session
synthesist task list tree/spec                # reads current view
synthesist session close my-session           # appends a closing supersession
```

Sessions are *not* separate database files. There is no
`session merge` and no `session discard`.

Multi-user writes merge automatically via CRDT. Run
`synthesist conflicts` to surface unresolved supersessions when
concurrent writers have disagreed; resolve them by appending a new
claim that supersedes the contested chain.

### AGREE Gate

The AGREE phase is a hard gate. The agent presents:
1. The task tree (what will be done, in dependency order)
2. Assumptions and risks
3. Which tasks need human gates
4. What "done" looks like

The agent halts and waits for explicit human approval.

**Pin the agree snapshot before `phase set agree`.** While still in
PLAN, record the exact claims the human is agreeing to:

```bash
synthesist spec update <tree/spec> --agree-snapshot <claim-iris>
```

This pins the snapshot in PLAN so the AGREE gate is anchored to a
concrete set of claim IRIs rather than whatever the live view happens
to be at approval time. Run it before `synthesist phase set agree`.

The **plan-at-risk** overlay flags any spec whose pinned agree
snapshot was later superseded: if a claim referenced by the snapshot
has since been replaced by a live successor, the spec was agreed
against state that no longer holds, and the plan should be revisited
(REPLAN) and re-agreed. Surface this with the overlay before trusting
a pinned snapshot during EXECUTE.

## Command Reference

### Estate
```
synthesist init                                  # creates the claims/ directory
synthesist status                                # trees, task counts, ready tasks, sessions
synthesist check                                 # referential integrity validation
synthesist conflicts                             # list diamond conflicts (same prior superseded by >1 live successor)
synthesist version                               # version + update check
synthesist skill                                 # this file
```

### Trees
```
synthesist tree add <name> --description TEXT     # e.g. tree add upstream --description "GitLab"
synthesist tree list                              # hides closed trees
synthesist tree list --include-closed             # include trees superseded with status=closed
synthesist tree show <name>                       # name, description, spec_count, session_count
synthesist tree close <name>                      # supersede with status=closed (non-destructive)
synthesist tree close <name> --start-id <hash>    # disambiguate when multiple trees share <name>
```

### Specs
```
synthesist spec add <tree/spec> --goal TEXT        # e.g. spec add upstream/auth --goal "Migrate v2->v3"
synthesist spec show <tree/spec>
synthesist spec update <tree/spec> --status done   # work delivered; spec moves to terminal state
synthesist spec list <tree>                       # positional form, e.g. spec list upstream
synthesist spec list --tree <name>                # flag form (same effect)
```
Status values: `draft`, `active`, `done`, `superseded`. To record
how a spec was disposed of (`completed`, `abandoned`, `deferred`,
`superseded_by`), use `synthesist outcome add` -- those are
Outcome claim values, not Spec status values, and the CLI rejects
them at parse time with a redirect message.

### Tasks
IDs auto-generate as t1, t2, ... unless --id is provided.
```
synthesist task add <tree/spec> "summary" --depends-on t1,t2 --gate human --files src/auth.rs
synthesist task list <tree/spec> --active          # hide cancelled tasks
synthesist task show <tree/spec> <id>              # full detail with deps, files, criteria
synthesist task update <tree/spec> <id> --summary "revised summary"
synthesist task update <tree/spec> <id> --depends-on t4,t5   # replace dep list (validates cycle/self/unknown)
synthesist task update <tree/spec> <id> --depends-on ""      # clear dep list
synthesist task claim <tree/spec> <id>             # pending -> in_progress (sets owner)
synthesist task done <tree/spec> <id>              # in_progress -> done (runs acceptance criteria)
synthesist task reset <tree/spec> <id>             # in_progress -> pending (crash recovery)
synthesist task reset --session <dead-session>     # bulk reset all tasks owned by dead session
synthesist task block <tree/spec> <id>             # pending/in_progress -> blocked
synthesist task wait <tree/spec> <id> --reason "waiting on MR !123"
synthesist task cancel <tree/spec> <id> --reason "approach changed"
synthesist task ready <tree/spec>                  # pending tasks with all deps done
synthesist task acceptance <tree/spec> <id> --criterion "tests pass" --verify "cargo test"
```

### Discoveries
```
synthesist discovery add <tree/spec> --finding "SQLite outperforms DuckDB for this workload" --impact high
synthesist discovery list <tree/spec>
```

### Outcomes
Outcome claims express *what happened* to a spec (distinct from
Spec status, which expresses *what state the spec is in*). Each
Outcome is its own claim with its own asserter and timestamp;
multiple Outcomes against the same spec form a history.
```
synthesist outcome add <tree/spec> --status completed --note "shipped in MR !500"
synthesist outcome add <tree/spec> --status abandoned --note "scope folded into auth-v3"
synthesist outcome add <tree/spec> --status deferred --note "blocked by upstream"
synthesist outcome add <tree/spec> --status superseded_by --linked-spec other/spec --note "absorbed"
synthesist outcome list <tree/spec>
```
Status values: `completed`, `abandoned`, `deferred`,
`superseded_by`. The `superseded_by` status requires
`--linked-spec` (the schema rejects it without).

### Campaigns
```
synthesist campaign add <tree> <spec-id> --summary "Auth migration"
synthesist campaign add <tree> <spec-id> --backlog --title "Future: OAuth2 support"
synthesist campaign list <tree>
```

### Sessions
```
synthesist session start <id> --tree upstream --spec auth --summary "Auth work"
synthesist session close <id>                     # append a closing supersession
synthesist session close <id> --start-id <hash>   # disambiguate when multiple openers share <id>
synthesist session list                           # show all sessions
synthesist session status <id>                    # claims written in this session
```

Multi-user writes merge automatically via CRDT. Run
`synthesist conflicts` to surface unresolved supersessions.

### Phase
Phase is per-session in v3. Both `phase show` and `phase set`
require `--session=<id>` or `SYNTHESIST_SESSION`. To see every
live session's phase at once, use `synthesist status`.
```
synthesist phase show --session=<id>               # current phase for one session
synthesist phase show                              # ERROR if SYNTHESIST_SESSION unset
synthesist phase set plan --session=<id>           # orient -> plan (validated)
synthesist phase set execute --session=<id>        # fails if not in agree
synthesist --force phase set execute --session=<id> # override transition validation
```

### Data Management
```
synthesist export                                                                  # full JSON backup (claim log export)
synthesist import backup.json                                                      # restore from backup
synthesist migrate status                                                          # report schema version + pending migrations
synthesist migrate v2-to-v3 --dry-run                                              # plan the v2-to-v3 migration without writing
```

Migration is an integrated subcommand. Pass `--dry-run` first to
plan the chain, then re-run without it. See synthesist/MIGRATION.md
for the operator runbook.

Observation commands (`stakeholder`, `disposition`, `signal`,
`topic`, `stance`, `landscape`) have moved to the `lattice` tool.
Running them here prints a pointer to the replacement.

## Display Conventions

- All output is JSON. The LLM formats for human display.
- Group tasks by status when presenting to humans.
- Use tables for task lists, not raw JSON.
- Summarize before showing detail.

## Error Handling

Errors return non-zero exit code and a message on stderr:

```
error: task t3 is in_progress, not pending
error: phase violation (plan): cannot claim tasks in PLAN phase
error: dependency t1 is pending, not done
error: task t1 already owned by factory-01
error: invalid phase transition: plan -> execute (valid: agree)
error: invalid session ID '../main': must not contain path separators or '..'
```

Schema errors carry full diagnostic detail: claim type, field
name, actual value, and expected enum set. The `--status
completed` / `abandoned` / `deferred` family on `spec update`
rejects with an inline redirect at parse time:

```
error: invalid value 'completed' for '--status <STATUS>':
       `completed` is an Outcome value, not a Spec status.
       To record this disposition, run
       `synthesist outcome add <tree>/<spec> --status completed [--note "..."]`.
       Spec status accepts: draft, active, done, superseded
```

On error: read the message, diagnose the root cause, fix it.
Do not retry the identical command blindly.

### `synthesist check` errors on existing data after upgrading from v2.3.x

Estates upgrading from v2.3 may surface schema errors for spec
claims with `status: "completed"`, `"abandoned"`, or `"deferred"`.
v2.3's CLI advertised those values in `--help` even though the
schema rejected them; users sometimes wrote them via `--force`.
v2.4.0 surfaces those claims as schema errors during `check`.
The estate is still usable -- errors are advisory.

To clean, record the disposition as an `Outcome` claim and reset
the spec status to a valid terminal value:

```
synthesist outcome add <tree>/<spec> --status completed --note "what shipped"
synthesist --force spec update <tree>/<spec> --status done
```

## Storage

All state lives in `claims/` at the repo root.

- `claims/<asserter>/log.jsonl` -- git-tracked, append-only per-asserter
  log. Each line is one compact JSON-LD claim document. There is one
  log per asserter (e.g. `claims/user-local-agd/log.jsonl`), so
  concurrent writers never contend on the same file.
- `claims/_schema.json` -- git-tracked schema version record.
- `claims/_view.gamma` -- gitignored, disposable redb gamma index file.
  The query engine rebuilds it from the per-asserter logs whenever the
  cache is absent or stale; it carries no source-of-truth state.

The per-asserter logs are the source of truth. The gamma index is a
rebuildable local cache. Never read or write these files directly;
always use synthesist subcommands.

Conflict resolution is via **supersession**: concurrent writers that
disagree produce competing supersession chains, and resolution means
appending a new claim that supersedes the contested chain. See the
`nomograph-claim` documentation for the substrate contract.

## Schema (v3)

Synthesist's claim types are described declaratively in a SHACL Turtle
document shipped alongside the binary:

```
ontology/synthesist.shacl.ttl
```

The SHACL shapes are a **documentation artifact only**. They are not
evaluated at runtime. Synthesist's imperative validators in
`src/schema/*.rs` remain authoritative. External tools and LLM consumers
may use the shapes to understand the predicate vocabulary, cardinality
constraints, and allowed enum values for each claim type.

### Claim types and their SHACL shapes

| Claim type | Shape IRI | Key predicates |
|------------|-----------|---------------|
| Tree | `synthesist:TreeShape` | `synthesist:name` (required), `synthesist:description` |
| Spec | `synthesist:SpecShape` | `synthesist:tree`, `synthesist:id`, `synthesist:goal`, `synthesist:status` |
| Task | `synthesist:TaskShape` | `synthesist:tree`, `synthesist:spec`, `synthesist:id`, `synthesist:summary`, `synthesist:status` |
| Discovery | `synthesist:DiscoveryShape` | `synthesist:tree`, `synthesist:spec`, `synthesist:id`, `synthesist:finding`, `synthesist:date` |
| Session | `synthesist:SessionShape` | `synthesist:id`, `synthesist:summary` |
| Phase | `synthesist:PhaseShape` | `synthesist:session_id`, `synthesist:name` |
| Campaign | `synthesist:CampaignShape` | `synthesist:tree`, `synthesist:spec`, `synthesist:kind` |
| Outcome | `synthesist:OutcomeShape` | `synthesist:tree`, `synthesist:spec`, `synthesist:status` |

Every claim includes the universal PROV-O envelope:
- `prov:generatedAtTime` -- ISO 8601 timestamp with millisecond precision
- `prov:wasAttributedTo` -- asserter IRI identifying who wrote the claim

Prefix declarations used throughout the shapes:
- `synthesist:` = `<https://nomograph.org/synthesist/>`
- `prov:` = `<http://www.w3.org/ns/prov#>`
- `xsd:` = `<http://www.w3.org/2001/XMLSchema#>`
- `nomograph:` = `<https://nomograph.org/v3/>`

Read `ontology/synthesist.shacl.ttl` at the synthesist repo root for the full
shape definitions, including `sh:in` value sets for enumerated fields such
as `synthesist:status` on Task and Spec, and `synthesist:name` on Phase.
"#;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Enumerate every top-level section heading the skill file must contain.
    // These are load-bearing: the LLM behavioral contract depends on them.
    const REQUIRED_SECTIONS: &[&str] = &[
        "## Data Model",
        "## Worked Example: Full Session Lifecycle",
        "## Behavioral Contract",
        "### Workflow State Machine",
        "### Session Protocol",
        "### AGREE Gate",
        "## Command Reference",
        "### Estate",
        "### Trees",
        "### Specs",
        "### Tasks",
        "### Discoveries",
        "### Outcomes",
        "### Campaigns",
        "### Sessions",
        "### Phase",
        "### Data Management",
        "## Display Conventions",
        "## Error Handling",
        "## Storage",
        // v3 T3.4: SHACL schema reference section
        "## Schema (v3)",
    ];

    #[test]
    fn skill_content_contains_all_required_sections() {
        for section in REQUIRED_SECTIONS {
            assert!(
                SKILL_CONTENT.contains(section),
                "skill content missing required section: {section}"
            );
        }
    }

    #[test]
    fn skill_content_references_shacl_artifact() {
        assert!(
            SKILL_CONTENT.contains("ontology/synthesist.shacl.ttl"),
            "skill content must reference the SHACL Turtle artifact path"
        );
    }

    #[test]
    fn skill_content_references_all_claim_type_shapes() {
        // Drive the expected shape IRIs from wire_format::shape_iri so
        // the assertion uses the same builder the SHACL emitter does;
        // any rename or case shift surfaces here and in emit_shacl
        // simultaneously.
        for ty in ["tree", "spec", "task", "discovery", "session", "phase", "campaign", "outcome"] {
            let shape = crate::wire_format::shape_iri(ty);
            assert!(
                SKILL_CONTENT.contains(&shape),
                "skill content missing SHACL shape reference: {shape}"
            );
        }
    }

    #[test]
    fn skill_content_contains_every_cli_command_usage_block() {
        // Each v2.5 command group must appear in the content, identified by
        // its usage pattern. This is the acceptance criterion from T3.4.
        let commands = &[
            "synthesist init",
            "synthesist status",
            "synthesist check",
            "synthesist conflicts",
            "synthesist version",
            "synthesist skill",
            "synthesist tree add",
            "synthesist tree list",
            "synthesist spec add",
            "synthesist spec show",
            "synthesist task add",
            "synthesist task claim",
            "synthesist task done",
            "synthesist task ready",
            "synthesist discovery add",
            "synthesist outcome add",
            "synthesist campaign add",
            "synthesist session start",
            "synthesist session close",
            "synthesist phase show",
            "synthesist phase set",
            "synthesist export",
            "synthesist import",
            "synthesist migrate",
        ];
        for cmd in commands {
            assert!(
                SKILL_CONTENT.contains(cmd),
                "skill content missing CLI command usage: {cmd}"
            );
        }
    }

    #[test]
    fn skill_content_parses_as_text_without_em_dashes() {
        // No em dashes per project conventions.
        assert!(
            !SKILL_CONTENT.contains('\u{2014}'),
            "skill content must not contain em dashes"
        );
    }

    #[test]
    fn cmd_skill_returns_ok() {
        // Smoke test: default (no manifest) must not error.
        cmd_skill(None).expect("cmd_skill(None) should return Ok");
    }

    // T5.3 acceptance tests
    // -----------------------------------------------------------------------

    /// Helper: build an inline manifest and run generate_skill_for_manifest.
    fn skill_for_toml(toml: &str) -> String {
        let manifest = crate::surface::manifest::parse_str(toml, "<test>").unwrap();
        generate_skill_for_manifest(&manifest)
    }

    #[test]
    fn default_skill_is_byte_equivalent_to_skill_content() {
        // The no-manifest path prints SKILL_CONTENT unchanged. Verify that
        // SKILL_CONTENT is non-empty and contains the canonical sections.
        // cmd_skill(None) is exercised by cmd_skill_returns_ok; here we
        // verify the content contract directly.
        assert!(!SKILL_CONTENT.is_empty());
        assert!(SKILL_CONTENT.contains("## Command Reference"));
        assert!(SKILL_CONTENT.contains("## Schema (v3)"));
    }

    #[test]
    fn baseline_manifest_produces_all_v25_sections() {
        // A manifest with an explicit include list covering all v2.5 commands
        // (empty include = all baseline) should produce a skill that contains
        // all the expected section headings.
        let toml = r#"
[manifest]
name        = "baseline-v25"
description = "v2.5 surface"

[commands]
include = []
exclude = []
add     = []
"#;
        let skill = skill_for_toml(toml);

        // All structural sections must be present.
        for section in &[
            "## Data Model",
            "## Behavioral Contract",
            "## Command Reference",
            "### Estate",
            "### Trees",
            "### Specs",
            "### Tasks",
            "### Discoveries",
            "### Outcomes",
            "### Sessions",
            "### Phase",
            "### Data Management",
            "## Display Conventions",
            "## Error Handling",
            "## Storage",
            "## Schema (v3)",
        ] {
            assert!(
                skill.contains(section),
                "baseline skill missing section: {section}"
            );
        }

        // Baseline must NOT include the non-baseline sections.
        assert!(
            !skill.contains("### Graph Query"),
            "baseline skill must not include Graph Query section"
        );
        assert!(
            !skill.contains("### Overlays"),
            "baseline skill must not include Overlays section"
        );
    }

    #[test]
    fn overlay_exposed_manifest_includes_overlay_commands() {
        // A manifest that adds the overlay commands must produce a skill
        // that documents those commands.
        let toml = r#"
[manifest]
name        = "overlay-exposed"
description = "baseline plus overlay query surface"

[commands]
include = []
exclude = []
add     = ["overlay list", "overlay run"]
"#;
        let skill = skill_for_toml(toml);

        // The overlay commands must appear.
        assert!(
            skill.contains("synthesist overlay run"),
            "overlay-exposed skill must document `synthesist overlay run`"
        );
        assert!(
            skill.contains("synthesist overlay list"),
            "overlay-exposed skill must document `synthesist overlay list`"
        );

        // Baseline commands must still be present (empty include = all baseline).
        assert!(
            skill.contains("synthesist task add"),
            "overlay-exposed skill must still document baseline `synthesist task add`"
        );
        assert!(
            skill.contains("synthesist spec add"),
            "overlay-exposed skill must still document baseline `synthesist spec add`"
        );
    }

    #[test]
    fn pruned_manifest_omits_excluded_sections() {
        // A manifest that omits task management commands should produce a skill
        // without the Tasks section.
        let toml = r#"
[manifest]
name        = "no-tasks"
description = "minimal surface without task management"

[commands]
include = ["status", "init", "spec add", "spec show", "session start", "session close", "skill"]
exclude = []
add     = []
"#;
        let skill = skill_for_toml(toml);

        assert!(
            skill.contains("synthesist spec add"),
            "pruned skill must include spec add"
        );
        // The ### Tasks section must be absent; "synthesist task add" may still
        // appear in the Worked Example section (always included), so we check
        // for the section heading rather than a command mention.
        assert!(
            !skill.contains("### Tasks"),
            "pruned skill must not include the Tasks command-reference section"
        );
    }

    #[test]
    fn manifest_filtered_skill_has_no_em_dashes() {
        // Per project convention: no em dashes in any output.
        let toml = r#"
[manifest]
name        = "em-dash-check"
description = "check for em dashes"
"#;
        let skill = skill_for_toml(toml);
        assert!(
            !skill.contains('\u{2014}'),
            "manifest-filtered skill must not contain em dashes"
        );
    }

    #[test]
    fn cmd_skill_with_manifest_path_works() {
        use std::io::Write;
        // Write a temp manifest file and verify cmd_skill(Some(path)) returns Ok.
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            "[manifest]\nname = \"test\"\ndescription = \"test manifest\"\n"
        )
        .unwrap();
        cmd_skill(Some(f.path())).expect("cmd_skill with manifest path should return Ok");
    }
}

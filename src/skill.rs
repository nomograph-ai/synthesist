//! Skill file emission for LLM agents.
//!
//! The skill file is the primary interface between synthesist and LLM agents.
//! It must be execution-system agnostic (works with Claude Code, Cursor, etc.).

use anyhow::Result;

pub fn cmd_skill() -> Result<()> {
    print!("{SKILL_CONTENT}");
    Ok(())
}

const SKILL_CONTENT: &str = r#"# Synthesist -- Specification Graph Manager (v2)

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
synthesist init                                      # writes claims/genesis.amc
synthesist session start research --tree upstream --spec auth \
  --summary "Auth migration research"

# 2. Set phase and create the work plan
export SYNTHESIST_SESSION=research
synthesist --force phase set plan
synthesist tree add upstream --description "GitLab upstream project"
synthesist spec add upstream/auth --goal "Migrate auth API from v2 to v3"
synthesist task add upstream/auth "Research API versioning strategy"
synthesist task add upstream/auth "Implement token refresh migration" --depends-on t1
synthesist task add upstream/auth "Write integration tests" --depends-on t2 --gate human

# 3. Present plan to human (AGREE phase)
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
| PLAN | Add tasks/specs, add dependencies. No task claims. | agree |
| AGREE | Present plan. No writes. Block until human approves. | execute |
| EXECUTE | Claim tasks, complete tasks. No task creation/cancellation. | reflect, report |
| REFLECT | Assess plan validity, record discoveries. No claims. | execute, replan, report |
| REPLAN | Modify task tree, add/remove tasks. | agree |
| REPORT | Summarize outcomes, record discoveries. Session close. | (end) |

Use `--force` to override phase enforcement when necessary.
The system starts in ORIENT after init. Use `--force phase set plan`
before your first write.

**Phase is per-session** (stored as a Phase claim scoped to the
active session). Concurrent sessions can be in different phases
without interfering. After closing a session, start a fresh one
to begin a new cycle.

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

## Command Reference

### Estate
```
synthesist init                                  # creates claims/genesis.amc
synthesist status                                # trees, task counts, ready tasks, sessions
synthesist check                                 # referential integrity validation
synthesist conflicts                             # list unresolved supersessions
synthesist version                               # version + update check
synthesist skill                                 # this file
```

### Trees
```
synthesist tree add <name> --description TEXT     # e.g. tree add upstream --description "GitLab"
synthesist tree list
```

### Specs
```
synthesist spec add <tree/spec> --goal TEXT        # e.g. spec add upstream/auth --goal "Migrate v2->v3"
synthesist spec show <tree/spec>
synthesist spec update <tree/spec> --status completed --outcome "Shipped in MR !500"
synthesist spec list <tree>                       # e.g. spec list upstream
```
Status values: active, completed, abandoned, superseded, deferred.

### Tasks
IDs auto-generate as t1, t2, ... unless --id is provided.
```
synthesist task add <tree/spec> "summary" --depends-on t1,t2 --gate human --files src/auth.rs
synthesist task list <tree/spec> --active          # hide cancelled tasks
synthesist task show <tree/spec> <id>              # full detail with deps, files, criteria
synthesist task update <tree/spec> <id> --summary "revised summary"
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

### Campaigns
```
synthesist campaign add <tree> <spec-id> --summary "Auth migration" --phase execute
synthesist campaign add <tree> <spec-id> --backlog --title "Future: OAuth2 support"
synthesist campaign list <tree>
```

### Sessions
```
synthesist session start <id> --tree upstream --spec auth --summary "Auth work"
synthesist session close <id>                     # append a closing supersession
synthesist session list                           # show all sessions
synthesist session status <id>                    # claims written in this session
```

Multi-user writes merge automatically via CRDT. Run
`synthesist conflicts` to surface unresolved supersessions.

### Phase
```
synthesist phase show                              # current phase (for active session)
synthesist phase set plan                          # orient -> plan (validated)
synthesist phase set execute                       # fails if not in agree
synthesist --force phase set execute               # override transition validation
```

### Data Management
```
synthesist export                                  # full JSON backup (claim log export)
synthesist import backup.json                      # restore from backup
synthesist sql "SELECT id, summary, status FROM tasks WHERE tree = 'upstream'"
synthesist migrate v1-to-v2                        # port existing .synth/main.db to claims/
```

The standalone binary `migrate-v1-to-v2` has the same effect as the
`migrate v1-to-v2` subcommand; use whichever your install ships.

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

On error: read the message, diagnose the root cause, fix it.
Do not retry the identical command blindly.

## Storage

All state lives in `claims/` at the repo root.

- `claims/genesis.amc` -- git-tracked bootstrap
- `claims/changes/<hash>.amc` -- git-tracked, content-addressed, append-only
- `claims/config.toml` -- git-tracked, schema version
- `claims/snapshot.amc` -- gitignored local compaction cache
- `claims/view.sqlite` -- gitignored local SQL cache of current state
- `claims/view.heads` -- gitignored heads-stale check

The claim log is the source of truth. `view.sqlite` is a rebuildable
local cache. Never read or write these files directly; always use
synthesist subcommands.

Conflict resolution is via **supersession**: concurrent writers that
disagree produce competing supersession chains, and resolution means
appending a new claim that supersedes the contested chain. See the
`nomograph-claim` documentation for the substrate contract.
"#;

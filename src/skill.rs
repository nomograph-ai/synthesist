//! Skill file emission for LLM agents.
//!
//! The skill file is the primary interface between synthesist and LLM agents.
//! It must be execution-system agnostic (works with Claude Code, Cursor, etc.).

use anyhow::Result;

pub fn cmd_skill() -> Result<()> {
    print!("{SKILL_CONTENT}");
    Ok(())
}

const SKILL_CONTENT: &str = r#"# Synthesist -- Specification Graph Manager

## Data Model

Estate > Trees > Specs > Tasks.

- **Tree**: a project domain (e.g. "upstream", "lever", "sysml")
- **Spec**: a unit of work within a tree (e.g. "auth-migration", "gkg-bench")
- **Task**: an atomic work item in a dependency DAG within a spec
- **Discovery**: a timestamped finding recorded during work
- **Stakeholder**: a person relevant to the work (scoped to a tree)
- **Disposition**: an assessed stance a stakeholder holds on a specific topic
- **Signal**: observable evidence supporting a disposition (immutable, bi-temporal)

Path format: `tree/spec` (e.g. "upstream/auth", "lever/ci-containers").
Stakeholders are scoped to trees, not specs: `stakeholder add <tree> <id>`.

## Worked Example: Full Session Lifecycle

This shows a complete workflow from init to reporting.

```bash
# 1. Initialize and start a session
synthesist init
synthesist session start research --tree upstream --spec auth --summary "Auth migration research"

# 2. Set phase and create the work plan
export SYNTHESIST_SESSION=research
synthesist --force phase set plan
synthesist tree add upstream --description "GitLab upstream project"
synthesist spec add upstream/auth --goal "Migrate auth API from v2 to v3"
synthesist task add upstream/auth "Research API versioning strategy"
synthesist task add upstream/auth "Implement token refresh migration" --depends-on t1
synthesist task add upstream/auth "Write integration tests" --depends-on t2 --gate human

# 3. Record stakeholder intelligence
synthesist stakeholder add upstream mwilson --context "lead maintainer, auth team" --name "M. Wilson"
synthesist signal add upstream/auth mwilson \
  --source "https://gitlab.com/upstream/auth/-/merge_requests/412#note_1234" \
  --source-type pr_comment \
  --content "Prefers incremental migration over breaking rewrite. Wants backward compat."
synthesist disposition add upstream/auth mwilson \
  --topic "migration strategy" \
  --stance opposed \
  --confidence documented \
  --preferred "incremental migration with feature flags" \
  --detail "Based on MR !412 review: explicitly rejected breaking-change approach"

# 4. Check stance before proceeding
synthesist stance mwilson
# Returns: {"dispositions": [{"stance": "opposed", "topic": "migration strategy", ...}]}

# 5. Present plan to human (AGREE phase)
synthesist phase set agree
# Present: 3 tasks, t3 has human gate, mwilson opposed to breaking changes.
# Human approves incremental approach. Human says: "proceed"

# 6. Execute
synthesist phase set execute
synthesist task claim upstream/auth t1
# ... do the research work ...
synthesist task done upstream/auth t1

# 7. Reflect -- check what's ready
synthesist phase set reflect
synthesist task ready upstream/auth
# Returns: [{"id": "t2", "summary": "Implement token refresh migration"}]
synthesist discovery add upstream/auth \
  --finding "v3 API supports token refresh natively, no custom implementation needed" \
  --impact "high -- simplifies t2 significantly"

# 8. Continue executing
synthesist phase set execute
synthesist task claim upstream/auth t2
synthesist task done upstream/auth t2
# t3 has gate=human, so present to human before claiming

# 9. Report and merge
synthesist phase set report
synthesist discovery add upstream/auth \
  --finding "Migration completed using incremental approach aligned with mwilson stance"
synthesist session merge research
```

## Behavioral Contract

### Workflow State Machine

7-phase cycle. Phase enforcement is algorithmic -- the CLI rejects
operations that violate the current phase. Transitions are validated.

| Phase | Allowed | Transitions to |
|-------|---------|---------------|
| ORIENT | Read status, query dispositions, read discoveries. No writes. | plan |
| PLAN | Add tasks/specs, add dependencies. No task claims. | agree |
| AGREE | Present plan. No writes. Block until human approves. | execute |
| EXECUTE | Claim tasks, complete tasks. No task creation/cancellation. | reflect, report |
| REFLECT | Assess plan validity, record discoveries. No claims. | execute, replan, report |
| REPLAN | Modify task tree, add/remove tasks. | agree |
| REPORT | Summarize outcomes, record discoveries. Session close. | (end) |

Use `--force` to override phase enforcement when necessary.
The system starts in ORIENT after init. Use `--force phase set plan`
before your first write.

Phase is global state (stored in main.db). After a session merge that
advanced the phase to EXECUTE or REPORT, starting new work requires
resetting: `synthesist --force phase set orient` to begin a fresh cycle.

### Session Protocol

All write operations require a session. Reads within a session see the
session's data (not main.db).

```bash
synthesist session start my-session           # creates isolated .db copy
export SYNTHESIST_SESSION=my-session          # or use --session=my-session
synthesist --force task add tree/spec "task"  # writes to session .db
synthesist task list tree/spec                # reads from session .db
synthesist session merge my-session           # three-way merge to main
```

After merge, the session is closed. Start a new session for more work.

### AGREE Gate

The AGREE phase is a hard gate. The agent presents:
1. The task tree (what will be done, in dependency order)
2. Assumptions and risks
3. Stakeholder dispositions that constrain approach
4. Which tasks need human gates
5. What "done" looks like

The agent halts and waits for explicit human approval.

## Command Reference

### Estate
```
synthesist init                                  # Creates synthesist/main.db
synthesist status                                # Trees, task counts, ready tasks, sessions
synthesist check                                 # Referential integrity validation
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
synthesist task claim <tree/spec> <id>             # pending -> in_progress (atomic, sets owner)
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

### Disposition Graph
```
# Stakeholders are per-tree (not per-spec)
synthesist stakeholder add <tree> <id> --context "lead maintainer" --name "M. Wilson"
synthesist stakeholder list <tree>

# Dispositions are per-spec, scoped to a topic
synthesist disposition add <tree/spec> <stakeholder> \
  --topic "API versioning" --stance opposed --confidence documented \
  --preferred "incremental migration" --detail "Based on MR !412 review"
synthesist disposition list <tree/spec>
synthesist disposition supersede <tree/spec> <old-id> --stance cautious --confidence verified

# Signals are immutable evidence records
synthesist signal add <tree/spec> <stakeholder> \
  --source "https://gitlab.com/project/-/issues/123#note_456" \
  --source-type pr_comment --content "Prefers composition over inheritance"
synthesist signal list <tree/spec>

# Query current stance (dispositions with no valid_until)
synthesist stance <stakeholder>                    # all current dispositions
synthesist stance <stakeholder> "API"              # filter by topic substring
```

Stances: supportive, cautious, opposed, neutral, unknown.
Confidence: documented, verified, inferred, speculative.
Signal types: pr_comment, issue_comment, review, commit_message, chat, meeting, email, other.

### Campaigns
```
synthesist campaign add <tree> <spec-id> --summary "Auth migration" --phase execute
synthesist campaign add <tree> <spec-id> --backlog --title "Future: OAuth2 support"
synthesist campaign list <tree>
```

### Sessions
```
synthesist session start <id> --tree upstream --spec auth --summary "Auth work"
synthesist session merge <id>                      # three-way merge to main
synthesist session merge <id> --dry-run            # preview changes without applying
synthesist session merge <id> --theirs             # on conflict, keep session values
synthesist session list                            # show all sessions
synthesist session status <id>                     # per-table diff summary
synthesist session discard <id>                    # delete session, lose changes
```

### Phase
```
synthesist phase show                              # current phase
synthesist phase set plan                          # orient -> plan (validated)
synthesist phase set execute                       # fails if not in agree
synthesist --force phase set execute               # override transition validation
```

### Data Management
```
synthesist export                                  # full JSON backup
synthesist import backup.json                      # restore from backup
synthesist sql "SELECT id, summary, status FROM tasks WHERE tree = 'upstream'"
synthesist skill                                   # this file
synthesist version                                 # version + update check
```

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

Data: `synthesist/main.db` (SQLite, git-tracked).
Sessions: `synthesist/sessions/*.db` (gitignored, ephemeral).
Never read or write these files directly. Use synthesist commands.
"#;

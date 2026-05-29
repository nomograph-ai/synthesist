---
name: synthesist
description: >-
  Operates the Synthesist specification-graph CLI (trees, specs, tasks, sessions,
  stakeholders, propagation). Use when the user mentions synthesist, .synth,
  estate/spec/task state, Dolt, or wants planning or execution tracked in
  Synthesist. Always honor the project rule that forbids bypassing the CLI for
  database access.
---

# Synthesist (project skill)

## Authority

The **data and mediation contract** lives in `.cursor/rules/synthesist-skill.mdc` (always applied in this repo). This skill adds workflow, display rules, and the command catalog. If a future Synthesist version changes flags or commands, prefer `synthesist skill` (or `synthesist <cmd> --help`) over stale prose.

## Mediation

You are the layer between the human and the tool: run `synthesist` from the repo root (or the directory that contains `.synth/`). The human does not need to invoke the CLI themselves unless they choose to.

## Sessions and writes

Write operations require **`--session=<id>`** or the **`SYNTHESIST_SESSION`** environment variable. Shell state does not persist across separate tool invocations: pass `--session=...` on every write in each command, or export `SYNTHESIST_SESSION` in the same shell before a batch.

Pattern:

```bash
S="synthesist --session=my-session"
$S task create mordecai/my/spec "Summary here"
```

Reads (`status`, `check`, `task list`, `spec show`, `export`, etc.) do not require a session.

## Session start sequence

1. `synthesist session start <descriptive-name>`
2. `synthesist --session=<name> phase set orient`
3. `synthesist session list` — abandoned sessions from prior runs
4. `synthesist status` — estate overview and ready tasks
5. For the spec in focus:
   - `synthesist spec show <tree/spec>`
   - `synthesist discovery list <tree/spec>`
   - `synthesist retro show <tree/spec>` if relevant
6. Summarize for the human in plain language (see Display rules)
7. If they stated intent, acknowledge and move toward PLAN

## State machine

```
ORIENT → PLAN → AGREE → EXECUTE ↔ REFLECT → REPORT
                  ↑                    |
                  └──── REPLAN ←───────┘
```

After every phase transition: `synthesist --session=<name> phase set <phase>`

| Phase   | Purpose             | Allowed                                                              | Forbidden                                                       | Transition                     |
| ------- | ------------------- | -------------------------------------------------------------------- | --------------------------------------------------------------- | ------------------------------ |
| ORIENT  | Shared mental model | Read: status, task list, spec show, discovery list, session list     | Any writes                                                      | → PLAN when work is indicated  |
| PLAN    | Model work          | Read + spec create/update, task create, discovery add, research      | Claiming tasks, repo code edits, modifying non-synthesist files | → AGREE when plan complete     |
| AGREE   | Human checkpoint    | Present only — wait                                                  | Everything else                                                 | → EXECUTE on explicit approval |
| EXECUTE | Do work             | task claim/done/block/wait, discovery add, code edits scoped to task | Creating/cancelling tasks, reshaping task tree                  | → REFLECT after each task      |
| REFLECT | Assess plan         | Read + discovery add                                                 | Claim next task before assessment                               | → EXECUTE / REPLAN / REPORT    |
| REPLAN  | Adjust plan         | task create/cancel/block, spec update, discovery add                 | Claiming tasks                                                  | → AGREE (human re-approves)    |
| REPORT  | Hand off            | Read only                                                            | Writes                                                          | → ORIENT                       |

## AGREE protocol

Before execution: present grouped task table, assumptions, file/repo scope, human-gated vs autonomous tasks, and definition of done. Then **stop** until the human explicitly approves (“yes”, “proceed”, “approved”, etc.). “Ready to proceed?” without their answer is not approval.

## Task types

- **Discussion** tasks — close in PLAN/AGREE; do not claim in EXECUTE.
- **Implementation** tasks — claim in EXECUTE; complete with `task done` so acceptance criteria run.

## Display rules

1. Task trees: grouped markdown tables, not raw JSON.
2. Task changes: diff-style table of adds/cancels before showing current tree.
3. Cancelled tasks: report count only unless asked.
4. ORIENT summaries: plain language.
5. Prefer `synthesist task list <tree/spec> --active` when showing current work.

## Listing specs

There is no `synthesist spec list`. Use:

- `synthesist tree list` — trees
- `synthesist status` — threads and ready tasks (partial spec coverage)
- `synthesist export` — parse the `specs` array for a full inventory (read-only analysis)
- `synthesist archive list <tree>` — archived specs for that tree

## Concepts (quick)

- **Tree**: domain of work; create with `tree create` before attaching specs.
- **`tree/spec`**: positional ID for most commands, e.g. `mordecai/patient-web/foo`.
- **Campaign commands**: `campaign active|backlog|list` take `<tree>` and `<spec-id>` as **two** arguments, not `tree/spec`.
- **Threads**: `thread create` / `thread list`; `status` surfaces active threads.
- **Propagation**: `propagation add` / `propagation list` / `propagation check` for downstream staleness.

## Enums (common)

**stance**: supportive | cautious | opposed | neutral | unknown
**confidence**: documented | verified | inferred | speculative
**signal type**: pr_comment | issue_comment | review | commit_message | chat | meeting | email | other
**direction status**: committed | proposed | experimental | rejected
**task status**: pending | in_progress | done | blocked | waiting | cancelled
**archive reason**: completed | abandoned | superseded | deferred

## When to use which command

- Bootstrap: `synthesist init` / `synthesist scaffold`
- Orientation: `session start`, `phase set`, `session list`, `status`
- Planning: `spec create` | `spec update`, `task create`, `discovery add`
- Doing work: `task claim`, `task done` (runs verify), `task block`, `task wait`
- People: `stakeholder add`, `disposition add`, `signal record`
- Campaigns / archive: `campaign …`, `archive add`, `archive list`
- Cross-spec: `propagation add`, `propagation check`
- Closure: `retro create`, `retro transform`, then archive as appropriate
- Health / backup: `check`, `migrate`, `export`; stale sessions: `session prune`
- Adaptation: `replay <tree/spec>`

## Output

CLI output is JSON: parse it; empty collections are `[]`. Present humans with tables or prose, not raw dumps, unless they ask.

## Error protocol

1. Do not retry silently — explain failures.
2. Harder than expected → REPLAN → AGREE.
3. Record institutional memory with `discovery add`, not only in chat.

## Long autonomous runs

After approval: tasks in dependency order; skip human-gated tasks but note them; before context fills, enter REPORT, `discovery add`, commit code; next session continues from Synthesist state.

## Core commands (check `synthesist skill` for your installed version)

```
synthesist init
synthesist scaffold
synthesist status
synthesist check
synthesist tree create <name> [flags]
synthesist tree list
synthesist thread create --tree=STRING --summary=STRING <id> [flags]
synthesist thread list
synthesist campaign active <tree> <spec-id> [flags]
synthesist campaign backlog <tree> <spec-id> [flags]
synthesist campaign list <tree>
synthesist archive add --reason=STRING <tree-spec> [flags]
synthesist archive list <tree>
synthesist discovery add --finding=STRING <tree-spec> [flags]
synthesist discovery list <tree-spec>
synthesist task create <tree-spec> <summary> [flags]
synthesist task list <tree-spec> [--human] [--active]
synthesist task claim <tree-spec> <task-id>
synthesist task done <tree-spec> <task-id> [flags]
synthesist task wait --reason=STRING --external=STRING --check=STRING <tree-spec> <task-id> [flags]
synthesist task block <tree-spec> <task-id>
synthesist task ready <tree-spec>
synthesist task acceptance --criterion=STRING --verify=STRING <tree-spec> <task-id>
synthesist task cancel <tree-spec> <task-id> [flags]
synthesist stakeholder add --context=STRING <tree> <id> [flags]
synthesist stakeholder list <tree>
synthesist disposition add --topic=STRING --stance=STRING --confidence=STRING <tree-spec> <stakeholder> [flags]
synthesist disposition list <tree-spec>
synthesist disposition supersede --new-stance=STRING <tree-spec> <disposition-id> [flags]
synthesist signal record --source=STRING --type=STRING --content=STRING <tree-spec> <stakeholder> [flags]
synthesist signal list <tree-spec>
synthesist direction add --project=STRING --topic=STRING --status=STRING --impact=STRING <tree> [flags]
synthesist direction list <tree>
synthesist direction impact --affected-tree=STRING --affected-spec=STRING --description=STRING <tree> <direction-id>
synthesist spec create <tree-spec> [flags]
synthesist spec show <tree-spec>
synthesist spec update <tree-spec> [flags]
synthesist propagation add <source> <target> [flags]
synthesist propagation list <tree-spec>
synthesist propagation check <tree-spec>
synthesist retro create --arc=STRING <tree-spec> [flags]
synthesist retro show <tree-spec>
synthesist retro transform --label=STRING --description=STRING <tree-spec> [flags]
synthesist pattern register --name=STRING --description=STRING <tree> <id> [flags]
synthesist pattern list <tree>
synthesist ready <tree-spec>
synthesist landscape show <tree-spec>
synthesist stance <stakeholder-id> [<topic>]
synthesist replay <tree-spec>
synthesist phase set <name>
synthesist phase show
synthesist session start <session-id> [flags]
synthesist session merge <session-id> [flags]
synthesist session list
synthesist session status <session-id>
synthesist session prune [flags]
synthesist migrate
synthesist export
synthesist import [<file>]
synthesist skill
synthesist version
```

Default commits: the tool auto-commits Dolt state; use `--no-commit` only when batching intentional multi-step writes (see upstream `--help` on subcommands that support it).

## Sources (rationale)

SOAR, Reflexion, LangGraph checkpoints, OODA, ACT-R-style phase rules, persistent task DAGs (Beads/Gastown pattern).

# Synthesist Specification Format (v5)

Specs are organized into **context trees** -- named domains that scope agent context
to one area of work at a time. Trees, campaigns, archives, and all state data live
in the Dolt embedded database at `.synth/`, managed exclusively through the
`synthesist` CLI. Agents never read or write data files directly.

Each spec has a human-written intent file and an optional discovery log:

| File | Format | Owner | Purpose |
|------|--------|-------|---------|
| `spec.md` | Markdown with YAML frontmatter and XML sections | Human / Agent | Intent, context, constraints, decisions |
| `discovery.md` | Markdown (append-only) | Agent | Findings persisted before context fills |

Task state, stakeholder intelligence, dispositions, signals, patterns, and all
other structured data live in the database. Use `synthesist task list <tree/spec>`,
`synthesist landscape show <tree/spec>`, etc. to query state.

---

## spec.md — The Specification

The spec is the source of truth for *what* and *why*. Agents read it for context.
Humans write and review it.

### Structure

```markdown
---
id: feature-slug
title: Human-readable feature title
status: draft | review | approved | in-progress | done
created: YYYY-MM-DD
updated: YYYY-MM-DD
author: human | plan
---

<goal>
One paragraph describing what success looks like. Be specific enough that
someone could verify it without reading the rest of the spec.
</goal>

<context>
Files and resources the agent needs to understand before working.
Each entry has a path and a reason for inclusion.

- path: src/relevant/module.ts
  why: Contains the interface we're extending
- path: docs/architecture.md
  why: Current system design constraints
</context>

<constraints>
Hard requirements that all tasks must satisfy. These are non-negotiable.

- Must maintain backward compatibility with v2 API
- No new runtime dependencies
- All changes must pass existing test suite
- Response time must not increase by more than 50ms p99
</constraints>

<decisions>
Design decisions made during the discuss/plan phase. Record the question,
the answer, and the rationale so future agents don't re-litigate.

- question: Use REST or GraphQL for the new endpoint?
  answer: REST
  rationale: Aligns with existing API surface, minimizes client changes

- question: Store in PostgreSQL or Redis?
  answer: PostgreSQL
  rationale: Need ACID guarantees for this data, already have the schema
</decisions>

<discovery>
Agent-appended findings during exploration and implementation.
This section grows over the life of the feature. Append only.

- finding: Auth middleware doesn't support API key authentication
  impact: Need to extend auth before the API endpoint task
  action: Added t0 for auth extension, t2 now depends on t0
  agent: plan
  date: 2026-03-08
</discovery>
```

### Section Rules

- `<goal>` is required. Everything else is strongly recommended.
- `<context>` paths are relative to the target project root.
- `<constraints>` are verified during quality review.
- `<decisions>` prevent agents from re-opening settled questions.
- `<discovery>` is append-only. Never delete entries. Any agent may append.

---

## Task DAG (database)

Task state lives in the Dolt database, managed via the `synthesist` CLI.
Agents create, claim, and complete tasks through commands -- never by writing
JSON files. The schema below documents the fields for reference.

### Creating tasks

```bash
synthesist task create <tree/spec> "summary" [--depends-on t1,t2] [--gate human] [--files f1,f2]
synthesist task list <tree/spec>
synthesist task ready <tree/spec>
synthesist task claim <tree/spec> <id>
synthesist task done <tree/spec> <id>    # runs acceptance criteria automatically
```
```

### Field Reference

#### Task Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Unique within this spec. Convention: t1, t2, ... |
| `summary` | string | yes | One-line task description |
| `description` | string | no | Implementation details, hints, context |
| `files` | string[] | yes | Files this task will create or modify |
| `depends_on` | string[] | yes | Task IDs that must be "done" before this task starts |
| `type` | enum | no | `"task"` (default) or `"retro"`. Retro nodes are created at spec completion. |
| `status` | enum | yes | `pending` \| `in_progress` \| `done` \| `blocked` \| `waiting` |
| `gate` | string\|null | no | `"human"` = requires human approval before starting. `null` = no gate |
| `acceptance` | object[] | yes | At least one acceptance criterion with a verify command |
| `failure_note` | string\|null | no | Set by `synthesist task done` when a criterion fails. Cleared when task restarts |
| `owner` | string\|null | no | Identifies which session/agent owns an in_progress task. Set when claiming, cleared on completion. |
| `created` | date\|null | no | When the task was created. YYYY-MM-DD. |
| `completed` | date\|null | no | When the task reached done. Set on status transition. |
| `waiter` | object\|null | no | For `waiting` status: describes external blocker. See Waiter Object. |
| `arc` | string\|null | no | Retro nodes only. What we set out to do vs what happened (2-3 sentences). |
| `transforms` | object[]\|null | no | Retro nodes only. Key moves that characterized this work. See Transform Object. |
| `duration_days` | int\|null | no | Retro nodes only. Created to completed, computed. |

#### Concurrency Rules

The `synthesist` CLI handles concurrency for task state -- ownership, dependency
checking, and commits are managed by the binary:

1. `synthesist task claim` checks ownership and dependencies before granting a task.
2. `synthesist task done` clears ownership and auto-commits on completion.
3. Pull before starting work if the repo has multiple contributors or sessions.
4. The binary auto-commits to both Dolt and git on state changes (disable with `--no-commit`).

#### Acceptance Criterion Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `criterion` | string | yes | Human-readable statement of what must be true |
| `verify` | string | yes | Shell command. Exit 0 = pass. Non-zero = fail. |

#### Quality Fields

| Field | Type | Description |
|-------|------|-------------|
| `score` | float\|null | 0.0-1.0 overall quality score from review |
| `validations` | object[] | Array of review entries |

Each validation entry:

```json
{
  "reviewer": "review",
  "date": "2026-03-08",
  "score": 0.85,
  "findings": "2 warnings, 0 critical. See review output.",
  "tasks_reviewed": ["t1", "t2", "t3"]
}
```

#### Waiter Object

For tasks with `status: "waiting"`. Describes an external blocker with a
machine-checkable resolution condition.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `reason` | string | yes | Human-readable: what we are waiting for |
| `external` | string | yes | URL or reference (MR, issue, Slack thread) |
| `check` | string | yes | Shell command that exits 0 when the wait is resolved |
| `check_after` | date\|null | no | Skip checking before this date (avoids spamming remote APIs) |

#### Transform Object

For retro nodes (`type: "retro"`). Captures a key move in the work,
abstract enough to replay in a different context.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `label` | string | yes | Short name for this move (e.g. `schema-before-migration`) |
| `description` | string | yes | What we did and why (1-2 sentences) |
| `transferable` | boolean | yes | Would this move make sense in a different project context? |

### Status Transitions

```
pending ──► in_progress ──► done
  ▲                          │
  └──────────────────────────┘
        (verify failure)

pending ──► blocked
              │
              └──► pending (when blocker resolves)

pending ──► waiting ──► done (check passes)
              │
              └──► pending (retracted/refiled)
```

The `synthesist` CLI enforces these transitions:
- `synthesist task claim` transitions `pending -> in_progress`
- `synthesist task done` runs acceptance criteria; transitions to `done` if all pass, or resets to `pending` if any fail
- `synthesist task wait` transitions to `waiting` with a waiter object
- `synthesist task block` transitions to `blocked`
- Human gates: tasks with `--gate human` require explicit approval before work begins
- `waiting` is for external blockers (MRs under review, issues awaiting response).
  The `waiter.check` command determines if the wait is resolved.

### Verify Command Guidelines

Good verify commands:
- `grep -q 'export.*XModel' src/models/x.ts` — checks a symbol exists
- `npm test -- --grep 'XModel'` — runs a specific test
- `curl -sf http://localhost:3000/api/x | jq '.data | length > 0'` — checks API response
- `test -f src/routes/x.ts` — checks a file was created

Bad verify commands:
- `echo "looks good"` — always passes, verifies nothing
- `npm test` — runs entire test suite, too slow and not specific
- (empty) — no verification at all

---

## discovery.md — Institutional Memory

Optional file, created on first append. Append-only — never delete entries.

```markdown
# Discovery Log

## 2026-03-08 — plan agent

Auth middleware at src/middleware/auth.ts only supports JWT bearer tokens.
API key authentication needs to be added before the new endpoint can work.
Added task t0 via `synthesist task create` for auth extension.

## 2026-03-08 — build agent

The existing User model uses soft deletes. The new endpoint must filter
deleted records. Updated t2 acceptance criteria to include this check.
```

---

## Landscape -- Stakeholder Intelligence (v5)

Stakeholder intelligence is stored in the Dolt database, managed via CLI commands.
It records the human landscape relevant to the work: who influences whether it
lands, what they've signaled about technical direction, and how assessments change.

### Why

LLMs move fast writing code. The delta between proposed implementation and
what a maintainer/reviewer will accept is the real cost. Disposition tracking
models that delta so agents can make informed implementation choices instead
of contributing blind.

### CLI commands

```bash
synthesist stakeholder add <tree> <id> --context "role" [--name "Full Name"] [--orgs "org1,org2"]
synthesist stakeholder list <tree>
synthesist disposition add <tree/spec> <stakeholder> --topic "..." --stance cautious --confidence inferred [--preferred "..."]
synthesist disposition list <tree/spec>
synthesist disposition supersede <tree/spec> <id> --new-stance supportive [--evidence <signal-id>]
synthesist signal record <tree/spec> <stakeholder> --source "url" --type pr_comment --content "..."
synthesist signal list <tree/spec>
synthesist landscape show <tree/spec>      # full stakeholder graph for a spec
synthesist stance <stakeholder>            # current dispositions across tree
```
```

### Disposition Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Unique within this landscape. Convention: d1, d2, ... |
| `stakeholder` | string | yes | Ref to stakeholder ID in tree-level stakeholder registry |
| `topic` | string | yes | Technical direction or implementation choice, not general sentiment |
| `stance` | enum | yes | `supportive` \| `cautious` \| `opposed` \| `neutral` \| `unknown` |
| `preferred_approach` | string | no | What direction they favor. The signal that constrains our choices. 1-2 sentences. |
| `detail` | string | no | Additional context. 1-2 sentences. |
| `confidence` | enum | yes | `documented` \| `verified` \| `inferred` \| `speculative` |
| `valid_from` | date | yes | When this assessment became current |
| `valid_until` | date\|null | no | Null means still current. Set when superseded. |
| `superseded_by` | string\|null | no | ID of the disposition that replaces this one |

### Signal Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Unique within this landscape. Convention: s1, s2, ... |
| `stakeholder` | string | yes | Who produced this signal |
| `date` | date | yes | When the signal was observed |
| `source` | string | yes | URL or reference to the source |
| `source_type` | enum | yes | `pr_comment` \| `issue_comment` \| `review` \| `commit_message` \| `chat` \| `meeting` \| `email` \| `other` |
| `content` | string | yes | Direct quote or close paraphrase |
| `interpretation` | string | no | What this means for our strategy |

### Influence Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `stakeholder` | string | yes | Ref to stakeholder ID |
| `task` | string | yes | Ref to task ID in this spec's task DAG |
| `role` | enum | yes | `maintainer` \| `reviewer` \| `approver` \| `blocker` \| `champion` \| `observer` |

### Temporal Model

Dispositions have validity windows. When new evidence changes an assessment:

1. Set `valid_until` on the old disposition
2. Create a new disposition with updated stance/approach
3. Set `superseded_by` on the old to reference the new

History is preserved. The query "what did we think this person's stance was
on date X?" is: filter dispositions by stakeholder + topic, find the one
where `valid_from <= X` and (`valid_until > X` or `valid_until` is null).

Signals are immutable once recorded. They are the evidence chain.

---

## Stakeholders -- Tree-Level Registry (v5)

Stakeholders are registered per-tree in the database. Each stakeholder is defined
once and referenced by dispositions and influences across specs in that tree.

```bash
synthesist stakeholder add upstream cgwalters --context "bootc maintainer" --name "Colin Walters" --orgs "containers/bootc,ostreedev/ostree"
synthesist stakeholder list upstream
```
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Handle or stable identifier |
| `name` | string | no | Full name |
| `context` | string | yes | Role relative to the work, not job title |
| `orgs` | string[] | no | Projects/orgs they maintain or represent |

---

## Patterns -- Tree-Level Pattern Registry (v5)

Named patterns discovered through retrospective analysis, stored per-tree in
the database. Retro nodes reference patterns by ID.

```bash
synthesist pattern register upstream tracking-spec --name "Tracking Spec Pattern" \
  --description "Lightweight spec for work with no active build tasks" \
  --transferability "Any project tracking external work" \
  --observed-in "harness/estate-campaign-contract"
synthesist pattern list upstream
```
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Unique within this tree. Used by retro nodes' patterned edges. |
| `name` | string | yes | Human-readable label |
| `description` | string | yes | What the pattern is |
| `transferability` | string | no | What other contexts this fits |
| `first_observed` | date | yes | When this pattern was first identified |
| `observed_in` | string[] | yes | Spec IDs where this pattern was used. Cross-tree refs allowed. |

---

## Retro Nodes

A retrospective is a task node with `type: "retro"` that depends on the final
task(s) of a spec. Retro nodes enable replay: "play back this sub-tree from
project A onto project B."

The task DAG provides the structure of the work. The retro provides the
interpretive layer: what transforms we made, why, and what to adapt.

### Creating a Retro Node

When a spec completes:

```bash
synthesist retro create <tree/spec> --arc "what we set out to do vs what happened" --depends-on t8,t9
synthesist retro transform <tree/spec> --label "schema-before-migration" --description "what we did and why" --transferable
synthesist pattern register <tree> <id> --name "..." --description "..."
synthesist retro show <tree/spec>
```

### Replay Model

To replay a sub-tree from project A onto project B, run `synthesist replay <tree/spec>`.
This outputs the task DAG shape, retro transforms, patterns, and landscape summary.
The agent reads the transforms (what moves were made and why), checks the landscape
(what stakeholder constraints shaped choices), and generates a new spec in the target
tree with adapted tasks. Replay is interpreted, not directly applied.

---

## Archive Extension (v5)

Archive entries in the database gain optional fields for queryable retrospective data.
The `archives` table stores completed/deferred specs with enriched metadata:
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `duration_days` | int | no | Created to archived (v5) |
| `patterns` | string[] | no | Pattern IDs exercised or created by this spec (v5) |
| `contributions` | string[] | no | Paths to contribution records generated (v5) |

---

## Task Sizing

Each task MUST fit in one agent context window. Rules of thumb:

- If a task touches more than 5 files, split it
- If a task requires understanding more than 3 modules, split it
- If a task description exceeds 500 words, split it
- If you can't write a specific verify command, the task is too vague

When splitting, maintain the dependency DAG: child tasks depend on parent.

---

## Lifecycle

```
1. Discuss    Human describes intent. Agent asks clarifying questions.
              Output: shared understanding.

2. Draft      Agent writes spec.md. Creates tasks via synthesist task create.
              Human reviews, iterates.

3. Build      Agent claims tasks (synthesist task claim), executes in
              dependency order. synthesist task done runs acceptance
              criteria and marks complete only if they pass.

4. Record     Agent writes findings to discovery.md. When work completes,
              creates retro node via synthesist retro create.

5. Complete   All tasks done, all criteria pass.
              synthesist retro create captures arc + transforms.
```

---

## Context Trees

Specs are organized into **context trees** -- named domains that scope work to a
single area. Tree metadata, campaigns, and archives live in the Dolt database.
Human-readable spec files (spec.md, discovery.md) live on the filesystem.

### Tree Structure

```
your-project/
├── .synth/                        <- Dolt database (trees, tasks, landscape, etc.)
│   └── synthesist/.dolt/
├── specs/
│   ├── SPEC_FORMAT.md             <- this file
│   ├── <tree-name>/               <- one context tree
│   │   ├── decisions.md           <- optional: locked decisions for this domain
│   │   ├── <spec>/                <- a spec directory
│   │   │   ├── spec.md            <- human-written intent
│   │   │   └── discovery.md       <- agent-written findings
│   │   └── archive/               <- archived spec directories
│   │       └── <old-spec>/
│   └── _template/                 <- spec templates
└── ...
```

Trees are named after the service or domain they support. Examples:
- `auth.dunn.dev` — SSO service
- `immutable.dunn.dev` — image supply chain
- `upstream` — open source contributions
- `ops` — operational tasks

### Phases

Trees may optionally group specs into **phases** — subdirectories that batch
related work by session or milestone. Phases are optional; small trees can have
flat specs directly under the tree root.

---

## Estate -- Meta-Switchboard

The estate is the top-level entry point. It lists all context trees, their status,
and active threads. Run `synthesist status` at session start.

```bash
synthesist status    # shows trees, threads, task counts, ready tasks
```

### Trees table

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Tree identifier (e.g. "upstream", "harness") |
| `status` | string | `active`, `dormant`, or `archived` |
| `description` | string | One-line description of this tree's domain |

### Threads table

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Unique key: `{tree}/{spec}` or `{tree}/{account}`. Stable across sessions. |
| `tree` | string | yes | Which tree this thread lives in |
| `spec` | string | no | Active spec ID, or null for tree-level work |
| `task` | string | no | Active task ID within the spec, or null |
| `date` | date | yes | Last update date (YYYY-MM-DD) |
| `summary` | string | yes | Current state and what's next |

### Session Entry Protocol

1. Run `synthesist status`
2. Review active threads sorted by date (most recent first)
3. Offer to continue any thread, or start a new one
4. Follow spec/task pointers to load context for current work

---

## Campaign -- Tree-Level Coordination

Campaign state lives in the database, tracking active and backlog specs per tree.

### Campaign tables

**campaign_active** -- specs currently being worked:

| Field | Type | Description |
|-------|------|-------------|
| `tree` | string | Tree name |
| `spec_id` | string | Spec identifier |
| `path` | string | Path to spec.md |
| `summary` | string | One-line description |
| `phase` | string | Optional phase grouping |

**campaign_backlog** -- ideas modeled but not yet started:

| Field | Type | Description |
|-------|------|-------------|
| `tree` | string | Tree name |
| `spec_id` | string | Spec identifier |
| `title` | string | Human-readable title |
| `summary` | string | One-line description |

### Campaign Rules

- A spec moves from backlog to active when work begins
- A spec moves from active to the archives table when archived
- `blocked_by` references spec IDs within the same tree, or cross-tree refs
  in the format `<tree>/<spec-id>`
- Backlog items may be stubs (no spec.md yet) or full specs

---

## Archives -- Archived Specs

Each tree's archived specs live in the `archives` table in the database.
Archive is append-only. Spec directories on the filesystem move to the
`archive/` subdirectory under the tree.
```

### Archive Reasons

| Reason | Meaning |
|--------|---------|
| `completed` | All tasks done, acceptance criteria met |
| `abandoned` | Work stopped, not worth continuing |
| `superseded` | Replaced by a different spec |
| `deferred` | Parked indefinitely, may return to it later |

### Archive Rules

- When archiving, move the spec directory to `archive/` under the tree
- The database archives table records the reason, outcome, and metadata
- `synthesist check` validates cross-references including archived specs
- Archive is append-only -- never delete entries

---

## Cross-References

Specs may reference other specs, including specs in different context trees.
Cross-references are declared in spec.md using a `<references>` section.

### Format

```xml
<references>
- spec: upstream/immich/oidc-callback
  type: discovered_from
  note: "Found incorrect callback path during bench testing"
- spec: immutable.dunn.dev/carmine/installer
  type: informs
  note: "Blueprints from bench will be deployed via carmine installer"
</references>
```

### Reference Types

| Type | Meaning |
|------|---------|
| `blocked_by` | This spec cannot proceed until the referenced spec is done |
| `informs` | Findings here affect the referenced spec |
| `discovered_from` | This spec was created because of work on the referenced spec |

### Reference Resolution

References use the format `<tree>/<spec-id>` (or `<tree>/<phase>/<spec-id>`).
`synthesist check` resolves references by checking the campaign and archive
tables in the database. References to archived specs are valid.

---

## Session State

Session state lives in the `threads` table in the database. Each thread
represents an active workstream -- a spec being built, an account being
researched, a contribution being tracked. Multiple sessions can run concurrently.

### Session Entry Protocol

1. Run `synthesist status`
2. Review active threads sorted by date (most recent first)
3. Offer to continue any thread, or start a new one
4. Follow spec/task pointers to load context for current work

### Session End Protocol

Before the session ends (or when context is getting heavy):
- Ensure all task status changes are committed through the CLI
- Write findings to discovery.md
- Pending decisions go into the relevant spec's `<discovery>` section

### Concurrent Sessions

Multiple sessions may run simultaneously. The `synthesist` CLI manages task
ownership and state transitions. Each session can claim and complete tasks
independently without conflicting with other sessions.

# Synthesist Specification Format (v3)

Specs are organized into **context trees** — hierarchical directories that scope
agent context to one domain of work at a time. Each tree has its own campaign.json
(active work) and archive.json (completed/deferred work).

Every spec gets two files in its directory:

| File | Format | Owner | Purpose |
|------|--------|-------|---------|
| `spec.md` | Markdown with YAML frontmatter and XML sections | Human / Plan agent | Intent, context, constraints, decisions |
| `state.json` | JSON | Build agent / Verify agent | Task DAG, status, acceptance criteria |

An optional third file, `discovery.md`, holds findings that agents persist before
their context windows fill.

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
- `<constraints>` are verified by the `@review` agent during quality review.
- `<decisions>` prevent agents from re-opening settled questions.
- `<discovery>` is append-only. Never delete entries. Any agent may append.

---

## state.json — The Task DAG

The state file is the source of truth for *what to do next* and *whether it worked*.
Agents update it. Humans read it via git diff.

### Schema

```json
{
  "spec": "specs/feature-slug/spec.md",
  "tasks": [
    {
      "id": "t1",
      "summary": "One-line description of the task",
      "description": "Optional longer description with implementation notes",
      "files": ["src/models/x.ts", "src/models/x.test.ts"],
      "depends_on": [],
      "status": "pending",
      "gate": null,
      "acceptance": [
        {
          "criterion": "Human-readable description of what must be true",
          "verify": "shell command that exits 0 on success, non-zero on failure"
        }
      ],
      "failure_note": null
    }
  ],
  "quality": {
    "score": null,
    "validations": []
  }
}
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
| `failure_note` | string\|null | no | Set by @verify when a criterion fails. Cleared when task restarts |
| `owner` | string\|null | no | Identifies which session/agent owns an in_progress task. Set when claiming, cleared on completion. |
| `created` | date\|null | no | When the task was created. YYYY-MM-DD. |
| `completed` | date\|null | no | When the task reached done. Set on status transition. |
| `waiter` | object\|null | no | For `waiting` status: describes external blocker. See Waiter Object. |
| `arc` | string\|null | no | Retro nodes only. What we set out to do vs what happened (2-3 sentences). |
| `transforms` | object[]\|null | no | Retro nodes only. Key moves that characterized this work. See Transform Object. |
| `duration_days` | int\|null | no | Retro nodes only. Created to completed, computed. |

#### Concurrency Rules

When multiple sessions may operate on the same spec tree:

1. **Commit after every status change.** When a task transitions to "done" (and verify
   passes), commit state.json and modified files immediately. Do not batch.
2. **Check owner before claiming.** Read state.json from disk before setting a task to
   "in_progress". If `owner` is set and status is "in_progress", skip to the next task.
3. **Set owner when claiming.** Write a session identifier to the `owner` field when
   transitioning a task to "in_progress".
4. **Clear owner on completion.** Set `owner` to null when the task reaches "done".
5. **Pull before starting.** If the spec tree is version-controlled, pull latest state
   before picking up tasks.

#### Acceptance Criterion Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `criterion` | string | yes | Human-readable statement of what must be true |
| `verify` | string | yes | Shell command. Exit 0 = pass. Non-zero = fail. |

#### Quality Fields

| Field | Type | Description |
|-------|------|-------------|
| `score` | float\|null | 0.0–1.0 overall quality score from @review |
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

Only the verify agent may transition `done → pending` (on verification failure).
Only the build agent may transition `pending → in_progress → done`.
Human gates: the build agent sets status to `blocked` and waits for human input,
then the human (or build agent after approval) sets it back to `pending`.
`waiting` is for external blockers (MRs under review, issues awaiting response).
The `waiter.check` command is run to determine if the wait is resolved. Only the
verify agent runs check commands and transitions `waiting → done`.

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
Added task t0 to state.json for auth extension.

## 2026-03-08 — build agent

The existing User model uses soft deletes. The new endpoint must filter
deleted records. Updated t2 acceptance criteria to include this check.
```

---

## landscape.json -- Stakeholder Intelligence (v5)

Optional file per spec. Records the human landscape relevant to this work:
who influences whether it lands, what they've signaled about technical direction,
and how those assessments change over time.

The primary consumer is an LLM agent. Every query maps to a JSON read plus
filter. No graph traversal engine required.

### Why

LLMs move fast writing code. The delta between proposed implementation and
what a maintainer/reviewer will accept is the real cost. Disposition tracking
models that delta so agents can make informed implementation choices instead
of contributing blind.

### Schema

```json
{
  "spec": "specs/upstream/bootc/composefs-timestamps/spec.md",
  "stakeholders": ["cgwalters", "jmarrero"],
  "dispositions": [
    {
      "id": "d1",
      "stakeholder": "cgwalters",
      "topic": "composefs timestamp preservation strategy",
      "stance": "cautious",
      "preferred_approach": "prefers upstream kernel solution over userspace workaround",
      "detail": null,
      "confidence": "inferred",
      "valid_from": "2026-03-25",
      "valid_until": null,
      "superseded_by": null
    }
  ],
  "signals": [
    {
      "id": "s1",
      "stakeholder": "cgwalters",
      "date": "2026-03-25",
      "source": "https://github.com/containers/bootc/pull/123#comment-456",
      "source_type": "pr_comment",
      "content": "direct quote or close paraphrase, attributed",
      "interpretation": "what this signal means for our implementation choices"
    }
  ],
  "influences": [
    {
      "stakeholder": "cgwalters",
      "task": "t1",
      "role": "maintainer"
    }
  ]
}
```

### Disposition Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Unique within this landscape. Convention: d1, d2, ... |
| `stakeholder` | string | yes | Ref to stakeholder ID in tree-level `stakeholders.json` |
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
| `task` | string | yes | Ref to task ID in this spec's state.json |
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

## stakeholders.json -- Tree-Level Registry (v5)

Optional file per context tree. Defines stakeholder identity once; referenced
by `landscape.json` files across specs in the tree.

```json
{
  "tree": "upstream",
  "stakeholders": [
    {
      "id": "cgwalters",
      "name": "Colin Walters",
      "context": "bootc maintainer",
      "orgs": ["containers/bootc", "ostreedev/ostree"]
    }
  ]
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Handle or stable identifier |
| `name` | string | no | Full name |
| `context` | string | yes | Role relative to the work, not job title |
| `orgs` | string[] | no | Projects/orgs they maintain or represent |

---

## patterns.json -- Tree-Level Pattern Registry (v5)

Optional file per context tree. Named patterns discovered through retrospective
analysis. Retro nodes reference these by ID.

```json
{
  "tree": "upstream",
  "patterns": [
    {
      "id": "tracking-spec",
      "name": "Tracking Spec Pattern",
      "description": "Lightweight spec for work with no active build tasks -- MRs under review, issues awaiting response. One or two tasks using waiting status with waiter objects.",
      "transferability": "Any project tracking external work that blocks on human review",
      "first_observed": "2026-03-27",
      "observed_in": ["harness/estate-campaign-contract"]
    }
  ]
}
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

1. Add a task with `type: "retro"` and `depends_on` referencing the final task(s)
2. Write the `arc` field: what we set out to do vs what happened (2-3 sentences)
3. Write `transforms`: the key moves, each with a label, description, and
   transferability flag
4. Reference patterns in the tree's `patterns.json` (create new ones if needed)
5. Set status to `done` and `completed` date

### Replay Model

To replay a sub-tree from project A onto project B, the agent reads:

1. The task DAG from A's `state.json` -- the structural shape of the work
2. The retro node's transforms -- the moves and rationale
3. The patterns referenced -- which named approaches apply
4. The `landscape.json` from A -- what stakeholder constraints shaped choices

The agent generates a new spec for B by interpreting the transforms against
B's constraints and stakeholder landscape. Replay is interpreted, not directly
applied.

---

## archive.json Extension (v5)

Archive entries gain optional fields for queryable retrospective data:

```json
{
  "id": "estate-campaign-contract",
  "path": "specs/harness/archive/estate-campaign-contract/state.json",
  "summary": "Three-layer contract formalization",
  "archived": "2026-03-27",
  "reason": "completed",
  "outcome": "Three-layer contract, waiting status, tracking spec template",
  "duration_days": 1,
  "patterns": ["tracking-spec", "three-layer-contract"],
  "contributions": ["contributions/2026-03-27-synthesist-v5.md"]
}
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
1. Discuss    Human describes intent. Plan agent asks questions.
              Output: shared understanding.

2. Plan       Plan agent writes spec.md + state.json.
              Human reviews, iterates.
              Status: draft → review → approved

3. Build      Build agent executes tasks in dependency order.
              Delegates to @edit, @test as needed.
              Status: approved → in-progress

4. Review     @review agent reviews completed milestone.
              Writes quality score to state.json.

5. Verify     @verify agent runs all acceptance criteria.
              Resets failed tasks to pending.

6. Complete   All tasks done, all criteria pass, quality scored.
              Status: in-progress → done
```

---

## Context Trees

Specs are organized into **context trees** — top-level directories under `specs/`
that scope work to a single domain. Each tree is a self-contained unit of context
that an agent can load without seeing the entire estate.

### Tree Structure

```
specs/
├── estate.json                    ← meta-switchboard (all trees + session state)
├── SPEC_FORMAT.md                 ← this file
├── <tree-name>/                   ← one context tree
│   ├── campaign.json              ← active + backlog specs
│   ├── archive.json               ← archived specs (done, abandoned, deferred)
│   ├── decisions.md               ← optional: locked decisions for this domain
│   ├── <spec>/                    ← a spec directory
│   │   ├── spec.md
│   │   ├── state.json
│   │   └── discovery.md
│   ├── <phase>/<spec>/            ← optional: specs grouped by phase
│   │   ├── spec.md
│   │   └── state.json
│   └── archive/                   ← archived spec directories
│       └── <old-spec>/
└── _template/                     ← spec templates
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

## estate.json — Meta-Switchboard

The estate file is the top-level entry point for the agent. It lists all context
trees, their status, and session state. The agent reads this first at session start.

### Schema

```json
{
  "version": 4,
  "trees": {
    "<tree-name>": {
      "path": "specs/<tree-name>/campaign.json",
      "status": "active | dormant | archived",
      "description": "One-line description of this tree's domain"
    }
  },
  "active_threads": [
    {
      "id": "<tree>/<spec-or-account>",
      "tree": "<tree-name>",
      "spec": "spec-id or null",
      "task": "task-id or null",
      "date": "YYYY-MM-DD",
      "summary": "1-2 sentence current state and what's next"
    }
  ]
}
```

#### Thread Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Unique key: `{tree}/{spec}` or `{tree}/{account}` for account work. Must be stable across sessions. |
| `tree` | string | yes | Which tree this thread lives in |
| `spec` | string | no | Active spec ID, or null for tree-level work (research, cadence) |
| `task` | string | no | Active task ID within the spec, or null |
| `date` | string | yes | Last update date (YYYY-MM-DD) |
| `summary` | string | yes | Current state and what's next |

#### Thread Lifecycle

- A thread is **created** when a session starts work on a new spec or account
- A thread is **updated** when a session makes progress (date, task, summary change)
- A thread is **pruned** when it is older than 7 days AND has no active spec/task
- Threads with an active spec or task are never pruned regardless of age

### Session Entry Protocol

1. Read `specs/estate.json`
2. Display all `active_threads` sorted by date (most recent first)
3. Offer to continue any thread, or start a new one
4. If the human names a thread, load that tree's `campaign.json` and follow spec/task pointers

---

## campaign.json — Tree-Level Coordination

Each context tree has a campaign.json that tracks active and backlog specs.
This replaces the v2 flat campaign format.

### Schema

```json
{
  "tree": "<tree-name>",
  "description": "One-line description of this tree's domain",
  "active": [
    {
      "id": "spec-slug",
      "path": "specs/<tree>/<spec>/state.json",
      "summary": "One-line description of current work",
      "phase": "phase-name or null",
      "blocked_by": []
    }
  ],
  "backlog": [
    {
      "id": "future-spec-slug",
      "title": "Human-readable title",
      "summary": "One-line description of intent",
      "blocked_by": [],
      "path": null
    }
  ]
}
```

### Campaign Rules

- A spec moves from `backlog` → `active` when work begins
- A spec moves from `active` → archive.json when archived
- `blocked_by` references spec IDs within the same tree, or cross-tree refs
  in the format `<tree>/<spec-id>`
- Backlog items may be stubs (no spec.md yet) or full specs

---

## archive.json — Archived Specs

Each context tree has an archive.json that records specs removed from active work.
Archive is append-only. Spec directories move to the `archive/` subdirectory.

### Schema

```json
{
  "tree": "<tree-name>",
  "archived": [
    {
      "id": "spec-slug",
      "path": "specs/<tree>/archive/<spec>/state.json",
      "summary": "One-line description",
      "archived": "YYYY-MM-DD",
      "reason": "completed | abandoned | superseded | deferred",
      "outcome": "Optional: what was the result"
    }
  ]
}
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
- Add an entry to archive.json with the reason
- Update any cross-references that point to the archived spec (or the
  integrity tool will flag them)
- Archive is append-only — never delete entries

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
The integrity tool resolves references by checking campaign.json first, then
archive.json. References to archived specs are valid — they just point to
the archive/ directory.

---

## Session State

Session state lives in `specs/estate.json` as an `active_threads` array. Each
thread represents an active workstream -- a spec being built, an account being
researched, a contribution being tracked. Multiple sessions can run concurrently
without overwriting each other's state.

### Session Entry Protocol

1. Read `specs/estate.json`
2. Display all `active_threads` sorted by date (most recent first)
3. Offer to continue any thread, or start a new one
4. If the human names a thread, load that tree's `campaign.json` and follow
   the spec/task pointers

### Session End Protocol

Before the session ends (or when context is getting heavy), update estate.json:
- Find this session's thread in `active_threads` by `id`
- If found, update `date`, `summary`, `task` (and `spec` if changed)
- If not found, append a new thread entry
- Prune threads older than 7 days with no active spec/task
- Pending decisions go into the relevant spec's `<discovery>` section or as
  backlog items in the appropriate campaign.json -- not into a separate file

### Concurrent Sessions

Multiple sessions may run simultaneously. Each session manages its own thread
entry in `active_threads`, identified by the `id` field. Sessions update only
their own thread -- they do not modify other threads. If two sessions update
the same thread simultaneously, the last writer wins (acceptable -- same thread
means same workstream).

# Synthesist Specification Format

Every feature gets two files in `specs/<feature>/`:

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
| `status` | enum | yes | `pending` \| `in_progress` \| `done` \| `blocked` |
| `gate` | string\|null | no | `"human"` = requires human approval before starting. `null` = no gate |
| `acceptance` | object[] | yes | At least one acceptance criterion with a verify command |
| `failure_note` | string\|null | no | Set by @verify when a criterion fails. Cleared when task restarts |
| `owner` | string\|null | no | Identifies which session/agent owns an in_progress task. Set when claiming, cleared on completion. |

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

### Status Transitions

```
pending ──► in_progress ──► done
  ▲                          │
  └──────────────────────────┘
        (verify failure)

pending ──► blocked
              │
              └──► pending (when blocker resolves)
```

Only the verify agent may transition `done → pending` (on verification failure).
Only the build agent may transition `pending → in_progress → done`.
Human gates: the build agent sets status to `blocked` and waits for human input,
then the human (or build agent after approval) sets it back to `pending`.

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

## campaign.json — Cross-Spec Coordination

For projects with multiple related specs, a campaign file tracks cross-spec
dependencies and temporal intent. Lives at `specs/<campaign>/campaign.json`.

### Schema

```json
{
  "campaign": "campaign-name",
  "done": [
    {
      "id": "spec-slug",
      "path": "specs/campaign-name/spec-slug/state.json",
      "summary": "One-line description of what was shipped"
    }
  ],
  "active": [
    {
      "id": "spec-slug",
      "path": "specs/campaign-name/spec-slug/state.json",
      "summary": "One-line description of current work",
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

### Three Horizons

| Horizon | Meaning | Spec required? |
|---------|---------|----------------|
| `done` | Completed specs — institutional memory of what shipped | Yes (path to state.json) |
| `active` | Specs currently being worked | Yes (path to state.json) |
| `backlog` | Ideas modeled but not yet started | No (path is null until spec is created) |

Backlog items may be full spec stubs (with spec.md + state.json) or lightweight
entries with just an id, title, and summary. They graduate to `active` when work
begins and a spec is written.

### Campaign Rules

- A spec moves from `backlog` → `active` when work begins
- A spec moves from `active` → `done` when all tasks pass verification
- `blocked_by` references other spec IDs within the same campaign
- Multiple campaigns may exist in the same project (e.g., one per workstream)

---

## session.md — Session Handoff

The session handoff file lives at `memory/session.md`. It is a lightweight pointer
that gets the agent into spec context at the start of a new session.

### Structure

```markdown
# Session Handoff

## Last Session
Date: YYYY-MM-DD
Summary: 2-3 lines describing what was accomplished.

## Active Work
- Campaign: specs/<campaign>/campaign.json
- Spec: specs/<campaign>/<spec>/state.json — current task status
- Spec: specs/<campaign>/<spec>/state.json — current task status

## Pending Decisions
- Decision not yet captured in any spec (if any)
```

### Session Handoff Rules

- **Read first, ask second.** At session start, read session.md, follow the links
  to campaign.json and active state.json files. Do not ask the human to re-explain
  what's already in the specs.
- **Keep it short.** The specs are the source of truth. session.md is a pointer,
  not a parallel document.
- **Update at session end.** Before context fills or the session ends, update
  session.md with current state.
- **No duplication.** If information is in a spec, link to it. If it's not in a
  spec yet, either write it to a spec or note it as a pending decision.

# Synthesist — Agent Instructions

Spec-driven multi-agent orchestration framework for OpenCode. One primary agent
handles the full loop: discuss → draft → iterate → codify → build → record.

---

## Workflow: Discuss → Draft → Iterate → Codify → Build → Verify

All feature work follows this loop. The full protocol is in `prompts/framework.md`.

### Key Principles

1. **The spec is the shared workspace.** Write to disk immediately. Don't hold drafts in context.
2. **Executable acceptance criteria on every task.** If you can't write a verify command, the task is too vague.
3. **Each task fits in one context window.** ≤5 files, ≤500 word description.
4. **Trust nothing, verify everything.** The @verify agent runs acceptance criteria; it doesn't trust self-reports.
5. **Findings persist to disk before context dies.** If it matters, it's in discovery.md.
6. **Human gates at high-stakes transitions.** External writes, auth changes, and visual reviews always gate.

---

## Agent Roles

| Agent | Mode | Model | Access | Steps | Role |
|-------|------|-------|--------|-------|------|
| `primary` | primary | Opus | full | — | Discuss, plan, build — the full loop |
| `@explore` | subagent | Sonnet | read-only | 5 | Fast codebase search with countdown |
| `@edit` | subagent | Sonnet | write+edit+bash | 15 | Targeted file changes from spec tasks |
| `@review` | subagent | GPT (cross-model) | read-only | 15 | Cross-model code review |
| `@verify` | subagent | Haiku | full | 20 | Acceptance criteria verification |

Model IDs are defaults. Instances override for their provider (e.g., `gitlab/duo-chat-*`
for GitLab Duo, `anthropic/claude-*` for Anthropic direct).

### Subagent Delegation

The primary agent delegates to subagents for specific tasks:

| Subagent | When to use |
|----------|-------------|
| `@edit` | Targeted file changes for a specific task |
| `@review` | Cross-model review after completing a milestone |
| `@verify` | Run acceptance criteria for completed tasks |
| `@explore` | Search the target codebase for patterns or context |

---

## Concurrent Session Safety

Multiple sessions may run against the same spec tree. The framework protects
against conflicts:

1. **Commit after every task completion.** State changes are committed immediately,
   not batched. This ensures other sessions see current state.
2. **Task ownership.** The `owner` field in state.json identifies which session
   claimed a task. Check before claiming. Clear on completion.
3. **Pull before starting.** Read state.json from disk, not from context cache.
   Another session may have completed dependencies.
4. **Respect the DAG.** Dependencies must be "done" regardless of which session
   completed them.

### Layer 2: Autonomous Execution

The concurrent safety rules also enable a more powerful pattern:

- **Plan now, execute later.** The human shares context and drives planning in one
  session. Specs and state.json capture everything. A later session (or multiple
  sessions) can pick up tasks and execute without the human present.
- **Codify everything.** For autonomous execution to work, all context must be in
  specs — not in chat history. Every decision, constraint, and implementation note
  must be written to spec.md, state.json, or discovery.md before the planning
  session ends.

---

## Campaign Coordination

For projects with multiple related specs, campaigns track cross-spec dependencies
and temporal intent. See `specs/SPEC_FORMAT.md` for the campaign.json schema.

Three horizons:
- **done** — completed specs (what we shipped)
- **active** — specs currently being worked
- **backlog** — ideas modeled but not yet started

Campaign state lives at `specs/<campaign>/campaign.json`.

---

## Session Handoff

Sessions are finite. The `memory/session.md` file provides structured handoff
between sessions. See `specs/SPEC_FORMAT.md` for the session.md format.

Key rules:
- Read session.md first at session start — follow links to specs
- Update session.md at session end — keep it current
- Don't duplicate spec content — link to it

---

## Staging Directory

All chunked file writes use `staging/` as a temporary workspace. This directory
is gitignored and exists in the project root.

Rules:
- staging/ is never a final destination — always mv to the target path
- Every staged file must reach its destination before a task is marked done
- Abandoned files in staging/ indicate incomplete work

---

## Prompt Architecture

The primary agent prompt is composed from two files:

| File | Owner | Content |
|------|-------|---------|
| `prompts/framework.md` | Synthesist framework | Loop, write rules, explore rules, context rules, spec rules, subagent rules, concurrency rules, session rules |
| `prompts/instance.md` | Project instance | Identity, skill decision tree, estate structure, project-specific overrides |

Framework updates flow without merge conflicts on instance-specific content.

---

## Recommended Global Config

The global `~/.config/opencode/` layer should contain only personal preferences
that apply across all projects:

- **AGENTS.md**: External write approval rules, git practices, epistemic honesty
- **opencode.json**: Provider credentials, default model preferences
- **skills/**: Cross-project personal utilities only

Behavioral instructions belong in the project, not in global config. Global config
should not contain workflow rules, skill inventories, or estate-specific instructions.

---

## Principles

1. **Markdown for human intent, JSON for machine state.**
2. **Executable acceptance criteria on every task.**
3. **Each task fits in one context window.**
4. **Trust nothing, verify everything.**
5. **Discuss before draft, draft before build.**
6. **Findings persist to disk before context dies.**
7. **Human gates at high-stakes transitions.**

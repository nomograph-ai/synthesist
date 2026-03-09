# Synthesist — Agent Instructions

## Workflow: Discuss → Plan → Build → Verify

All feature work follows this loop. Skipping steps produces drift.

### 1. Discuss (Human + Plan Agent)

Before any spec exists, the human describes intent and the plan agent asks clarifying
questions. The goal is to surface ambiguity, capture preferences, and prevent the
reasonable-but-wrong defaults that agents choose when left to infer.

Output: shared understanding, not artifacts.

### 2. Plan (Plan Agent)

The plan agent reads the target codebase, writes `specs/<feature>/spec.md` with
XML-tagged sections, and creates `specs/<feature>/state.json` with the task DAG.

<rules type="MUST" section="plan">
  <rule n="1">MUST read the target codebase before writing a spec</rule>
  <rule n="2">MUST write executable verify commands for every task acceptance criterion</rule>
  <rule n="3">MUST size each task to fit in one agent context window</rule>
  <rule n="4">MUST mark high-stakes tasks with "gate": "human"</rule>
  <rule n="5">MUST write discovery findings to specs/<feature>/discovery.md before context dies</rule>
</rules>

### 3. Build (Build Agent + Subagents)

The build agent reads the spec, works through tasks in dependency order, and updates
state.json as it goes. It may delegate to subagents:

| Subagent | When to use |
|----------|-------------|
| `@edit` | Targeted file changes for a specific task |
| `@test` | Generate and run tests after implementation |
| `@review` | Cross-model review after completing a milestone |
| `@explore` | Search the target codebase for patterns or context |

<rules type="MUST" section="build">
  <rule n="1">MUST read state.json before starting work — pick the first task where all depends_on are "done" and status is "pending"</rule>
  <rule n="2">MUST set task status to "in_progress" before starting work</rule>
  <rule n="3">MUST run verify commands after completing a task — do not trust self-assessment</rule>
  <rule n="4">MUST set task status to "done" only after verify commands pass</rule>
  <rule n="5">MUST stop and surface to human when encountering a task with "gate": "human"</rule>
  <rule n="6">MUST persist findings to discovery.md when learning something that affects future tasks</rule>
</rules>

### 4. Verify (Verify Agent)

The verify agent is the trust-nothing layer. After a set of tasks is marked "done",
invoke `@verify` with the spec path. It reads state.json, runs every verify command
for every "done" task, and reports pass/fail. It does not trust the build agent's
self-reports.

If a verify command fails, the verify agent sets the task status back to "pending"
and appends a failure note.

---

## Spec Format

All specifications follow the format defined in `specs/SPEC_FORMAT.md`.

Two files per feature:

| File | Owner | Purpose |
|------|-------|---------|
| `spec.md` | Human / Plan agent | Intent, context, constraints, decisions |
| `state.json` | Build agent / Verify agent | Task DAG, status, acceptance criteria |

The separation is intentional: spec.md is for reasoning (what and why), state.json
is for execution (what to do next and whether it worked).

---

## Agent Roles

| Agent | Mode | Access | Role |
|-------|------|--------|------|
| `plan` | primary | read-only | Spec writing, architecture, codebase analysis |
| `build` | primary | full | Implementation, task execution, state updates |
| `@explore` | subagent | read-only | Fast codebase search |
| `@edit` | subagent | write, edit | Targeted file changes from spec tasks |
| `@review` | subagent | read-only | Cross-model code review |
| `@verify` | subagent | write, bash | Acceptance criteria verification |
| `@test` | subagent | write, bash | Test generation and execution |

---

## Coordination Patterns

### Findings Survive Context Death

Any agent that discovers something relevant to future work MUST write it to
`specs/<feature>/discovery.md` before its context window fills or session ends.
This is the append-only institutional memory.

### Human Gates

Tasks with `"gate": "human"` in state.json require explicit human approval before
the build agent may proceed. The build agent MUST stop, present a summary of what
it plans to do, and wait.

### Quality Scoring

After `@review` completes, the review agent writes a quality assessment to
state.json's `quality` field: a score (0.0–1.0) and validation entries recording
who reviewed and what they found.

### Cross-Model Diversity

The `@review` agent should use a different model family than the plan and build
agents. Different model families catch different failure modes. A review that passes
both is more reliable than one that passes either alone.

---

## Principles

1. **Markdown for human intent, JSON for machine state.** Agents reason over prose;
   they execute from structured data.
2. **Executable acceptance criteria on every task.** If you can't write a verify
   command, the task is underspecified.
3. **Each task fits in one context window.** If it doesn't, decompose further.
4. **Trust nothing, verify everything.** The verify agent runs tests itself.
5. **Discuss before plan, plan before build.** Skipping the discuss phase produces
   specs that encode wrong assumptions.
6. **Findings persist to disk before context dies.** Discovery.md is the institutional
   memory.
7. **Human gates at high-stakes transitions.** Agents stop and ask when the cost of
   being wrong is high.

# Synthesist Workflow State Machine

The LLM is the complete mediation layer between the human and synthesist.
The human never calls synthesist directly. This state machine defines
the phases of LLM-human interaction and what operations are allowed in
each phase.

## LLM Behavioral Contract

You MUST follow these rules when mediating between the human and synthesist.
The human never calls synthesist directly — you are the complete interface.

### Session Start Sequence

Every session begins with this exact sequence:
1. `synthesist session start <descriptive-name>` — create your session branch
2. `synthesist phase set orient` — declare you are in ORIENT
3. `synthesist status` — read the estate
4. Present the status to the human in plain language (see Display Rules)
5. Ask the human what they want to focus on

### Display Rules

1. When showing task trees, ALWAYS use grouped tables. Example format:

   **Session Store Layer**

   | ID | Task | Deps |
   |----|------|------|
   | s1 | Add branch operations to store.go | — |
   | s2 | Add EnsureSession for auto-checkout | s1 |
   | s3 | Commit skips GitCommit on session branches | s2 |

   Never show raw JSON to the human. Process all synthesist output
   into this format before presenting.

2. When the task tree changes, show a diff table BEFORE the current state:

   **Changes**

   | Action | ID | Description |
   |--------|----|-------------|
   | added | s4 | Wire --session flag into main.go |
   | cancelled | s1 | Replaced by s34 with correct deps |

3. Cancelled tasks: summarize as count (e.g., "27 cancelled, hidden").
   Show cancellation reasons only when reviewing history.

4. When entering ORIENT, present status as plain language:
   "3 trees active. 22 tasks pending across 4 specs. Ready tasks:
   gkg-bench/t16 (run analysis), packaging/t7 (GitLab release)."
   NOT the raw JSON from `synthesist status`.

5. Always use `--active` flag when running `task list` to hide cancelled
   tasks by default.

### Phase Rules

1. After every phase transition, run `synthesist phase set <phase>`.
2. You cannot claim tasks in PLAN. You cannot create tasks in EXECUTE.
3. PLAN → EXECUTE must pass through AGREE.
4. AGREE means: present the full plan in grouped table format, state
   assumptions, identify decision points, and WAIT for explicit human
   approval. "Ready to proceed?" followed by proceeding is NOT approval.
   The human must say "yes", "proceed", "approved", or equivalent.
5. After each task in EXECUTE, enter REFLECT: assess if the plan holds.
6. If the plan changes, enter REPLAN, show the diff, then go to AGREE.

### Pre-Execution Protocol (in AGREE phase)

Present to the human:
1. The full task tree in grouped table format
2. Your assumptions
3. Scope: what files/repos will be touched
4. Which tasks are autonomous vs need human input
5. What "done" looks like

### Error Protocol

1. Never retry a failed approach silently — explain what failed.
2. When a task is harder than expected, remodel it (REPLAN → AGREE).
3. Record all discoveries in synthesist, not just in chat.
   Chat context dies; synthesist persists.

## Phases

```
ORIENT → PLAN → AGREE → EXECUTE ↔ REFLECT → REPORT
                  ↑                    |
                  └──── REPLAN ←───────┘
```

### ORIENT

**Purpose:** Build a shared mental model of where things stand.

**Trigger:** Session start, or human asks "where are we?"

**Allowed operations:**
- `synthesist status` — read estate overview
- `synthesist task list` — read task trees
- `synthesist spec show` — read spec details
- `synthesist discovery list` — read past findings
- `synthesist session list` — show active sessions

**Allowed LLM actions:**
- Present current state as grouped tables with descriptions
- Summarize what happened in previous sessions
- Identify ready tasks across all specs
- Ask the human what they want to focus on

**Forbidden:**
- Claiming tasks
- Creating specs or tasks
- Modifying any data

**Transition → PLAN:** Human indicates what they want to work on.

### PLAN

**Purpose:** Model the work before doing it.

**Trigger:** Human says what they want to accomplish.

**Allowed operations:**
- `synthesist spec create/update` — model intent
- `synthesist task create` — decompose work
- `synthesist discovery add` — record findings from research
- All ORIENT operations (reads)

**Allowed LLM actions:**
- Research to inform the plan (read files, search, web fetch)
- Create/modify specs and tasks
- Show the plan as grouped tables after each modification
- Show diffs when the plan changes
- State assumptions explicitly
- Identify which tasks are autonomous vs need human input
- Estimate scope and blast radius

**Forbidden:**
- Claiming tasks
- Writing code
- Modifying files outside synthesist
- Running builds or tests (except for research)

**Transition → AGREE:** LLM presents the complete plan and explicitly
asks for concurrence. This is NOT "ready to proceed?" followed by
proceeding. The LLM must WAIT for an explicit human approval.

### AGREE

**Purpose:** Explicit human checkpoint before execution begins.

**Trigger:** LLM has finished planning and presents the full plan.

**Allowed operations:** None. The LLM presents and waits.

**LLM must present:**
1. The full task tree in grouped table format
2. Assumptions being made
3. Scope: what files/repos will be touched
4. Decision points: which tasks need human input during execution
5. What "done" looks like

**Transition → EXECUTE:** Human explicitly approves (e.g., "proceed",
"go ahead", "approved", "yes").

**Transition → PLAN:** Human requests changes. LLM returns to PLAN,
modifies, and comes back to AGREE.

### EXECUTE

**Purpose:** Claim and complete tasks in dependency order.

**Trigger:** Human approves the plan in AGREE.

**Allowed operations:**
- `synthesist task claim` — take ownership of a task
- `synthesist task done` — complete a task (runs acceptance)
- `synthesist task block` — block a task with reason
- `synthesist discovery add` — record findings
- All ORIENT operations (reads)
- File modifications, builds, tests — scoped to the current task

**Allowed LLM actions:**
- Work on ONE task at a time
- Commit after each task completion
- Run `make build && make test && make lint` after each commit

**Forbidden:**
- Creating new tasks (that's REPLAN)
- Modifying the task tree structure
- Working on tasks not in the agreed plan
- Skipping human-gated tasks without approval

**Transition → REFLECT:** After completing each task.

### REFLECT

**Purpose:** Assess whether the plan still holds after completing a task.

Inspired by Reflexion (Shinn et al., 2023) — self-assessment after
execution prevents blind continuation on a broken plan.

**Trigger:** A task completes (done or blocked).

**LLM must assess:**
1. Did the task succeed? If not, what went wrong?
2. Did we learn something that changes the plan?
3. Is the next task still the right thing to do?
4. Should any findings be recorded as discoveries?

**Transition → EXECUTE:** Plan still holds, pick up next task.

**Transition → REPLAN:** Something changed that invalidates the plan.

**Transition → REPORT:** All tasks in the plan are complete.

### REPLAN

**Purpose:** Modify the plan based on what was learned during execution.

Inspired by SOAR's impasse model — when the current plan can't proceed,
create a subgoal to resolve the blocker rather than retrying blindly.

**Trigger:** REFLECT identifies that the plan needs to change.

**Allowed operations:**
- `synthesist task create` — add new tasks
- `synthesist task cancel` — remove tasks that are no longer needed
- `synthesist spec update` — update decisions/constraints
- `synthesist discovery add` — record what triggered the replan

**LLM must:**
1. Show what changed (diff format: added/cancelled/rewired tasks)
2. Explain WHY the plan changed
3. Show the updated plan in grouped table format

**Transition → AGREE:** REPLAN always goes back through AGREE. The human
must approve the modified plan before execution resumes. This is the
critical safety property — the human always has a checkpoint before
the LLM resumes autonomous work.

### REPORT

**Purpose:** Summarize what was accomplished and what's next.

**Trigger:** All tasks in the agreed plan are complete, or the human
asks for a status update.

**LLM must present:**
1. What was completed (grouped table with done status)
2. What was discovered (new findings)
3. What's next (ready tasks in other specs, or new work to plan)
4. Any decisions that need human input

**Transition → ORIENT:** New work cycle begins.

## Display Conventions

These apply across ALL phases:

1. **Task trees:** Always presented as grouped tables with phase header,
   ID column, description column, and dependency column. Never raw JSON.

2. **Task changes:** When the task tree is modified, show a diff table
   (action: added/cancelled/rewired, ID, description) BEFORE showing
   the current state.

3. **Cancelled tasks:** Summarized as a count at the bottom of the task
   tree display. Never listed individually unless specifically asked.

4. **Status:** When entering ORIENT, present synthesist status as plain
   language with ready tasks listed per spec, not as raw command output.

## CLI Enforcement

The `synthesist phase` command allows the LLM to declare its current
phase. Synthesist validates that the operations being attempted are
allowed in that phase:

- `synthesist phase orient` — only reads allowed
- `synthesist phase plan` — reads + spec/task creation
- `synthesist phase execute` — reads + task claim/done + file ops
- `synthesist phase report` — reads only

The phase is advisory — an experienced LLM can override it with
`--force`. But the default behavior is to error on invalid operations
for the current phase.

## Sources

- **SOAR** (Laird, Newell, Rosenbloom) — impasse → subgoal model for
  REPLAN phase
- **Reflexion** (Shinn et al., 2023) — self-assessment after execution
  for REFLECT phase
- **LangGraph** — checkpoint model for AGREE phase
- **OODA** (Boyd) — ORIENT as mental model building, not status display
- **ACT-R** — per-phase production rules (allowed actions)
- **Beads/Gastown** (Yegge, 2026) — task DAG as persistence layer

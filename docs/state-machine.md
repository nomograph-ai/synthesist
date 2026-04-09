## Behavioral Contract

This section defines your behavioral contract. Follow it exactly.

## Session Management

Every write command requires `--session=<name>`. Shell state does not
persist between Bash calls, so include `--session=<name>` on every
write command. Reads within a session see the session's data.

Pattern: `export SYNTHESIST_SESSION=my-session` then
`synthesist --force task add ...`

## Concurrent Sessions

Multiple agents can work in the same project simultaneously. This is
the intended production pattern, not an edge case.

**Session isolation:** Each agent starts its own session (`session start`).
Each session gets a copy of the database. Writes are invisible to other
sessions until merge.

**Zero-contention pattern:** Assign each agent a different spec. Two
agents working on different specs will never conflict on merge.

**Task claim is atomic:** `task claim` uses `UPDATE WHERE status='pending'
AND owner IS NULL`. Two agents cannot claim the same task. If they race,
one succeeds and the other gets an error -- retry with a different task.

**Merge protocol:**
1. Finish all work in your session
2. `synthesist session merge <name>` -- three-way merge to main
3. If conflicts: resolve with `--ours` (keep main) or `--theirs`
   (keep session). Conflicts only occur when two sessions modify the same
   column of the same row.
4. The session file is deleted after merge

**Session naming:** Use descriptive names that identify the agent and
work: `paper-citations`, `grammar-fixes`, `factory-agent-03`. Avoid
generic names like `session-1`.

## Session Start Sequence

1. `synthesist session start <descriptive-name>`
2. `synthesist --session=<name> --force phase set orient`
3. `synthesist session list` -- check for abandoned sessions from previous runs
4. `synthesist status` -- estate overview and ready tasks
5. For the spec you'll work on:
   - `synthesist spec show <tree/spec>` -- goal, decisions
   - `synthesist discovery list <tree/spec>` -- findings from previous sessions
   - `synthesist stance <stakeholder>` -- for each stakeholder in the tree
6. If stakeholders have recorded dispositions, present a **landscape summary**
   to the human: who are the stakeholders, what are their known positions,
   what ecosystem constraints apply. Do not skip this.
7. Present to the human in plain language (see Display Rules)
8. If human stated intent, acknowledge and transition to PLAN

## State Machine

```
ORIENT -> PLAN -> AGREE -> EXECUTE <-> REFLECT -> REPORT
                   ^                     |
                   +---- REPLAN <--------+
```

After every transition: `synthesist --session=<name> phase set <phase>`

| Phase | Purpose | Allowed | Forbidden | Transition |
|-------|---------|---------|-----------|------------|
| ORIENT | Build shared mental model. If tree has stakeholders: `stance` queries are mandatory. | Read: status, task list, spec show, discovery list, stance, session list | Any writes | -> PLAN when human indicates work. Landscape summary must be presented first. |
| PLAN | Model work before doing it | Read + spec add/update, task add, discovery add, research | Claiming tasks, writing code, modifying non-synthesist files | -> AGREE when plan is complete |
| AGREE | Human checkpoint. Plan must include ecosystem constraints from dispositions/discoveries if they exist. | Nothing -- present and wait | Everything | -> EXECUTE on explicit approval, -> PLAN on changes |
| EXECUTE | Claim and complete tasks | task claim/done/block, discovery add, file modifications scoped to current task. If blocker discovered mid-task: `task block` with reason, then -> REFLECT | Adding/cancelling tasks, modifying task tree | -> REFLECT after each task |
| REFLECT | Assess plan validity | Read + discovery add | Claiming next task before assessment | -> EXECUTE if plan holds, -> REPLAN if not, -> REPORT if done |
| REPLAN | Modify plan from execution learnings | task add/cancel/block, spec update, discovery add | Claiming tasks | -> AGREE (always -- human must re-approve) |
| REPORT | Summarize and hand off | Read + discovery add | Other writes | -> ORIENT for new cycle |

## AGREE Protocol

Present to the human before ANY execution:
1. Full task tree in grouped table format
2. Your assumptions
3. Scope: what files/repos will be touched
4. Which tasks are autonomous vs need human input
5. What "done" looks like
6. **Ecosystem constraints** -- if the spec's tree has stakeholders with
   recorded dispositions, list them:
   ```
   Ecosystem constraints (from stakeholder dispositions + discoveries):
   - stakeholder_a: prefers X over Y (documented)
   - stakeholder_b: opposed to Z (inferred)
   - Discovery f1: canonical implementation uses pattern P

   The plan accounts for these by: [task references]
   ```
   If no ecosystem constraints are listed and the tree has stakeholders,
   that is a sign ORIENT was incomplete. Go back and run `stance` queries
   before presenting the plan.

Then WAIT. "Ready to proceed?" followed by proceeding is NOT approval.
The human must explicitly say "yes", "proceed", "approved", or equivalent.

## Display Rules

1. Task trees as grouped tables. Never raw JSON. Example:

   **Session Store Layer**

   | ID | Task | Deps |
   |----|------|------|
   | s1 | Add branch operations to store.go | -- |
   | s2 | Add EnsureSession for auto-checkout | s1 |

2. Task changes as diff table BEFORE current state:

   | Action | ID | Description |
   |--------|----|-------------|
   | added | s4 | Wire --session flag into main.go |
   | cancelled | s1 | Replaced by s34 with correct deps |

3. Cancelled tasks: count only ("27 cancelled, hidden").
4. Status in ORIENT: plain language, not JSON.
5. Always `--active` flag on task list.

## Long Autonomous Execution

When the human approves a plan and steps away:
- Work through tasks in dependency order, one at a time
- At human-gated tasks: skip and continue with other ready tasks.
  Do NOT stop entirely. Record that the gated task is waiting.
- Before context fills: enter REPORT. Write findings to synthesist
  via `discovery add`. Commit all work. The next session picks up
  from synthesist state, not from chat context.
- If all remaining tasks are gated or blocked: enter REPORT with
  a summary of what's done and what's waiting for the human.

## Error Protocol

1. Never retry silently -- explain what failed.
2. When a task is harder than expected: REPLAN -> AGREE.
3. Record discoveries in synthesist, not just chat.

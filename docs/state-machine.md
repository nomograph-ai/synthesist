# Synthesist Workflow State Machine

You are the complete mediation layer between the human and synthesist.
The human never calls synthesist directly. This document defines your
behavioral contract.

## Session Management

Every write command requires `--session=<name>`. Shell state does not
persist between Bash calls, so include `--session=<name>` on every
write command. Read commands (status, check, task list, spec show)
work without a session.

Pattern: `S="synthesist --session=my-session"` then `$S task create ...`

## Session Start Sequence

1. `synthesist session start <descriptive-name>`
2. `synthesist --session=<name> phase set orient`
3. `synthesist status` + `synthesist spec show` for mentioned specs
4. Present status in plain language (see Display Rules)
5. If human stated intent, acknowledge and transition to PLAN

## State Machine

```
ORIENT → PLAN → AGREE → EXECUTE ↔ REFLECT → REPORT
                  ↑                    |
                  └──── REPLAN ←───────┘
```

After every transition: `synthesist --session=<name> phase set <phase>`

| Phase | Purpose | Allowed | Forbidden | Transition |
|-------|---------|---------|-----------|------------|
| ORIENT | Build shared mental model | Read: status, task list, spec show, discovery list, session list | Any writes | → PLAN when human indicates work |
| PLAN | Model work before doing it | Read + spec create/update, task create, discovery add, research | Claiming tasks, writing code, modifying non-synthesist files | → AGREE when plan is complete |
| AGREE | Human checkpoint | Nothing — present and wait | Everything | → EXECUTE on explicit approval, → PLAN on changes |
| EXECUTE | Claim and complete tasks | task claim/done/block, discovery add, file modifications scoped to current task. If blocker discovered mid-task: `task block` with reason, then → REFLECT | Creating/cancelling tasks, modifying task tree | → REFLECT after each task |
| REFLECT | Assess plan validity | Read + discovery add | Claiming next task before assessment | → EXECUTE if plan holds, → REPLAN if not, → REPORT if done |
| REPLAN | Modify plan from execution learnings | task create/cancel/block, spec update, discovery add | Claiming tasks | → AGREE (always — human must re-approve) |
| REPORT | Summarize and hand off | Read only | Writes | → ORIENT for new cycle |

## AGREE Protocol

Present to the human before ANY execution:
1. Full task tree in grouped table format
2. Your assumptions
3. Scope: what files/repos will be touched
4. Which tasks are autonomous vs need human input
5. What "done" looks like

Then WAIT. "Ready to proceed?" followed by proceeding is NOT approval.
The human must explicitly say "yes", "proceed", "approved", or equivalent.

## Task Types

- **Discussion tasks** ("Discuss objectives with Andrew") — completed
  during PLAN/AGREE. Do not claim during EXECUTE.
- **Implementation tasks** ("Rewrite Introduction section") — claimed
  and executed during EXECUTE.

If the existing task tree is too vague, propose new tasks in PLAN.
Do not present a 2-task plan when the work needs 10 tasks.

## Display Rules

1. Task trees as grouped tables. Never raw JSON. Example:

   **Session Store Layer**

   | ID | Task | Deps |
   |----|------|------|
   | s1 | Add branch operations to store.go | — |
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

1. Never retry silently — explain what failed.
2. When a task is harder than expected: REPLAN → AGREE.
3. Record discoveries in synthesist, not just chat.

## Sources

- SOAR — impasse → subgoal (REPLAN)
- Reflexion (Shinn 2023) — post-execution assessment (REFLECT)
- LangGraph — checkpoints (AGREE)
- OODA (Boyd) — orient as mental model (ORIENT)
- ACT-R — per-phase production rules
- Beads/Gastown (Yegge 2026) — task DAG persistence

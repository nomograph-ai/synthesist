package main

const skillContent = `# Synth -- Synthesist Specification Graph Manager

You have access to the ` + "`synth`" + ` CLI tool. Use it for ALL specification
management. Do not read or write spec data files directly. The tool
owns the Dolt database at .synth/.

## Concepts

**tree/spec format**: Commands take specs as ` + "`tree/spec`" + ` -- e.g.
` + "`upstream/bootc-install`" + `, ` + "`harness/estate-contract`" + `. The tree is the
context domain (upstream, harness, account). The spec is the work unit
within that tree. Specs are created implicitly when you add the first task.

**Stakeholders** are registered per-tree (` + "`synth stakeholder add <tree>`" + `).
They are referenced by ID across specs in that tree.

## Enums

**stance**: supportive | cautious | opposed | neutral | unknown
**confidence**: documented | verified | inferred | speculative
**signal type**: pr_comment | issue_comment | review | commit_message | chat | meeting | email | other
**influence role**: maintainer | reviewer | approver | blocker | champion | observer
**task status**: pending | in_progress | done | blocked | waiting

## When to use synth

- Starting a session: ` + "`synth status`" + ` to see estate overview
- Planning work: ` + "`synth task create`" + ` to add tasks to a spec
- Executing work: ` + "`synth task claim`" + ` then ` + "`synth task done`" + ` when verified
- External blockers: ` + "`synth task wait`" + ` with a check command
- Tracking people: ` + "`synth stakeholder add`" + ` and ` + "`synth disposition add`" + `
- Recording evidence: ` + "`synth signal record`" + ` for observable stakeholder actions
- Completing a spec: ` + "`synth retro create`" + ` with arc and transforms
- Checking health: ` + "`synth check`" + ` validates everything
- Replaying work: ` + "`synth replay <spec>`" + ` to get a playbook for adaptation

## Output

All output is JSON. Parse it directly. Do not ask the human to
interpret synth output for you. Empty collections are ` + "`[]`" + `, never null.

## Rules

1. Never write spec data files directly. Always use synth commands.
2. Run ` + "`synth status`" + ` at session start to see active threads.
3. Run ` + "`synth task ready <tree/spec>`" + ` to find what to work on next.
4. After completing a task, run ` + "`synth task done`" + ` -- it verifies
   acceptance criteria automatically. Do not self-report completion.
5. When you encounter a stakeholder whose technical preferences
   constrain your implementation choices, record them:
   ` + "`synth stakeholder add`" + ` then ` + "`synth disposition add`" + `.
6. When a stakeholder says something that reveals their stance on a
   technical direction, record it: ` + "`synth signal record`" + `.
7. When a body of work completes, create a retro node with transforms
   before archiving: ` + "`synth retro create`" + `.
8. The tool auto-commits by default. Use ` + "`--no-commit`" + ` to batch
   multiple changes without committing each one.

## Core commands

` + "```" + `
synth status                          # estate overview + ready tasks
synth task create <tree/spec> <summary>  [--depends-on t1,t2] [--gate human] [--files f1,f2]
synth task list <tree/spec>              # all tasks with status
synth task claim <tree/spec> <id>        # set owner + in_progress
synth task done <tree/spec> <id>         # verify acceptance + complete
synth task wait <tree/spec> <id> --reason "..." --external "url" --check "cmd"
synth task ready <tree/spec>             # unblocked pending tasks

synth stakeholder add <tree> <id> --context "role" [--name "Full Name"] [--orgs "org1,org2"]
synth stakeholder list <tree>
synth disposition add <tree/spec> <stakeholder> --topic "..." --stance cautious --confidence inferred [--preferred "..."]
synth disposition list <tree/spec>
synth disposition supersede <tree/spec> <id> --new-stance supportive [--evidence <signal-id>]
synth signal record <tree/spec> <stakeholder> --source "url" --type pr_comment --content "..." [--date YYYY-MM-DD] [--our-action "..."] [--interpretation "..."]
synth signal list <tree/spec>

synth retro create <tree/spec> --arc "..." --depends-on t8[,t9]
synth retro transform <tree/spec> --label "..." --description "..." [--transferable]
synth retro show <tree/spec>
synth pattern register <tree> <id> --name "..." --description "..." [--transferability "..."] [--observed-in spec1,spec2]
synth pattern list <tree>

synth landscape show <tree/spec>      # full stakeholder graph for a spec
synth stance <stakeholder>            # current dispositions across tree
synth stance <stakeholder> <topic>    # disposition history for person + topic
synth replay <tree/spec>              # playbook: DAG + transforms + patterns + landscape
synth check                           # validate referential integrity
` + "```" + `

## Replay

` + "`synth replay`" + ` outputs the task DAG shape, retro transforms, patterns, and
landscape from a completed spec. Use this to adapt a body of work for a
new context: read the transforms (what moves were made and why), check
the landscape (what stakeholder constraints shaped the choices), then
create a new spec in the target tree with adapted tasks.
`

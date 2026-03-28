package main

const skillContent = `# Synthesist -- Specification Graph Manager

You have access to the ` + "`synthesist`" + ` CLI tool. Use it for ALL specification
management. Do not read or write spec data files directly. The tool
owns the Dolt database at .synth/.

## Concepts

**tree/spec format**: Commands take specs as ` + "`tree/spec`" + ` -- e.g.
` + "`upstream/auth-service`" + `, ` + "`harness/site-redesign`" + `. The tree is the
context domain (upstream, harness, account). The spec is the work unit
within that tree. Specs can be created explicitly with ` + "`synthesist spec create`" + `
to capture intent (goal, constraints, decisions), or implicitly when you
add the first task.

**Propagation chains** link specs so that when a source spec's output changes,
downstream specs are flagged as potentially stale. Use ` + "`synthesist propagation`" + `
to manage these cross-spec data dependencies.

**Stakeholders** are registered per-tree (` + "`synthesist stakeholder add <tree>`" + `).
They are referenced by ID across specs in that tree.

## Enums

**stance**: supportive | cautious | opposed | neutral | unknown
**confidence**: documented | verified | inferred | speculative
**signal type**: pr_comment | issue_comment | review | commit_message | chat | meeting | email | other
**influence role**: maintainer | reviewer | approver | blocker | champion | observer
**direction status**: committed | proposed | experimental | rejected
**task status**: pending | in_progress | done | blocked | waiting | cancelled

## When to use synthesist

- Starting a session: ` + "`synthesist status`" + ` to see estate overview
- Planning work: ` + "`synthesist task create`" + ` to add tasks to a spec
- Executing work: ` + "`synthesist task claim`" + ` then ` + "`synthesist task done`" + ` when verified
- External blockers: ` + "`synthesist task wait`" + ` with a check command
- Tracking people: ` + "`synthesist stakeholder add`" + ` and ` + "`synthesist disposition add`" + `
- Recording evidence: ` + "`synthesist signal record`" + ` for observable stakeholder actions
- Completing a spec: ` + "`synthesist retro create`" + ` with arc and transforms
- Checking health: ` + "`synthesist check`" + ` validates everything
- Replaying work: ` + "`synthesist replay <spec>`" + ` to get a playbook for adaptation

## Output

All output is JSON. Parse it directly. Do not ask the human to
interpret synthesist output for you. Empty collections are ` + "`[]`" + `, never null.

## Rules

1. Never write spec data files directly. Always use synthesist commands.
2. Run ` + "`synthesist status`" + ` at session start to see active threads.
3. Run ` + "`synthesist task ready <tree/spec>`" + ` to find what to work on next.
4. After completing a task, run ` + "`synthesist task done`" + ` -- it verifies
   acceptance criteria automatically. Do not self-report completion.
5. When you encounter a stakeholder whose technical preferences
   constrain your implementation choices, record them:
   ` + "`synthesist stakeholder add`" + ` then ` + "`synthesist disposition add`" + `.
6. When a stakeholder says something that reveals their stance on a
   technical direction, record it: ` + "`synthesist signal record`" + `.
7. When a body of work completes, create a retro node with transforms
   before archiving: ` + "`synthesist retro create`" + `.
8. The tool auto-commits by default. Use ` + "`--no-commit`" + ` to batch
   multiple changes without committing each one.

## Core commands

` + "```" + `
synthesist status                          # estate overview + ready tasks
synthesist spec create <tree/spec> --goal "..." [--constraints "..."] [--decisions "..."]
synthesist spec show <tree/spec>             # spec intent + task summary + propagation
synthesist spec update <tree/spec> [--goal "..."] [--constraints "..."] [--decisions "..."]

synthesist propagation add <source-tree/spec> <target-tree/spec> --seq N [--description "..."]
synthesist propagation list <tree/spec>      # upstream and downstream links
synthesist propagation check <tree/spec>     # find stale downstream specs

synthesist tree create <name> [--description "..."] [--status active]
synthesist tree list
synthesist thread create <id> --tree <tree> --summary "..." [--spec id] [--task id] [--date YYYY-MM-DD]
synthesist thread list

synthesist task create <tree/spec> <summary>  [--depends-on t1,t2] [--gate human] [--files f1,f2] [--status pending] [--id t1] [--created YYYY-MM-DD] [--completed YYYY-MM-DD]
synthesist task list <tree/spec>              # all tasks with status
synthesist task claim <tree/spec> <id>        # set owner + in_progress
synthesist task done <tree/spec> <id>         # verify acceptance + complete (--skip-verify to bypass)
synthesist task cancel <tree/spec> <id>       # cancel a task [--reason "..."]
synthesist task acceptance <tree/spec> <id> --criterion "..." --verify "cmd"
synthesist task wait <tree/spec> <id> --reason "..." --external "url" --check "cmd"
synthesist task ready <tree/spec>             # unblocked pending tasks

synthesist campaign active <tree> <spec-id> [--summary "..."] [--phase "..."] [--blocked-by spec1,spec2]
synthesist campaign backlog <tree> <spec-id> [--title "..."] [--summary "..."] [--blocked-by spec1,spec2]
synthesist campaign list <tree>

synthesist archive add <tree/spec> --reason completed [--outcome "..."] [--archived YYYY-MM-DD] [--patterns p1,p2]
synthesist archive list <tree>

synthesist discovery add <tree/spec> --finding "..." [--impact "..."] [--action "..."] [--author agent] [--date YYYY-MM-DD]
synthesist discovery list <tree/spec>

synthesist stakeholder add <tree> <id> --context "role" [--name "Full Name"] [--orgs "org1,org2"]
synthesist stakeholder list <tree>
synthesist disposition add <tree/spec> <stakeholder> --topic "..." --stance cautious --confidence inferred [--preferred "..."]
synthesist disposition list <tree/spec>
synthesist disposition supersede <tree/spec> <id> --new-stance supportive [--evidence <signal-id>]
synthesist signal record <tree/spec> <stakeholder> --source "url" --type pr_comment --content "..." [--date YYYY-MM-DD] [--our-action "..."] [--interpretation "..."]
synthesist signal list <tree/spec>

synthesist direction add <tree> --project "org/repo" --topic "..." --status proposed --impact "..." [--owner stakeholder-id] [--timeline "..."]
synthesist direction list <tree>
synthesist direction impact <tree> <direction-id> --affected-tree "..." --affected-spec "..." --description "..."

synthesist retro create <tree/spec> --arc "..." --depends-on t8[,t9]
synthesist retro transform <tree/spec> --label "..." --description "..." [--transferable]
synthesist retro show <tree/spec>
synthesist pattern register <tree> <id> --name "..." --description "..." [--transferability "..."] [--observed-in spec1,spec2]
synthesist pattern list <tree>

synthesist landscape show <tree/spec>      # full stakeholder graph for a spec
synthesist stance <stakeholder>            # current dispositions across tree
synthesist stance <stakeholder> <topic>    # disposition history for person + topic
synthesist replay <tree/spec>              # playbook: DAG + transforms + patterns + landscape
synthesist check                           # validate referential integrity
` + "```" + `

## Replay

` + "`synthesist replay`" + ` outputs the task DAG shape, retro transforms, patterns, and
landscape from a completed spec. Use this to adapt a body of work for a
new context: read the transforms (what moves were made and why), check
the landscape (what stakeholder constraints shaped the choices), then
create a new spec in the target tree with adapted tasks.
`

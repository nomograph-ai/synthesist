package main

import (
	"fmt"
	"reflect"
	"strings"

	"gitlab.com/nomograph/synthesist/docs"
)

// generateSkillContent builds the skill file from the kong CLI struct
// and the embedded state machine document.
func generateSkillContent(cli any) string {
	var b strings.Builder

	b.WriteString(skillPreamble)
	b.WriteString("\n## Core commands\n\n```\n")
	writeCommands(&b, "synthesist", reflect.TypeOf(cli))
	b.WriteString("```\n")
	b.WriteString("\n## Authored behavioral rules\n\n")
	b.WriteString(docs.StateMachine)
	b.WriteString("\n")
	return b.String()
}

// writeCommands recursively walks the kong CLI struct to produce
// command usage lines. Sub-commands become "parent child" prefixes.
func writeCommands(b *strings.Builder, prefix string, t reflect.Type) {
	if t.Kind() == reflect.Ptr {
		t = t.Elem()
	}
	if t.Kind() != reflect.Struct {
		return
	}

	for i := 0; i < t.NumField(); i++ {
		f := t.Field(i)
		tag := f.Tag

		// A field with cmd:"" is a sub-command group.
		if _, ok := tag.Lookup("cmd"); ok {
			ft := f.Type
			if ft.Kind() == reflect.Ptr {
				ft = ft.Elem()
			}
			cmdName := strings.ToLower(f.Name)
			if ft.Kind() == reflect.Struct && hasSubCommands(ft) {
				// Parent command with sub-commands.
				writeCommands(b, prefix+" "+cmdName, ft)
			} else {
				// Leaf command.
				writeLeaf(b, prefix+" "+cmdName, ft, tag)
			}
			continue
		}
	}
}

// hasSubCommands returns true if the struct has fields tagged with cmd:"".
func hasSubCommands(t reflect.Type) bool {
	for i := 0; i < t.NumField(); i++ {
		if _, ok := t.Field(i).Tag.Lookup("cmd"); ok {
			return true
		}
	}
	return false
}

// writeLeaf writes one command usage line: name, args, flags.
func writeLeaf(b *strings.Builder, name string, t reflect.Type, parentTag reflect.StructTag) {
	if t.Kind() == reflect.Ptr {
		t = t.Elem()
	}

	var args []string
	var flags []string

	if t.Kind() == reflect.Struct {
		for i := 0; i < t.NumField(); i++ {
			f := t.Field(i)
			tag := f.Tag

			// Positional arg
			if _, ok := tag.Lookup("arg"); ok {
				argName := strings.ToLower(f.Name)
				if n, ok := tag.Lookup("name"); ok {
					argName = n
				}
				args = append(args, "<"+argName+">")
				continue
			}

			// Flag
			if flagName, ok := tag.Lookup("name"); ok {
				help := tag.Get("help")
				required := tag.Get("required")

				// Bool flags (no value)
				if f.Type.Kind() == reflect.Bool {
					flags = append(flags, fmt.Sprintf("[--%-12s %s]", flagName, help))
					continue
				}

				if required == "true" {
					flags = append(flags, fmt.Sprintf("--%-12s %s", flagName, help))
				} else {
					flags = append(flags, fmt.Sprintf("[--%-12s %s]", flagName, help))
				}
			}
		}
	}

	help := parentTag.Get("help")
	line := name
	for _, a := range args {
		line += " " + a
	}
	for _, f := range flags {
		line += " " + f
	}
	if help != "" {
		// Pad to 50 chars for alignment, then add comment
		for len(line) < 50 {
			line += " "
		}
		line += " # " + help
	}
	b.WriteString(line + "\n")
}

const skillPreamble = `# Synthesist -- Specification Graph Manager

You have access to the ` + "`synthesist`" + ` CLI tool. Use it for ALL specification
management. Do not read or write spec data files directly. The tool
owns the Dolt database at .synth/.

## Concepts

**Trees** are named domains of work (upstream, harness, account). They
are explicit entities that must be created with ` + "`synthesist tree create`" + `
before you can add specs, stakeholders, or campaigns to them.

**tree/spec format**: Most commands take specs as ` + "`tree/spec`" + ` -- e.g.
` + "`upstream/auth-service`" + `, ` + "`harness/site-redesign`" + `. Campaign commands
take ` + "`<tree> <spec-id>`" + ` as two separate positional arguments instead.

**Specs** capture intent (goal, constraints, decisions) via
` + "`synthesist spec create`" + `. Specs can also be created implicitly when
you add the first task.

**Threads** are session pointers that track active workstreams. Create
them with ` + "`synthesist thread create`" + ` to record what you're working on.
` + "`synthesist status`" + ` shows all active threads.

**Propagation chains** link specs so that when a source spec's output
changes, downstream specs are flagged as potentially stale. Use
` + "`synthesist propagation check`" + ` to find stale targets.

**Stakeholders** are registered per-tree (` + "`synthesist stakeholder add <tree>`" + `).
They are referenced by ID across specs in that tree. Dispositions and
signals are per-spec (` + "`tree/spec`" + ` format).

**Bootstrap**: Run ` + "`synthesist init`" + ` to create the database, then
` + "`synthesist tree create`" + ` to set up your first tree.

## Enums

**stance**: supportive | cautious | opposed | neutral | unknown
**confidence**: documented | verified | inferred | speculative
**signal type**: pr_comment | issue_comment | review | commit_message | chat | meeting | email | other
**influence role**: maintainer | reviewer | approver | blocker | champion | observer
**direction status**: committed | proposed | experimental | rejected
**task status**: pending | in_progress | done | blocked | waiting | cancelled
**archive reason**: completed | abandoned | superseded | deferred

## When to use synthesist

- New project setup: ` + "`synthesist scaffold`" + ` creates CLAUDE.md, .mise.toml, and .synth/ database
- Starting a session: ` + "`synthesist session start <name>`" + ` then ` + "`synthesist status`" + `
- Planning work: ` + "`synthesist spec create`" + ` then ` + "`synthesist task create`" + `
- Executing work: ` + "`synthesist task claim`" + ` then ` + "`synthesist task done`" + ` when verified
- Blocking a task: ` + "`synthesist task block`" + ` for internal blockers
- External blockers: ` + "`synthesist task wait`" + ` with a check command
- Tracking people: ` + "`synthesist stakeholder add`" + ` and ` + "`synthesist disposition add`" + `
- Recording evidence: ` + "`synthesist signal record`" + ` for observable stakeholder actions
- Recording findings: ` + "`synthesist discovery add`" + ` for institutional memory
- Managing campaigns: ` + "`synthesist campaign active/backlog`" + `
- Archiving completed work: ` + "`synthesist archive add`" + `
- Cross-spec dependencies: ` + "`synthesist propagation add`" + ` then ` + "`synthesist propagation check`" + `
- Completing a spec: ` + "`synthesist retro create`" + ` with arc and transforms
- Checking health: ` + "`synthesist check`" + ` validates everything
- Replaying work: ` + "`synthesist replay <spec>`" + ` to get a playbook for adaptation
- Checking schema: ` + "`synthesist migrate`" + ` to check database version and pending migrations
- Backup: ` + "`synthesist export`" + ` dumps all tables to JSON
- Cleaning stale sessions: ` + "`synthesist session prune`" + ` merges and removes inactive branches

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

`

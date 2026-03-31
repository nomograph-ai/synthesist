package main

import (
	"os"
	"path/filepath"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

const miseContent = `[tools]
"ubi:nomograph/synthesist" = { version = "v5.0.0", exe = "synthesist", provider = "gitlab" }
`

const synthClaudeMDSection = `## Synthesist

All work goes through synthesist. Do not execute tasks ad-hoc — create
a spec and tasks first, then execute through the workflow state machine.

Run ` + "`synthesist skill`" + ` for the full behavioral contract and command reference.

Session required for all writes:
- ` + "`synthesist session start <name>`" + ` before working
- ` + "`SYNTHESIST_SESSION=<name>`" + ` on every write command
- ` + "`synthesist session merge <name>`" + ` when done

Multiple agents can work concurrently — each starts its own session.
Sessions are isolated Dolt branches with row-level merge.
`

const claudeCommandContent = `# Synthesist Orient

Start every work session by running the synthesist ORIENT sequence.
This loads the estate state, stakeholder dispositions, and pending work.

## Steps

1. ` + "`synthesist status`" + ` — estate overview
2. ` + "`synthesist session start <name>`" + ` — start your session
3. For the spec you will work on:
   - ` + "`synthesist spec show <tree/spec>`" + `
   - ` + "`synthesist landscape show <tree/spec>`" + `
   - ` + "`synthesist discovery list <tree/spec>`" + `
4. Present findings to the human before planning

Run ` + "`synthesist skill`" + ` for the full behavioral contract.
`

const cursorRuleContent = `---
description: Synthesist specification graph manager — behavioral contract
globs: ["**"]
alwaysApply: true
---

# Synthesist

All work goes through synthesist. Do not execute tasks ad-hoc — create
a spec and tasks first, then execute through the workflow state machine.

## Session Start

1. ` + "`synthesist status`" + ` — estate overview
2. ` + "`synthesist session start <name>`" + ` — start your session
3. For the spec you will work on:
   - ` + "`synthesist spec show <tree/spec>`" + `
   - ` + "`synthesist landscape show <tree/spec>`" + `
4. Present findings to the human before planning

Run ` + "`synthesist skill`" + ` for the full behavioral contract and command reference.
Follow the state machine: ORIENT → PLAN → AGREE → EXECUTE ↔ REFLECT → REPORT.
The AGREE phase requires explicit human approval before execution begins.

Multiple agents can work concurrently — each must start its own session.
`

func cmdScaffold() error {
	dir, err := os.Getwd()
	if err != nil {
		return err
	}

	result := map[string]string{}

	// 1. .mise.toml
	misePath := filepath.Join(dir, ".mise.toml")
	if _, err := os.Stat(misePath); os.IsNotExist(err) {
		if err := os.WriteFile(misePath, []byte(miseContent), 0o644); err != nil {
			return Wrap("writing .mise.toml", err)
		}
		result["mise_toml"] = "created"
	} else {
		result["mise_toml"] = "skipped"
	}

	// 2. CLAUDE.md
	claudePath := filepath.Join(dir, "CLAUDE.md")
	if _, err := os.Stat(claudePath); os.IsNotExist(err) {
		// Create new file with just the synthesist section
		if err := os.WriteFile(claudePath, []byte(synthClaudeMDSection), 0o644); err != nil {
			return Wrap("writing CLAUDE.md", err)
		}
		result["claude_md"] = "created"
	} else {
		// File exists — check if it already has a ## Synthesist section
		content, err := os.ReadFile(claudePath)
		if err != nil {
			return Wrap("reading CLAUDE.md", err)
		}
		if strings.Contains(string(content), "## Synthesist") {
			result["claude_md"] = "skipped"
		} else {
			// Append the section
			f, err := os.OpenFile(claudePath, os.O_APPEND|os.O_WRONLY, 0o644)
			if err != nil {
				return Wrap("opening CLAUDE.md for append", err)
			}
			defer f.Close() //nolint:errcheck
			if _, err := f.WriteString("\n" + synthClaudeMDSection); err != nil {
				return Wrap("appending to CLAUDE.md", err)
			}
			result["claude_md"] = "appended"
		}
	}

	// 3. Claude Code command
	claudeCmdDir := filepath.Join(dir, ".claude", "commands")
	claudeCmdPath := filepath.Join(claudeCmdDir, "synthesist-orient.md")
	if _, err := os.Stat(claudeCmdPath); os.IsNotExist(err) {
		if err := os.MkdirAll(claudeCmdDir, 0o755); err != nil {
			return Wrap("creating .claude/commands/", err)
		}
		if err := os.WriteFile(claudeCmdPath, []byte(claudeCommandContent), 0o644); err != nil {
			return Wrap("writing claude command", err)
		}
		result["claude_command"] = "created"
	} else {
		result["claude_command"] = "skipped"
	}

	// 4. Cursor rule
	cursorRuleDir := filepath.Join(dir, ".cursor", "rules")
	cursorRulePath := filepath.Join(cursorRuleDir, "synthesist.mdc")
	if _, err := os.Stat(cursorRulePath); os.IsNotExist(err) {
		if err := os.MkdirAll(cursorRuleDir, 0o755); err != nil {
			return Wrap("creating .cursor/rules/", err)
		}
		if err := os.WriteFile(cursorRulePath, []byte(cursorRuleContent), 0o644); err != nil {
			return Wrap("writing cursor rule", err)
		}
		result["cursor_rule"] = "created"
	} else {
		result["cursor_rule"] = "skipped"
	}

	// 5. Run synthesist init if .synth/ doesn't exist
	synthPath := filepath.Join(dir, ".synth", "synthesist", ".dolt")
	if _, err := os.Stat(synthPath); os.IsNotExist(err) {
		s, err := store.Init(dir)
		if err != nil {
			return Wrap("initializing synth database", err)
		}
		_ = s.Close()
		result["synth_db"] = "created"
	} else {
		result["synth_db"] = "exists"
	}

	return jsonOut(result)
}

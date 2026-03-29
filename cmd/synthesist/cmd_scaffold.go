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
- ` + "`synthesist session merge <name>`" + ` when done
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

	// 3. Run synthesist init if .synth/ doesn't exist
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

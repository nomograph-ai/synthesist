package main

import (
	"fmt"
	"os"
	"strings"

	"github.com/alecthomas/kong"
	"gitlab.com/nomograph/synthesist/internal/store"
)

var version = "dev" // set by -ldflags "-X main.version=..."

// noCommit disables auto-commit when --no-commit is passed globally.
var noCommit bool

// discoverStore wraps store.Discover, applies flags, and ensures session branch.
func discoverStore() (*store.Store, error) {
	s, err := store.Discover()
	if err != nil {
		return nil, err
	}
	if noCommit {
		s.AutoCommit = false
	}
	if err := s.EnsureSession(); err != nil {
		_ = s.Close()
		return nil, err
	}
	return s, nil
}

func main() {
	// Strip global flags from os.Args before kong parses.
	// These can appear anywhere in the arg list.
	var filtered []string
	var forcePhase bool
	for _, arg := range os.Args {
		switch {
		case arg == "--no-commit":
			noCommit = true
		case arg == "--force":
			forcePhase = true
		case len(arg) > 10 && arg[:10] == "--session=":
			store.Session = arg[10:]
		default:
			filtered = append(filtered, arg)
		}
	}
	// Also check SYNTHESIST_SESSION env var (flag takes precedence)
	if store.Session == "" {
		store.Session = os.Getenv("SYNTHESIST_SESSION")
	}
	os.Args = filtered

	var cli CLI
	ctx := kong.Parse(&cli,
		kong.Name("synthesist"),
		kong.Description("specification graph manager"),
		kong.UsageOnError(),
	)

	// Generate skill content from the kong struct.
	initSkillContent(cli)

	// Enforce session for write operations.
	// Read-only commands and read subcommands work without a session.
	// This mirrors the original enforcement: top-level read-only commands
	// and read-only subcommands (list, show) bypass the session requirement.
	readOnlyCommands := map[string]bool{
		"init": true, "scaffold": true, "status": true, "check": true,
		"ready": true, "landscape": true, "stance": true, "replay": true,
		"session": true, "skill": true, "version": true, "help": true,
		"migrate": true, "export": true,
	}
	readOnlySubcommands := map[string]bool{
		"list": true, "show": true, "ready": true, "check": true,
	}

	cmdPath := ctx.Command()
	parts := strings.Fields(cmdPath)
	topCmd := ""
	subCmd := ""
	if len(parts) > 0 {
		topCmd = parts[0]
	}
	if len(parts) > 1 {
		subCmd = parts[1]
	}

	if !readOnlyCommands[topCmd] && !readOnlySubcommands[subCmd] && store.Session == "" {
		_, _ = fmt.Fprintf(os.Stderr, "error: session required for write operations\n\n")
		// Try to show active sessions for context
		if s, err := store.Discover(); err == nil {
			if branches, bErr := s.ListBranches(); bErr == nil {
				var sessions []string
				for _, b := range branches {
					if b != "main" {
						sessions = append(sessions, b)
					}
				}
				if len(sessions) > 0 {
					_, _ = fmt.Fprintf(os.Stderr, "  active sessions:\n")
					for _, sess := range sessions {
						_, _ = fmt.Fprintf(os.Stderr, "    - %s\n", sess)
					}
					_, _ = fmt.Fprintf(os.Stderr, "\n  join one:  SYNTHESIST_SESSION=%s %s\n", sessions[0], cmdPath)
				}
			}
			_ = s.Close()
		}
		_, _ = fmt.Fprintf(os.Stderr, "  start new: synthesist session start <name>\n")
		_, _ = fmt.Fprintf(os.Stderr, "  then:      SYNTHESIST_SESSION=<name> synthesist %s\n", cmdPath)
		os.Exit(1)
	}

	// Phase enforcement: check if the operation is allowed in the current phase.
	// Only enforced for write operations (reads always allowed).
	// --force (stripped from os.Args above) bypasses enforcement.
	// NOTE: This opens the database a second time (the command's Run() opens it
	// again via discoverStore). This is acceptable for a CLI tool — the process
	// exits immediately after the command completes, and restructuring to share
	// the connection across kong's Run() boundary adds complexity without benefit.
	if !readOnlyCommands[topCmd] && !readOnlySubcommands[subCmd] && !forcePhase && topCmd != "phase" {
		if s, err := store.Discover(); err == nil {
			var phase string
			if qErr := s.DB.QueryRow("SELECT name FROM phase WHERE id = 1").Scan(&phase); qErr == nil {
				violation := ""
				switch phase {
				case "orient":
					violation = "no writes allowed in ORIENT phase"
				case "plan":
					if topCmd == "task" && (subCmd == "claim" || subCmd == "done" || subCmd == "block") {
						violation = "cannot claim/complete tasks in PLAN phase — transition to EXECUTE first"
					}
				case "agree":
					violation = "no operations in AGREE phase — present the plan and wait for human approval"
				case "execute":
					if topCmd == "task" && subCmd == "create" {
						violation = "cannot create tasks in EXECUTE phase — transition to REPLAN first"
					}
					if topCmd == "task" && subCmd == "cancel" {
						violation = "cannot cancel tasks in EXECUTE phase — transition to REPLAN first"
					}
					if topCmd == "spec" && subCmd == "create" {
						violation = "cannot create specs in EXECUTE phase — transition to REPLAN first"
					}
				case "reflect":
					if topCmd == "task" && subCmd == "claim" {
						violation = "cannot claim tasks in REFLECT phase — complete retrospective first"
					}
				case "replan":
					if topCmd == "task" && subCmd == "claim" {
						violation = "cannot claim tasks in REPLAN phase — finalize plan first"
					}
				case "report":
					violation = "no writes allowed in REPORT phase"
				}
				if violation != "" {
					_, _ = fmt.Fprintf(os.Stderr, "error: phase violation (%s): %s\n", phase, violation)
					_, _ = fmt.Fprintf(os.Stderr, "  current phase: %s\n", phase)
					_, _ = fmt.Fprintf(os.Stderr, "  use --force to override\n")
					_ = s.Close()
					os.Exit(1)
				}
			}
			_ = s.Close()
		}
	}

	err := ctx.Run()
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}
}

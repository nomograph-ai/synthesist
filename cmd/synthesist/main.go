package main

import (
	"fmt"
	"os"

	"gitlab.com/nomograph/synthesist/internal/store"
)

var version = "dev" // set by -ldflags "-X main.version=..."

// noCommit disables auto-commit when --no-commit is passed globally.
var noCommit bool

// discoverStore wraps store.Discover and applies the --no-commit flag.
func discoverStore() (*store.Store, error) {
	s, err := store.Discover()
	if err != nil {
		return nil, err
	}
	if noCommit {
		s.AutoCommit = false
	}
	return s, nil
}

func main() {
	// Strip --no-commit from os.Args before dispatching to subcommands.
	var filtered []string
	for _, arg := range os.Args {
		if arg == "--no-commit" {
			noCommit = true
		} else {
			filtered = append(filtered, arg)
		}
	}
	os.Args = filtered

	if len(os.Args) < 2 {
		printUsage()
		os.Exit(1)
	}

	cmd := os.Args[1]
	args := os.Args[2:]

	var err error
	switch cmd {
	// Estate
	case "init":
		err = cmdInit(args)
	case "status":
		err = cmdStatus(args)
	case "check":
		err = cmdCheck(args)

	// Task DAG
	case "task":
		err = cmdTask(args)

	// Landscape
	case "stakeholder":
		err = cmdStakeholder(args)
	case "disposition":
		err = cmdDisposition(args)
	case "signal":
		err = cmdSignal(args)

	// Retro + Patterns
	case "retro":
		err = cmdRetro(args)
	case "pattern":
		err = cmdPattern(args)

	// Query
	case "ready":
		err = cmdReady(args)
	case "landscape":
		err = cmdLandscape(args)
	case "stance":
		err = cmdStance(args)
	case "replay":
		err = cmdReplay(args)

	// Meta
	case "skill":
		err = cmdSkill(args)
	case "version":
		fmt.Println(version)
	case "help":
		printUsage()
	default:
		fmt.Fprintf(os.Stderr, "unknown command: %s\n", cmd)
		printUsage()
		os.Exit(1)
	}

	if err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}
}

func printUsage() {
	fmt.Print(`synthesist -- specification graph manager

Estate:
  init                          Scaffold estate structure in current directory
  status                        Show estate overview (trees, threads, ready tasks)
  check                         Validate all specs, landscapes, references

Task DAG:
  task create <spec> <summary>  Add a task to a spec
  task list <spec>              List tasks with status
  task claim <spec> <id>        Set owner + in_progress
  task done <spec> <id>         Verify acceptance criteria, transition to done
  task wait <spec> <id>         Set waiting status with waiter object
  task block <spec> <id>        Set blocked status
  task ready <spec>             Show unblocked, pending tasks

Landscape:
  stakeholder add <tree> <id>   Register a stakeholder in a tree
  stakeholder list <tree>       List stakeholders for a tree
  disposition add <spec>        Record a disposition assessment
  disposition list <spec>       List current dispositions for a spec
  disposition supersede <spec>  Update a disposition with new evidence
  signal record <spec>          Record an observed signal
  signal list <spec>            List signals for a spec
  landscape show <spec>         Full stakeholder graph for a spec

Retro + Patterns:
  retro create <spec>           Create a retrospective node
  retro show <spec>             Show retro with transforms and patterns
  pattern register <tree>       Register a named pattern
  pattern list <tree>           List patterns for a tree
  replay <spec>                 Generate replay playbook (retro + DAG + landscape)

Stance:
  stance <stakeholder>          Current dispositions for a person (across tree)
  stance <stakeholder> <topic>  Disposition history for person + topic

Meta:
  skill                         Output the synthesist skill file for LLM harness
  check                         Validate estate integrity
  version                       Print version
  help                          This message

All output is JSON by default. Use --human for human-readable output.
Auto-commits are on by default. Use --no-commit to disable.
`)
}

// cmdReady is a top-level alias for "synthesist task ready"
func cmdReady(args []string) error { return cmdTaskReady(args) }

func cmdSkill(args []string) error {
	fmt.Print(skillContent)
	return nil
}

func cmdTask(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("task requires a subcommand: create, list, claim, done, wait, block, ready")
	}
	sub := args[0]
	rest := args[1:]
	switch sub {
	case "create":
		return cmdTaskCreate(rest)
	case "list":
		return cmdTaskList(rest)
	case "claim":
		return cmdTaskClaim(rest)
	case "done":
		return cmdTaskDone(rest)
	case "wait":
		return cmdTaskWait(rest)
	case "block":
		return cmdTaskBlock(rest)
	case "ready":
		return cmdTaskReady(rest)
	default:
		return fmt.Errorf("unknown task subcommand: %s", sub)
	}
}

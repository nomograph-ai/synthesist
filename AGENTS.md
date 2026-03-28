# Synthesist -- Agent Instructions

You have the `synthesist` CLI tool for all specification management. Run
`synthesist skill` for the complete command reference, rules, and usage patterns.

## Session start

```bash
synthesist status
```

This shows trees, active threads, task counts, and ready tasks across the estate.
Pick up where the last session left off.

## Core workflow

1. `synthesist task create <tree/spec> <summary>` -- plan work
2. `synthesist task claim <tree/spec> <id>` -- start a task
3. Do the work (write code, research, etc.)
4. `synthesist task done <tree/spec> <id>` -- marks done only if acceptance criteria pass
5. `synthesist status` -- see what's next

## Rules

- **Never write data files directly.** All spec data lives in the Dolt database
  at `.synth/`. Use `synthesist` commands for all reads and writes.
- **spec.md is still human/agent-written intent** (Markdown). Create and edit
  spec.md files normally. But state management (tasks, status, stakeholders,
  dispositions) goes through the CLI.
- **Run `synthesist skill` for the full reference.** This file is the bootstrap.
  The skill output has every command, every flag, every rule.
- **All output is JSON.** Parse it directly. Do not ask the human to interpret
  synthesist output.

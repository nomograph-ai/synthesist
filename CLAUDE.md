# Synthesist

Specification graph manager. Rust + SQLite.

## Build

```
make build          # compile release binary, copy to ./synthesist
make test           # build + run all tests
make lint           # cargo clippy -D warnings
make check          # build + smoke-test help output
make skill          # build + output skill definition
cargo build         # dev build (no copy)
cargo test          # unit + integration tests only
```

## Data Directory

The working data directory is `synthesist/` (relative to the repo root where
`synthesist init` was run). Never access the SQLite database file directly --
all reads and writes go through `synthesist` subcommands.

## Skill File

The skill file is the primary LLM interface. Run `synthesist skill` to emit
the full behavioral contract, command reference, and state machine. Keaton
and other harness agents consume this output.

## Sessions

Sessions use per-file SQLite copies for isolation. Always set the
session context:

- **Environment variable**: `SYNTHESIST_SESSION=<name>` (preferred)
- **Flag**: `--session <name>` on any command

Session lifecycle:
1. `synthesist session start <name>` -- create a session copy
2. Work within the session (all commands read/write the session copy)
3. `synthesist session merge <name>` -- merge back to main
4. `synthesist session list` -- see active sessions

Commit after every task completion, not in batches.

## Workflow State Machine

The LLM follows a 7-phase state machine: ORIENT -> PLAN -> AGREE -> EXECUTE
<-> REFLECT -> REPORT (with REPLAN looping back to AGREE).

Key rules:
- Declare phase with `synthesist phase set <phase>`
- Cannot claim tasks in PLAN. Cannot create tasks in EXECUTE.
- PLAN -> EXECUTE must pass through AGREE (explicit human approval).
- After each task in EXECUTE, enter REFLECT to assess plan validity.
- If the plan changes, REPLAN -> AGREE before resuming EXECUTE.

## Conventions

- **Output**: all commands emit JSON; pipe through `jq` for human reading
- **SQL**: never query the SQLite file directly; use `synthesist sql` for ad-hoc queries
- **File size**: keep source files focused, one concern per file
- **Tests**: integration tests in `tests/integration.rs`
- **Linting**: zero warnings from `make lint`. Fix warnings before committing.
- **Single verification**: `make build && make test && make lint`

## Release Checklist

Before tagging a release:

1. `make build && make test && make lint` -- all pass locally
2. Push to main -- CI pipeline must pass
3. README.md, CHANGELOG.md, CLAUDE.md all reflect the release content
4. `git tag -a vX.Y.Z -m "release notes here"` -- annotated tag with release notes
5. `git push --tags` -- wait for tag CI pipeline to pass
6. `glab release create vX.Y.Z --notes "release notes"` -- create GitLab release

Never tag before CI passes. Never tag with stale documentation.
Never skip release notes -- both the annotated tag and the GitLab release must have them.

# Synthesist

Specification graph manager. Dolt-backed database with Git-like versioning.

## Build

Use `make` for everything. Never call `go build`, `go test`, etc. directly —
the Makefile sets CGO_ENABLED, ICU include/lib paths, and version ldflags
that raw commands miss.

```
make build          # compile binary
make test           # build + run all tests
make lint           # golangci-lint (errcheck, staticcheck, bodyclose)
make check          # build + run synthesist check
make golden-update  # regenerate golden test files (tests/golden/)
make loc-check      # fail if any non-generated Go file exceeds 650 LOC
make skill          # build + output skill definition
```

## Conventions

- **Errors**: use constructors from `cmd/synthesist/errors.go`, never inline `fmt.Errorf`
- **Output**: all commands emit JSON via `jsonOut()`; use `--human` for human-readable output
- **SQL**: raw SQL in store methods, no ORM. SQL is the source of truth.
- **File size**: 650 LOC max per file, one concern per file
- **Tests**: golden file tests in `tests/golden/testdata/*.golden`
- **Linting**: zero warnings from `make lint`. Fix warnings before committing.
- **Single verification**: `make build && make test && make lint`

## Sessions

Concurrent work uses Dolt-branched sessions. Always set the session context:

- **Environment variable**: `SYNTHESIST_SESSION=<name>` (preferred)
- **Flag**: `--session <name>` on any command

Session lifecycle:
1. `synthesist session start <name>` — create a session branch
2. Work within the session (all commands read/write the session branch)
3. `synthesist session merge <name>` — merge back to main
4. `synthesist session prune` — clean up stale sessions

Commit after every task completion, not in batches. The session merge
reconciles data from concurrent agents.

## Workflow State Machine

The LLM follows a 7-phase state machine: ORIENT → PLAN → AGREE → EXECUTE ↔ REFLECT → REPORT (with REPLAN looping back to AGREE).

Full specification: [docs/state-machine.md](docs/state-machine.md). This
document is embedded in the skill file output by `synthesist skill`.

Key rules:
- Declare phase with `synthesist phase set <phase>`
- Cannot claim tasks in PLAN. Cannot create tasks in EXECUTE.
- PLAN → EXECUTE must pass through AGREE (explicit human approval).
- After each task in EXECUTE, enter REFLECT to assess plan validity.
- If the plan changes, REPLAN → AGREE before resuming EXECUTE.

## Command Flags

- `--active` — filter to active/in-progress items (supported by list commands)
- `--human` — human-readable output instead of JSON
- `--session <name>` — select session (or use `SYNTHESIST_SESSION` env var)
- `--force` — override phase validation (use sparingly)
- `--no-commit` — skip automatic git commit on state changes

## Sync Rule

When making changes, keep these in sync before committing:

1. **README.md** — if build commands, architecture, or features changed
2. **CHANGELOG.md** — entry for any user-visible change
3. **Skill file** (generated from kong struct + docs/state-machine.md) — rebuild if commands changed
4. **Golden files** — `make golden-update` if command output shape changed
5. **Package READMEs** — if package purpose or dependencies changed

## Release Checklist

Before tagging a release:

1. `make build && make test && make lint` — all pass locally
2. `make loc-check` — all files under 650 LOC (or justified exceptions documented)
3. Push to main — CI pipeline must pass (check GitLab)
4. README.md, CHANGELOG.md, CLAUDE.md all reflect the release content
5. `make golden-update && make test` — golden files are current
6. `git tag -a vX.Y.Z -m "release notes here"` — annotated tag with release notes
7. `git push --tags` — wait for tag CI pipeline to pass
8. `glab release create vX.Y.Z --notes "release notes"` — create GitLab release with full notes from CHANGELOG

Never tag before CI passes. Never tag with stale documentation.
Never skip release notes — both the annotated tag and the GitLab release must have them.

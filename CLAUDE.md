# Synthesist

Specification graph manager. Rust + claim-core (Automerge) + SQLite view cache.

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

## Storage

State lives in `claims/` at the repo root (created by `synthesist init`).

- `claims/genesis.amc`, `claims/changes/<hash>.amc`, `claims/config.toml`
  are git-tracked. They are the source of truth.
- `claims/snapshot.amc`, `claims/view.sqlite`, `claims/view.heads` are
  gitignored local caches. Deleting them is safe; they rebuild from
  the log.

Never read or write these files directly. All access goes through
`synthesist` subcommands, which call into the
[`nomograph-claim`](https://gitlab.com/nomograph/claim) substrate.

The claim substrate (decisions, claim schema, file layout) is
specified in the [nomograph-claim](https://gitlab.com/nomograph/claim)
repo.

## Skill File

The skill file is the primary LLM interface. Run `synthesist skill` to emit
the full behavioral contract, command reference, and state machine. Keaton
and other harness agents consume this output.

## Sessions

Sessions tag writes. They are claim-scoped, not file-copied. Always
set the session context:

- **Environment variable**: `SYNTHESIST_SESSION=<name>` (preferred)
- **Flag**: `--session <name>` on any command

Session lifecycle:
1. `synthesist session start <name>` -- appends a Session claim
2. Work within the session (all subsequent writes carry the session tag)
3. `synthesist session close <name>` -- appends a supersession closing
   the session
4. `synthesist session list` -- see active sessions

There is no `session merge` or `session discard`. Multi-user writes
merge automatically via CRDT. Conflicts are resolved by supersession;
surface unresolved ones with `synthesist conflicts`.

Commit after every task completion, not in batches.

## Workflow State Machine

The LLM follows a 7-phase state machine: ORIENT -> PLAN -> AGREE -> EXECUTE
<-> REFLECT -> REPORT (with REPLAN looping back to AGREE).

Key rules:
- Declare phase with `synthesist phase set <phase> --session=<id>`
  (or set `SYNTHESIST_SESSION` and omit the flag).
- Cannot claim tasks in PLAN. Cannot create tasks in EXECUTE.
- PLAN -> EXECUTE must pass through AGREE (explicit human approval).
- After each task in EXECUTE, enter REFLECT to assess plan validity.
- If the plan changes, REPLAN -> AGREE before resuming EXECUTE.
- Phase is per-session. Different sessions may be in different phases.
  `phase show`/`phase set` require an explicit session id; sessionless
  invocations error rather than fall back to a global default.

## Conventions

- **Output**: all commands emit JSON; pipe through `jq` for human reading
- **SQL**: never query `view.sqlite` directly; use `synthesist sql` for
  ad-hoc read-only queries against the current view
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

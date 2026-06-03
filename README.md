![synthesist hero](hero.svg)

# Synthesist

[![pipeline](https://gitlab.com/nomograph/synthesist/badges/main/pipeline.svg)](https://gitlab.com/nomograph/synthesist/-/pipelines)
[![license](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![built with GitLab](https://img.shields.io/badge/built_with-GitLab-FC6D26?logo=gitlab)](https://gitlab.com/nomograph/synthesist)

Specification graph manager for AI-augmented collaborative development.
Claim-based storage over per-asserter, append-only JSON-LD logs.

> **Pre-release.** This is `3.0.0-rc.1`, the first v3-native cut. v3
> drops the v2 `.amc` substrate entirely and runs on per-asserter
> JSON-LD logs plus a disposable redb gamma index. The CLI surface is
> stable for the pre line but not yet frozen; the on-disk format is
> committed. If you are on v2, read
> [`MIGRATION-v2-to-v3.md`](MIGRATION-v2-to-v3.md) before upgrading.

## What it is

AI coding agents produce technically correct contributions that get
rejected. Studies of agent-authored pull requests find that a third of
rejections are driven by workflow constraints -- scope violations,
architectural misalignment, process expectations -- not code quality.
The agent wrote correct code for the wrong context.

The missing context is not about code. It is about the process that
governs the code: what has been planned, what has been agreed, what
has already been tried. Synthesist records this process as a graph of
specifications and tasks, annotated by phase, session, and discovery.

Every piece of workflow state is a **claim**: a typed, timestamped,
content-addressed assertion of what someone holds true. Claims are
appended to per-asserter logs and never overwritten -- a field update
is a new claim that *supersedes* the prior one, so the full history is
preserved per field. Multi-user collaboration merges by taking the
union of every asserter's log; there is no merge step. Observation-layer
data (stakeholders, dispositions, signals, topics) has moved to the
companion tool [`lattice`](https://gitlab.com/nomograph/lattice).

Named for the role aboard the *Theseus* in Peter Watts' *Blindsight* --
the crew member whose job is not expertise, but coherence.

## Monorepo layout

This repository is a Cargo workspace. The `synthesist` binary lives at
the repo root; the `claim` substrate is a workspace member under
`claim/`.

```
synthesist/            # repo root: the synthesist crate + binaries
  Cargo.toml           # workspace manifest + [package] for synthesist
  src/                 # synthesist: CLI, schema, overlays, migrations
  claim/               # nomograph-claim: the v3 storage substrate ([lib])
```

`nomograph-claim` is vocabulary-agnostic: it stores any well-formed
claim and serves typed reads through the gamma index. The synthesist
vocabulary (`ClaimType`, per-type validators) lives in synthesist, not
in the substrate.

## Install

### mise

```toml
[tools."http:synthesist"]
version = "3.0.0-rc.1"

[tools."http:synthesist".platforms]
macos-arm64 = { url = "https://gitlab.com/api/v4/projects/80084971/packages/generic/synthesist/v{{version}}/synthesist-darwin-arm64", bin = "synthesist" }
linux-arm64 = { url = "https://gitlab.com/api/v4/projects/80084971/packages/generic/synthesist/v{{version}}/synthesist-linux-arm64", bin = "synthesist" }
```

pre.1 ships macOS ARM and Linux ARM64 only. There is no linux-x64/amd64
artifact on the v3 line.

### Source

```bash
git clone https://gitlab.com/nomograph/synthesist.git
cd synthesist && make build
```

Requires Rust 1.88+. No system dependencies beyond a C compiler.

## Spec Tree

![spec tree](spec-tree.svg)

## Quickstart

Synthesist is an LLM-mediated tool. The human interacts with an LLM
agent; the agent interacts with synthesist. The human never calls
synthesist directly. The LLM reads state, builds a shared mental
model, presents plans, obtains approval, executes work, and reports
results. The binary enforces structure on this process.

```bash
synthesist init                           # scaffolds the claims/ directory
synthesist session start work             # appends a Session claim
export SYNTHESIST_SESSION=work

# Orient: read the landscape
synthesist phase set plan                 # session-scoped via SYNTHESIST_SESSION
synthesist status

# Plan: model the work
synthesist spec add upstream/auth --goal "Migrate auth API v2 to v3"
synthesist task add upstream/auth "Research versioning strategy"
synthesist task add upstream/auth "Implement migration" --depends-on t1
synthesist task add upstream/auth "Write tests" --depends-on t2 --gate human

# Agree: pin the plan snapshot in PLAN, then present and wait
synthesist spec update upstream/auth --agree-snapshot t1,t2,t3
synthesist phase set agree

# Execute: do the work in dependency order
synthesist phase set execute
synthesist task claim upstream/auth t1
synthesist task done upstream/auth t1
synthesist task ready upstream/auth    # shows t2 is now unblocked

# Report and close
synthesist phase set report
synthesist session close work
```

## Storage model

Synthesist v3 stores all workflow state as **claims** inside a
`claims/` directory at the repo root. Each asserter writes its own
append-only JSON-LD log; reads run as typed queries over a disposable
redb "gamma" index rebuilt from the logs.

```
claims/
  <asserter>/log.jsonl  # git-tracked, append-only per-asserter JSON-LD log
  _schema.json          # git-tracked, store schema version
  _view.gamma           # gitignored, disposable redb gamma index
```

The per-asserter logs are the source of truth. Each line is a
self-contained JSON-LD document (inline `@context`, lowerCamelCase
fields). `_view.gamma` is a redb-backed POS/PSO triple index plus a
canonical-doc table; it is a local cache keyed on the logs' heads and
rebuilt on demand. Delete it freely -- it costs only a one-time
rebuild (measured ~60 ms on a 1.5K-claim corpus).

Every update is a new claim that *supersedes* a previous one. Nothing
is overwritten; the full history is preserved per field. Multi-user
writes merge by taking the union of every asserter's log -- there is
no `session merge` step. Unresolved supersession conflicts (one prior
claim with more than one live successor) surface via
`synthesist conflicts`.

There is no Automerge, no SQLite projection, and no `.amc` change
files in v3. The Oxigraph/SPARQL runtime is gone; reads are typed
gamma queries.

See the [`claim/`](claim/) crate (`nomograph-claim`) for substrate
details: the JSON-LD log format, the gamma index, content addressing,
and the v2-read shim used only by the migration.

## Migration from v2

Existing v2 repositories store state as Automerge `.amc` files under
`claims/changes/` and `claims/snapshot.amc`. A one-shot, forward-only
migration drains that estate into v3 per-asserter JSON-LD logs,
preserving original timestamps, asserter attribution, and supersession
links. A `.synthesist-v2-backup.tar.gz` of the v2 state is written
before any files change, so rollback is always available.

```bash
# Dry run first: counts, size estimate, no writes
synthesist migrate v2-to-v3 --dry-run

# Real run
synthesist migrate v2-to-v3
```

The migration does not delete the v2 `.amc` files; it only reads them
through a minimal v2-read shim in `nomograph-claim`. The v3 runtime
ignores them entirely. Once you trust the v3 estate, delete
`claims/changes/` and `claims/snapshot.amc`.

Lattice claim types (`Stakeholder`, `Topic`, `Signal`, `Disposition`,
and friends) are not part of the v3 synthesist surface and are not
migrated by this command. See
[`MIGRATION-v2-to-v3.md`](MIGRATION-v2-to-v3.md) for the full playbook
(pre-flight, dry-run, verify, rollback). The earlier
[`MIGRATION.md`](MIGRATION.md) covers the v1-to-v2 path if you need it.

## Workflow State Machine

![state machine](state-machine.svg)

LLM agents left unconstrained skip planning and proceed directly to
code generation. The workflow state machine enforces a different
pattern with algorithmic enforcement -- the CLI rejects operations
that violate the current phase.

| Phase | What happens | What is forbidden |
|-------|-------------|-------------------|
| ORIENT | Read status, read discoveries. Build a shared mental model. | All writes. |
| PLAN | Create specs and tasks, define dependencies, research, pin the agree snapshot. | Task claims. No executing before agreeing. |
| AGREE | Present the plan. State assumptions. Halt and wait for human approval. | All writes. The agent stops. |
| EXECUTE | Claim and complete tasks in dependency order. | Task creation or cancellation. The plan is fixed. |
| REFLECT | After each task, assess: does the plan still hold? Record discoveries. | Task claims. Step back before stepping forward. |
| REPLAN | Modify the task tree. Returns to AGREE -- the human must re-approve. | Task claims. Changed plans need fresh consent. |
| REPORT | Summarize outcomes, record institutional memory, close the session. | -- |

The critical property is AGREE. The agent presents its full plan,
identifies which tasks need human gates, and waits. The human may
approve, reject, or reshape. Pin the plan snapshot with
`synthesist spec update --agree-snapshot` while still in PLAN: AGREE
forbids writes, so the snapshot must be in place beforehand. It drives
the `plan-at-risk` overlay -- if any pinned task claim is later
superseded, the spec is flagged.

Phase transitions are validated:

```
synthesist phase set execute
# error: invalid phase transition: plan -> execute (valid: agree)
```

**Phase is per-session**, recorded as a Phase claim scoped to the
active session. Concurrent sessions can be in different phases without
interfering.

## Sessions

Sessions tag writes so the origin of every claim is recoverable. A
session is not a separate database file -- it is an asserter namespace.
Claims written with `--session=<id>` carry the session in their
asserter (`user:local:<user>:<id>`), giving attribution and
concurrent-work isolation on top of the shared claim log.

```bash
synthesist session start research         # appends a Session claim
export SYNTHESIST_SESSION=research        # or --session=research per command
# ... work ...
synthesist session close research         # appends a superseding Session claim
synthesist session list                   # show sessions
synthesist session status research        # what this session changed vs the rest
```

There is no `session merge` or `session discard` -- those v1/v2
operations bail with a pointer to `session close` and
`synthesist conflicts`. The union of per-asserter logs *is* the merge;
diamond conflicts surface via `synthesist conflicts`.

## Command Reference

| Area | Commands |
|------|----------|
| Estate | `init`, `status`, `check`, `conflicts`, `version`, `skill` |
| Trees | `tree add`, `tree list` (`--include-closed`), `tree show`, `tree close` (`--start-id`) |
| Specs | `spec add`, `spec show`, `spec update` (`--agree-snapshot`), `spec list` (positional or `--tree`) |
| Tasks | `task add`, `task list`, `task show`, `task update`, `task claim`, `task done`, `task reset`, `task block`, `task wait`, `task cancel`, `task ready`, `task acceptance` |
| Discoveries | `discovery add`, `discovery list` |
| Outcomes | `outcome add`, `outcome list` |
| Campaigns | `campaign add`, `campaign list` |
| Sessions | `session start`, `session close` (`--start-id`), `session list`, `session status` |
| Phase | `phase show` (alias `phase get`), `phase set` |
| Overlays | `overlay list`, `overlay run <name>` |
| Jig | `jig run`, `jig list-scenarios`, `jig list-manifests`, `jig report` |
| Data | `export`, `import`, `migrate list`, `migrate status`, `migrate run`, `migrate v2-to-v3` |

Reads run as typed passes over the gamma index. **Overlays** are named
analysis passes (e.g. `plan-at-risk`, dangling supersedes, diamond
conflicts) that return structured hits; run `overlay list` for the
catalog. There is no ad-hoc query subcommand: `synthesist sql` and
`synthesist query --sparql` are retired along with the SQLite and
Oxigraph runtimes.

The `serve` HTTP dashboard, `claims compact`, and the `stakeholder` /
`disposition` / `signal` / `stance` observation commands are gone.
The observation commands moved to
[`lattice`](https://gitlab.com/nomograph/lattice); invoking them on v3
synthesist prints a pointer to the replacement.

## The Skill File

`synthesist skill` outputs the complete behavioral contract: data
model, workflow state machine, command reference with worked examples,
error handling, and display conventions. This is the primary interface
for LLM agents. It is execution-system agnostic -- works with Claude
Code, Cursor, or any framework that gives an LLM shell access.

## Building

```bash
make build    # release binary (workspace, from repo root)
make test     # integration tests
make lint     # clippy -D warnings
make skill    # emit skill file
```

## License

MIT. See [LICENSE](LICENSE).

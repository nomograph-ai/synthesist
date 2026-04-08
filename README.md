![synthesist hero](hero.svg)

# Synthesist

[![pipeline](https://gitlab.com/nomograph/synthesist/badges/main/pipeline.svg)](https://gitlab.com/nomograph/synthesist/-/pipelines)
[![license](https://img.shields.io/badge/license-MIT-green)](LICENSE)
![built with GitLab](https://img.shields.io/badge/built_with-GitLab-FC6D26?logo=gitlab)

Specification graph manager for AI-augmented projects. Synthesist is an
LLM-mediated tool: the LLM reads estate state, presents plans, gets human
approval, executes work, and reports results. A Rust binary with an embedded
SQLite database tracks task DAGs, stakeholder dispositions, and temporal
signals. LLM agents interact exclusively through CLI commands.

Named for the role aboard the *Theseus* in Peter Watts' *Blindsight*.

## Install

### mise (recommended)

```toml
[tools."http:synthesist"]
version = "1.0.0"

[tools."http:synthesist".platforms]
macos-arm64 = { url = "https://gitlab.com/api/v4/projects/80084971/packages/generic/synthesist/v{{version}}/synthesist-darwin-arm64", bin = "synthesist" }
linux-x64 = { url = "https://gitlab.com/api/v4/projects/80084971/packages/generic/synthesist/v{{version}}/synthesist-linux-amd64", bin = "synthesist" }
```

### Build from source

```bash
git clone https://gitlab.com/nomograph/synthesist.git
cd synthesist
make build    # requires Rust 1.85+
```

## Quick Start

```bash
cd your-project
synthesist init

synthesist session start my-session
synthesist --session=my-session --force spec add upstream/auth \
  --goal "Migrate auth API from v2 to v3"
synthesist --session=my-session --force task add upstream/auth \
  "Research API versioning strategy"
synthesist --session=my-session --force task claim upstream/auth t1
synthesist --session=my-session --force task done upstream/auth t1
synthesist session merge my-session

synthesist status
```

## Data Model

Estate > Trees > Specs > Tasks.

- **Tree**: a repository or project domain
- **Spec**: a unit of work with goal, constraints, and decisions
- **Task**: an atomic work item forming a dependency DAG within a spec
- **Discovery**: a timestamped finding recorded during work
- **Stakeholder**: a person relevant to the work
- **Disposition**: a temporal stance assessment, scoped to a topic, with
  confidence tiers and supersession chains
- **Signal**: immutable bi-temporal evidence (event time vs record time)
- **Campaign**: cross-tree spec coordination

## Disposition Graph

Stakeholder preferences are implicit: encoded in review decisions, PR
comments, and design documents. Dispositions make them explicit and
queryable. Each disposition is scoped to a topic, grounded in observable
signals, and versioned with supersession chains.

```
Signal (evidence) -> Disposition (assessed stance) -> Agent (aligned contribution)
```

See the companion paper: "Context Asymmetry Is a Representation Problem:
Disposition Graphs for AI-Augmented Collaborative Development" (Dunn, 2026).

## Workflow State Machine

7-phase cycle with algorithmic enforcement:

| Phase | Allowed |
|-------|---------|
| ORIENT | Read status, query dispositions, read discoveries. No writes. |
| PLAN | Add tasks/specs, add dependencies. No task claims. |
| AGREE | Present plan. No writes. Block until human approves. |
| EXECUTE | Claim tasks, complete tasks. No task creation. |
| REFLECT | Assess plan validity, record discoveries. No claims. |
| REPLAN | Modify task tree. Returns to AGREE for re-approval. |
| REPORT | Summarize outcomes, record discoveries. Session close. |

Phase transitions are validated: the agent cannot jump from PLAN to
EXECUTE without passing through AGREE.

## Sessions

Sessions provide isolation via per-file SQLite copies. Each session works
on its own database file; changes become visible only after merge.

```bash
synthesist session start factory-01
# ... work in the session ...
synthesist session merge factory-01     # three-way merge to main
synthesist session merge factory-01 --dry-run  # preview without applying
```

Merge uses EXCEPT-based three-way diff with primary-key-aware conflict
detection. Two sessions modifying different rows merge cleanly.

## Architecture

- **Storage**: SQLite via rusqlite (bundled, no system deps)
- **Data directory**: `synthesist/` (visible, not hidden)
- **Journal mode**: DELETE (not WAL, for git compatibility)
- **Session isolation**: per-file copies with ATTACH-based merge
- **CLI framework**: clap (derive macros)
- **Skill emission**: `synthesist skill` outputs the complete LLM
  behavioral contract, generalized across execution systems

The architecture is documented in [docs/architecture-v1.md](docs/architecture-v1.md),
including the literature-informed cutline that determines what ships in
v1.0.0 versus what is deferred pending empirical validation.

## Building

```bash
make build          # release binary -> ./synthesist
make test           # cargo test (22 integration tests)
make lint           # cargo clippy -D warnings
make skill          # output LLM skill file
cargo build         # dev build
```

Requires Rust 1.85+. No system dependencies beyond a C compiler (SQLite
is bundled via the `cc` crate).

## License

MIT. See [LICENSE](LICENSE).

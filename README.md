# Synthesist

A specification graph manager for AI-augmented projects. Synthesist is a Go binary
with an embedded Dolt database that tracks task DAGs, stakeholder intelligence,
temporal dispositions, and retrospective patterns. LLM agents interact exclusively
through CLI commands -- they never read or write data files directly.

Named for the role aboard the *Theseus* in Peter Watts' *Blindsight* -- the one
crew member whose job isn't expertise, but coherence.

## Install

### mise (recommended)

```toml
# .mise.toml
[tools]
"ubi:nomograph/synthesist" = { version = "latest", exe = "synthesist", provider = "gitlab" }
```

### Build from source

```bash
git clone https://gitlab.com/nomograph/synthesist.git
cd synthesist
make build    # requires Go 1.26+, CGo, and ICU (see Building section)
make install  # installs to $GOPATH/bin
```

## Quick Start

```bash
# Initialize in your project
cd your-project
synthesist init

# Create a task in the upstream/auth-service spec (tree/spec format)
synthesist task create upstream/auth-service "Research API versioning strategy"

# Track a stakeholder and their disposition
synthesist stakeholder add upstream mwilson --context "auth-service maintainer"
synthesist disposition add upstream/auth-service mwilson \
  --topic "API versioning" --stance cautious --confidence inferred

# Work through the task DAG
synthesist task claim upstream/auth-service t1
# ... do the work ...
synthesist task done upstream/auth-service t1

# See the full estate overview
synthesist status
```

All output is JSON by default. Use `--human` for human-readable output.

## Data Model

Synthesist stores a temporal specification graph with six node types and
eight edge types.

```
                    ┌──────────────┐   depends_on   ┌──────────────┐
                    │     Task     │───────────────▶│     Task     │
                    │   (pending)  │                │    (done)    │
                    └──────┬───────┘                └──────┬───────┘
                           │                               │
                  influences│                               │
                           │                               │
                    ┌──────▼───────┐                ┌──────▼───────┐
                    │ Stakeholder  │                │    Retro     │
                    │              │                │ (type=retro) │
                    └──────┬───────┘                └──────┬───────┘
                           │                               │
                   signaled│                      patterned│
                           │                               │
                    ┌──────▼───────┐   evidences   ┌──────▼───────┐
                    │    Signal    │───────────────▶│   Pattern    │
                    │ (immutable)  │                │   (named)    │
                    └──────┬───────┘                └──────────────┘
                           │
                    ┌──────▼───────┐   supersedes   ┌──────────────┐
                    │ Disposition  │───────────────▶│ Disposition  │
                    │  (temporal)  │                │ (superseded) │
                    └──────────────┘                └──────────────┘

                    ┌──────────────┐   impacts      ┌──────────────┐
                    │  Direction   │───────────────▶│     Task     │
                    │  (upstream)  │                │              │
                    └──────────────┘                └──────────────┘
```

### Six node types

| Node | What it represents |
|------|--------------------|
| **Task** | A unit of work with executable acceptance criteria and DAG dependencies |
| **Stakeholder** | A human actor relevant to the work, registered per-tree |
| **Signal** | Immutable, timestamped evidence from a stakeholder action |
| **Disposition** | A stakeholder's assessed stance on a technical direction (temporal, supersedable) |
| **Direction** | An upstream technical trajectory with impact assessment and validity window |
| **Pattern** | A named, transferable approach discovered through retrospective analysis |

### Eight edge types

| Edge | From | To | Purpose |
|------|------|----|---------|
| `depends_on` | task | task | DAG ordering |
| `influences` | stakeholder | task | Who affects which work |
| `disposition_of` | disposition | stakeholder | Stance on a topic |
| `evidenced_by` | disposition | signal | What supports an assessment |
| `impacts` | direction | spec | Which specs a trajectory affects |
| `observed_in` | pattern | spec | Where a pattern was used |
| `provenance` | task | task | "While doing X we discovered Y" (cross-spec) |
| `supersedes` | disposition/direction | disposition/direction | Temporal replacement chain |

### Temporal validity

Dispositions and directions have `valid_from` / `valid_until` windows. When new
evidence changes an assessment, the old record is superseded (not deleted) and a
new one is created. History is preserved. The query "what did we think this
person's stance was on date X?" resolves by filtering the validity windows.

## The Skill File

`synthesist skill` outputs the complete LLM behavioral contract -- the full
command reference, rules, and usage patterns. This is the primary interface
documentation for agents.

Install it into any LLM harness by referencing the skill output in your agent
instructions:

```bash
# For Claude Code -- add to AGENTS.md or CLAUDE.md:
# "Run synthesist skill for the full command reference"

# For OpenCode -- create a skill file:
synthesist skill > .opencode/skills/synthesist/SKILL.md

# For any other agent framework:
synthesist skill >> your-agent-config
```

The tool is agent-agnostic. It works with Claude Code, OpenCode, Cursor, or any
framework that gives an LLM access to shell commands.

## Architecture

### Dolt embedded database

The Dolt database lives at `.synth/synthesist/.dolt/` inside the consuming project.
Dolt is an embedded SQL database with git semantics -- content-addressed storage,
branch/merge on data, and table-level diffing.

```
your-project/
├── .synth/                    # Dolt database (created by synthesist init)
│   └── synthesist/.dolt/      # Database files
├── AGENTS.md                  # or CLAUDE.md -- tells agent to use synthesist
└── ...
```

### Why not JSON files?

v1-v4 stored all state as JSON files that LLM agents read and wrote directly. This
worked for simple task DAGs but broke down with temporal stakeholder intelligence.
Temporal queries across flat JSON files require loading everything and reconstructing
relationships in memory. LLMs writing raw JSON are trusted to produce valid state
transitions with no enforcement layer.

### Why not SQLite?

SQLite would require a separate JSON projection layer for git tracking. Dolt
eliminates this by being both the database and the version-controlled artifact.
`synthesist diff` shows table-level changes between commits without an external
diffing tool.

### Git-tracked .synth/ directory

The `.synth/` directory is tracked in git. When the binary writes data, it commits
to both the Dolt internal history and the outer git repository. Other contributors
pull the database as part of normal `git pull`. The tradeoff: `git diff` on `.synth/`
is binary, but `synthesist diff` provides richer table-level diffs.

### Binary owns all writes

The `synthesist` binary is the single write path to the database. This enforces:

- Valid state transitions (a task can only go `pending -> in_progress -> done`)
- Referential integrity (a disposition must reference an existing stakeholder)
- Temporal consistency (superseding a disposition sets `valid_until` and creates the replacement atomically)
- Automatic git commits on state changes (configurable with `--no-commit`)

LLMs produce better results when constrained to well-formed operations (Yegge,
Beads 2026). A CLI with typed commands prevents invalid states and handles
computation LLMs are bad at -- temporal resolution, graph traversal, date math.

## Key Design Decisions

**Why Dolt over TerminusDB?** TerminusDB is graph-native with better traversal,
but requires running a server. Synthesist needs an embedded database that compiles
into a single binary.

**Why a binary at all?** A CLI with typed commands provides a stable API that
decouples storage format from agent interface. Invalid state transitions are
impossible. The binary handles things LLMs are bad at (date math, temporal
queries, referential integrity checks) so agents can focus on what they're good
at (reasoning over context, making implementation decisions).

**Why temporal dispositions?** The delta between proposed implementation and what
a maintainer will accept is the real cost of upstream contributions. Disposition
tracking models that delta so agents make informed choices instead of contributing
blind. The temporal model preserves history -- when a maintainer changes their
mind, we can see the arc.

**Why retrospective replay?** Retro nodes with labeled transforms enable "play
back this work onto a different project." An agent reads the transforms (what moves
were made and why), checks the landscape (what stakeholder constraints shaped
choices), and generates a new spec adapted for the target context. This is the
Synthesist's core competency -- making work transferable.

**LLM simulation methodology.** Synthesist embodies a simulation approach to LLM
tool design: constrain the agent to well-formed operations, handle computation
externally, and let the agent focus on reasoning. This aligns with the Beads
framework (Yegge 2026) for structured agent interactions, the Graphiti/Zep
approach to temporal knowledge graphs, and the Howard & Matheson framing of
decision analysis as structured information flow.

## Building

### Prerequisites

- **Go 1.26+** with CGo enabled
- **ICU libraries** (required by Dolt):
  - macOS: `brew install icu4c@78` (or `brew install icu4c`)
  - Linux: `apt-get install libicu-dev` (Debian/Ubuntu) or `dnf install libicu-devel` (Fedora)

### Build commands

```bash
make build      # Build the binary (./synthesist)
make test       # Run all tests
make install    # Install to $GOPATH/bin
make lint       # Run go vet
make check      # Build + run synthesist check against local specs
make dev        # Build + show help
make skill      # Build + output the LLM skill file
make release    # Cross-compile for darwin/arm64, darwin/amd64, linux/amd64, linux/arm64
```

The Makefile auto-detects ICU on macOS via Homebrew and sets the correct
`CGO_CFLAGS`, `CGO_CXXFLAGS`, and `CGO_LDFLAGS`.

## Version History

See [CHANGELOG.md](CHANGELOG.md) for the full history. Brief summary:

- **v5** (2026-03-28) -- Dolt embedded storage, Go CLI binary, temporal specification graphs
- **v4** (2026-03-27) -- Concurrent session support with active threads
- **v3** (2026-03-21) -- Context trees, estate switchboard, campaign coordination
- **v2** (2026-03-18) -- Single primary agent, campaigns, concurrent sessions
- **v1** (2026-03-15) -- Spec format, agent roles, executable acceptance criteria

## Sources and Influences

| Source | What we took |
|--------|-------------|
| [Beads](https://github.com/steveyegge/beads) (Steve Yegge, 2026) | Structured agent interactions, CLI as stable API, "constrain the LLM to well-formed operations" |
| [Graphiti/Zep](https://github.com/getzep/graphiti) | Temporal knowledge graphs, bi-temporal entity modeling |
| Howard & Matheson (1968) | Decision analysis as structured information flow |
| [Symphony](https://github.com/openai/symphony) (OpenAI) | WORKFLOW.md hybrid format, stall detection |
| [BMAD Method](https://github.com/bmad-code-org/BMAD-METHOD) | Scale-adaptive planning, human gates |
| [Gastown](https://github.com/steveyegge/gastown) (Steve Yegge) | Typed dependency graphs, "findings survive context death" |
| [Metaswarm](https://github.com/dsifry/metaswarm) | "Trust nothing, verify everything", cross-model review |
| Tam et al. (arXiv 2408.02442) | No structured format consistently wins for LLM reasoning |

## License

MIT -- see [LICENSE](LICENSE).

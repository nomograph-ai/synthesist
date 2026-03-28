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

### Structure

The estate is a hierarchy. Trees organize work by domain. Specs are
units of work within a tree. Tasks form a DAG within a spec.
Campaigns track what's active and what's waiting.

```mermaid
graph TD
    E[Estate] --> T1[Tree<br/><i>upstream</i>]
    E --> T2[Tree<br/><i>harness</i>]
    E --> T3[Tree<br/><i>account</i>]

    T1 --> C1[Campaign<br/><i>active + backlog</i>]
    T1 --> SH1[Stakeholders<br/><i>per-tree registry</i>]
    T1 --> P1[Patterns<br/><i>per-tree registry</i>]
    T1 --> DIR1[Directions<br/><i>upstream trajectories</i>]

    C1 --> S1[Spec A<br/><i>active</i>]
    C1 --> S2[Spec B<br/><i>backlog</i>]
    C1 -.-> A1[Archive<br/><i>completed / deferred</i>]

    S1 --> TK1[t1<br/><i>done</i>]
    S1 --> TK2[t2<br/><i>in_progress</i>]
    S1 --> TK3[t3<br/><i>pending</i>]
    TK1 -->|depends_on| TK2
    TK2 -->|depends_on| TK3

    E --> TH[Threads<br/><i>active workstreams</i>]
    TH -.-> S1

    classDef estate fill:#34495e,stroke:#2c3e50,color:#fff
    classDef tree fill:#2c3e50,stroke:#1a252f,color:#fff
    classDef campaign fill:#7f8c8d,stroke:#616a6b,color:#fff
    classDef spec fill:#4a9eff,stroke:#2670c2,color:#fff
    classDef task fill:#3498db,stroke:#2980b9,color:#fff
    classDef registry fill:#1abc9c,stroke:#16a085,color:#fff
    classDef archive fill:#95a5a6,stroke:#7f8c8d,color:#fff
    classDef thread fill:#f39c12,stroke:#d68910,color:#fff

    class E estate
    class T1,T2,T3 tree
    class C1 campaign
    class S1,S2 spec
    class TK1,TK2,TK3 task
    class SH1,P1,DIR1 registry
    class A1 archive
    class TH thread
```

### Intelligence

Each spec has a landscape: who influences the work, what they've
signaled, and our assessment of their stance. Dispositions and
directions are temporal -- they have validity windows and form
supersession chains.

```mermaid
graph LR
    subgraph "Spec Landscape"
        TK[Task]
        SH[Stakeholder]
        SIG[Signal<br/><i>immutable</i><br/><i>event + record date</i>]
        D1[Disposition<br/><i>valid_from: Mar 1</i><br/><i>stance: cautious</i>]
        D2[Disposition<br/><i>valid_from: Mar 20</i><br/><i>stance: supportive</i>]
    end

    SH -->|influences| TK
    SH -->|signaled| SIG
    SIG -->|evidences| D2
    D1 -->|superseded_by| D2

    subgraph "Upstream Directions"
        DIR[Direction<br/><i>status: committed</i><br/><i>valid_from: Feb 15</i>]
    end

    DIR -->|impacts| TK

    subgraph "Retrospective"
        R[Retro<br/><i>arc + transforms</i>]
        P[Pattern<br/><i>named, transferable</i>]
    end

    TK --> R
    R -->|patterned| P

    classDef task fill:#4a9eff,stroke:#2670c2,color:#fff
    classDef stakeholder fill:#2ecc71,stroke:#27ae60,color:#fff
    classDef signal fill:#f39c12,stroke:#d68910,color:#fff
    classDef disposition fill:#e74c3c,stroke:#c0392b,color:#fff
    classDef direction fill:#1abc9c,stroke:#16a085,color:#fff
    classDef retro fill:#9b59b6,stroke:#7d3c98,color:#fff
    classDef pattern fill:#8e44ad,stroke:#6c3483,color:#fff

    class TK task
    class SH stakeholder
    class SIG signal
    class D1,D2 disposition
    class DIR direction
    class R retro
    class P pattern
```

### Temporal model

Dispositions and directions have validity windows. Signals are
bi-temporal (event time vs record time). When evidence changes an
assessment, the old record is superseded -- not deleted. The full
history is preserved and queryable.

This diagram shows how a disposition evolves as new signals arrive:

```mermaid
sequenceDiagram
    participant Agent
    participant Synthesist as synthesist
    participant DB as Dolt DB

    Note over Agent,DB: Mar 1 -- Initial assessment based on PR comments
    Agent->>Synthesist: signal record (PR comment from Mar 1)
    Synthesist->>DB: Signal s1 {date: Mar 1, recorded: Mar 1}
    Agent->>Synthesist: disposition add --stance cautious --confidence inferred
    Synthesist->>DB: Disposition d1 {stance: cautious, valid_from: Mar 1}

    Note over Agent,DB: Mar 18 -- New review changes our read
    Agent->>Synthesist: signal record (review from Mar 18)
    Synthesist->>DB: Signal s2 {date: Mar 18, recorded: Mar 18}
    Agent->>Synthesist: disposition supersede d1 --new-stance supportive --evidence s2
    Synthesist->>DB: d1.valid_until = Mar 18, d1.superseded_by = d2
    Synthesist->>DB: Disposition d2 {stance: supportive, valid_from: Mar 18, evidence: s2}

    Note over Agent,DB: Mar 25 -- Discover a week-old comment we missed
    Agent->>Synthesist: signal record --date Mar 20 (retroactive)
    Synthesist->>DB: Signal s3 {date: Mar 20, recorded: Mar 25}

    Note over DB: Query: "stance on Mar 10?" → d1 (cautious)<br/>Query: "stance on Mar 19?" → d2 (supportive)<br/>Query: "current stance?" → d2 (valid_until is null)
```

Key properties:
- **Dispositions** are never deleted, only superseded. Full history queryable by date.
- **Signals** are immutable and bi-temporal. `date` is when the event happened.
  `recorded_date` is when we captured it. This matters for retroactive discovery.
- **Directions** follow the same temporal model. An upstream trajectory that moves
  from `proposed` to `committed` creates a new direction record; the old one is
  superseded.
- **`synthesist stance <person>`** resolves the current disposition (valid_until is null).
  **`synthesist stance <person> <topic>`** returns the full supersession chain.

### Node reference

| Node | Scope | Temporal | Description |
|------|-------|----------|-------------|
| **Estate** | global | -- | Top-level switchboard. Lists trees and active threads. |
| **Tree** | estate | -- | Domain of work (upstream, harness, account). |
| **Campaign** | tree | -- | Active and backlog specs within a tree. |
| **Spec** | tree | -- | Unit of work. Contains task DAG and landscape. |
| **Thread** | estate | date | Active workstream pointer. Pruned after 7 days if idle. |
| **Task** | spec | created, completed | DAG node with acceptance criteria. Status: pending, in_progress, done, blocked, waiting. |
| **Retro** | spec | created, completed | Task node (type=retro) with arc, transforms, pattern links. |
| **Stakeholder** | tree | -- | Human actor. Identity registered once per tree. |
| **Signal** | spec | date, recorded_date | Immutable evidence. Bi-temporal: event time vs record time. |
| **Disposition** | spec | valid_from, valid_until | Temporal stance assessment. Supersession chains preserve history. |
| **Direction** | tree | valid_from, valid_until | Upstream technical trajectory. Supersedable. Impacts linked to specs. |
| **Pattern** | tree | first_observed | Named reusable approach. Referenced by retro nodes across specs. |
| **Archive** | tree | archived | Completed/deferred spec record with duration, patterns, contributions. |

### Edge reference

| Edge | From | To | Description |
|------|------|----|-------------|
| `depends_on` | task | task | Execution ordering within a spec DAG |
| `provenance` | task | task | "While doing X we discovered Y" (cross-spec) |
| `influences` | stakeholder | task | Who affects whether this work lands |
| `evidences` | signal | disposition | What evidence supports this assessment |
| `supersedes` | disposition | disposition | Temporal replacement (also direction -> direction) |
| `impacts` | direction | spec | Which specs an upstream trajectory affects |
| `patterned` | retro | pattern | Named approach identified during retrospective |
| `observed_in` | pattern | spec | Where a pattern has been applied |

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

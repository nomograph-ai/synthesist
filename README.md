# Synthesist

A specification graph manager for AI-augmented projects. Synthesist is an
LLM-mediated tool -- the human never calls it directly. The LLM is the
complete interface: it reads estate state, presents plans, gets human
approval, executes work, and reports results. Under the hood, a Go binary
with an embedded Dolt database tracks task DAGs, stakeholder intelligence,
temporal dispositions, and retrospective patterns. LLM agents interact
exclusively through CLI commands -- they never read or write data files
directly.

Named for the role aboard the *Theseus* in Peter Watts' *Blindsight* -- the
one crew member whose job isn't expertise, but coherence.

## Install

### mise (recommended)

```toml
# .mise.toml
[tools."http:synthesist"]
version = "5.3.1"

[tools."http:synthesist".platforms]
macos-arm64 = { url = "https://gitlab.com/api/v4/projects/80084971/packages/generic/synthesist/v{{version}}/synthesist-darwin-arm64", bin = "synthesist" }
linux-x64 = { url = "https://gitlab.com/api/v4/projects/80084971/packages/generic/synthesist/v{{version}}/synthesist-linux-amd64", bin = "synthesist" }
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

# Start a session (required for concurrent work, recommended always)
synthesist session start my-session

# Create a spec with goal and constraints (tree/spec format)
synthesist --session=my-session spec create upstream/auth-service \
  --goal "Migrate auth API from v2 to v3" \
  --constraints "Backward compatible. No breaking changes to existing clients."

# Add tasks to the spec
synthesist --session=my-session task create upstream/auth-service \
  "Research API versioning strategy"

# Claim and complete tasks
synthesist --session=my-session task claim upstream/auth-service t1
# ... do the work ...
synthesist --session=my-session task done upstream/auth-service t1

# Merge session back to main (commits to git)
synthesist session merge my-session

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

    C1 --> S1[Spec A<br/><i>goal, constraints, decisions</i>]
    C1 --> S2[Spec B<br/><i>backlog</i>]
    C1 -.-> A1[Archive<br/><i>completed / deferred</i>]

    S1 --> TK1[t1<br/><i>done</i>]
    S1 --> TK2[t2<br/><i>in_progress</i>]
    S1 --> TK3[t3<br/><i>pending</i>]
    TK1 -->|depends_on| TK2
    TK2 -->|depends_on| TK3

    S1 -.->|propagates_to| S2

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
| **Spec** | tree | created | Unit of work with goal, constraints, and decisions. Contains task DAG and landscape. |
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
| `propagates_to` | spec | spec | "When this spec changes, target needs updates" (ordered) |

## Workflow State Machine

The LLM agent follows a 7-phase state machine when mediating between the
human and synthesist. The human never calls synthesist directly -- the LLM
is the complete interface, and this state machine governs how it behaves.

```mermaid
stateDiagram-v2
    [*] --> ORIENT
    ORIENT --> PLAN : human indicates work
    PLAN --> AGREE : LLM presents plan
    AGREE --> EXECUTE : human approves
    AGREE --> PLAN : human requests changes
    EXECUTE --> REFLECT : task completes
    REFLECT --> EXECUTE : plan holds
    REFLECT --> REPLAN : plan needs changing
    REPLAN --> AGREE : must get concurrence
    REFLECT --> REPORT : all tasks done
    REPORT --> [*]
```

**ORIENT** -- Build a shared mental model of where things stand. The LLM
reads estate state and presents it in plain language. No writes.

**PLAN** -- Model the work before doing it. Create specs and tasks. Task
claims are forbidden in this phase.

**AGREE** -- Explicit human checkpoint. The LLM presents the full plan,
states assumptions, identifies decision points, and waits for approval.
"Ready to proceed?" followed by proceeding without a response is NOT
approval.

**EXECUTE** -- Claim and complete tasks in dependency order. Task creation
is forbidden in this phase (that would be changing the plan without
agreement).

**REFLECT** -- After each task, assess whether the plan still holds. If
it does, continue executing. If not, enter REPLAN.

**REPLAN** -- Modify the plan (add/cancel/rewire tasks), then return to
AGREE. The human must concur with every plan change before execution
resumes.

**REPORT** -- Summarize what was accomplished. Record retrospective
patterns for future transfer.

The `synthesist phase` command lets the agent declare its current phase.
Synthesist validates that attempted operations are allowed in that phase.
See [docs/state-machine.md](docs/state-machine.md) for the full behavioral
contract including display rules, pre-execution protocol, and error
protocol.

## Concurrent Sessions

Multiple LLM sessions can work in the same project simultaneously. Each
session operates on its own Dolt branch, isolating writes until merge.

```mermaid
sequenceDiagram
    participant LLM1 as Claude Session A
    participant Synth as synthesist (Dolt DB)
    participant LLM2 as Claude Session B

    Note over LLM1,LLM2: Both sessions work in the same directory, same .synth/

    LLM1->>Synth: session start session-a
    Synth-->>LLM1: branch created

    LLM2->>Synth: session start session-b
    Synth-->>LLM2: branch created

    par Session A works on spec-alpha
        LLM1->>Synth: --session=session-a task create spec-alpha/t1
        LLM1->>Synth: --session=session-a task claim spec-alpha/t1
        LLM1->>Synth: --session=session-a task done spec-alpha/t1
    and Session B works on spec-beta
        LLM2->>Synth: --session=session-b task create spec-beta/t1
        LLM2->>Synth: --session=session-b task claim spec-beta/t1
        LLM2->>Synth: --session=session-b task done spec-beta/t1
    end

    LLM1->>Synth: session merge session-a
    Synth-->>LLM1: merged to main (row-level)

    LLM2->>Synth: session merge session-b
    Synth-->>LLM2: merged to main (0 conflicts)
```

Key properties of the session model:

- **Dolt branches isolate writes.** Each session operates on its own branch
  of the embedded database. One session's task creation, claims, and
  completions are invisible to other sessions until merge.

- **Merge is row-level, not binary blob.** Unlike git (which would see
  `.synth/` as a binary blob and conflict on every concurrent write), Dolt
  merges at the row level. Two sessions writing to different specs merge
  cleanly with zero conflicts.

- **Atomic task claim prevents double-claiming.** When a session claims a
  task, the claim is atomic within the session's branch. On merge, if two
  sessions claimed the same task, Dolt detects the row-level conflict.

- **Git commits only happen on merge to main.** Individual session
  operations do not create git commits. The merge operation commits both the
  Dolt state and the outer git repository in one step.

- **Sub-agents and parallel execution are supported.** Multiple LLM agents
  (Cursor tabs, Claude Code instances, or framework sub-agents) can each
  start their own session and work concurrently. Assign each agent a
  different spec for zero-contention parallel execution. The Dolt LOCK
  file is process-exclusive but short-lived (<100ms per command); stale
  locks from crashed processes are auto-cleared after 60 seconds.

## Architecture

### Dolt embedded database

The Dolt database lives at `.synth/synthesist/.dolt/` inside the consuming project.
Dolt is an embedded SQL database with git semantics -- content-addressed storage,
branch/merge on data, and table-level diffing.

```
your-project/
├── .synth/                    # Dolt database (created by synthesist init)
│   └── synthesist/.dolt/      # Database files
├── CLAUDE.md                  # tells agent to use synthesist
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

### Kong CLI framework

The command tree is defined as Go structs with Kong struct tags. Kong
parses flags and arguments from the struct definitions, giving typed flag
parsing without a separate flag-registration layer. Wrong flags fail at
compile time, not at runtime. The `synthesist skill` command generates the
LLM skill file from struct reflection -- the skill file is always in sync
with the actual command tree because it reads the same structs that Kong
uses for parsing.

### Binary owns all writes

The `synthesist` binary is the single write path to the database. This enforces:

- Valid state transitions (a task can only go `pending -> in_progress -> done`)
- Referential integrity (a disposition must reference an existing stakeholder)
- Temporal consistency (superseding a disposition sets `valid_until` and creates the replacement atomically)
- Automatic git commits on state changes (configurable with `--no-commit`)

LLMs produce better results when constrained to well-formed operations (Yegge,
Beads 2026). A CLI with typed commands prevents invalid states and handles
computation LLMs are bad at -- temporal resolution, graph traversal, date math.

### LLM-maintainability conventions

The codebase enforces conventions that make it tractable for LLM agents to
navigate and modify:

- **Centralized errors:** All command errors use typed constructors in
  `errors.go`, never inline `fmt.Errorf`. An LLM can read the error
  catalog in one file.
- **Package READMEs:** Each package has a README explaining its purpose,
  dependencies, and key types.
- **Golden tests:** Regression tests in `tests/golden/` with
  `make golden-update` for regeneration.
- **golangci-lint:** errcheck, staticcheck, bodyclose enabled. Zero-warning
  policy enforced by `make lint`.
- **650 LOC limit:** Enforced by `make loc-check`. No non-generated Go
  file exceeds 650 lines. Large files have been split by domain:
  `main.go` into `main.go` (infrastructure) + `cli_types.go` (estate,
  spec, retro, query) + `cli_types_task.go` + `cli_types_landscape.go`;
  `store.go` into `store.go` (core DB) + `store_session.go` (branch ops);
  `cmd_landscape.go` into `cmd_landscape_show.go`, `cmd_disposition.go`,
  `cmd_signal.go`, `cmd_stakeholder.go`, and `cmd_stance.go`;
  `cmd_task.go` into `cmd_task_create.go`, `cmd_task_lifecycle.go`,
  `cmd_task_list.go`, `cmd_task_query.go`, and `cmd_task_helpers.go`;
  `cmd_retro.go` into `cmd_retro_create.go`, `cmd_replay.go`, and
  `cmd_pattern.go`.
- **Zero-warning policy:** The CI pipeline runs `golangci-lint run ./...`
  and `make loc-check`. Any warning or oversized file fails the build.

### Generated skill file

The skill file has two layers:

1. **Command reference** -- generated from Kong struct reflection. Every
   command, flag, and argument is extracted from the same Go structs that
   Kong uses for parsing. The reference cannot drift from the code because
   it is the code.

2. **Authored behavioral rules** -- embedded from
   [docs/state-machine.md](docs/state-machine.md). Phase rules, display
   rules, pre-execution protocol, and error protocol. These are authored
   content that describe how the LLM should behave, not what commands
   exist.

The two layers are concatenated by `synthesist skill` into a single output
that serves as the complete UI specification for LLM consumers.

## The Skill File

`synthesist skill` outputs the complete LLM behavioral contract -- the full
command reference, rules, and usage patterns. This is the primary interface
documentation for agents. The skill file IS the UI specification: it defines
not just what commands exist, but how the LLM should sequence them, what to
show the human, and when to ask for approval.

Install it into any LLM harness by referencing the skill output in your agent
instructions:

```bash
# For Claude Code -- add to CLAUDE.md:
# "Run synthesist skill for the full command reference"

# For any other agent framework:
synthesist skill >> your-agent-config
```

The tool is agent-agnostic. It works with Claude Code, Cursor, or any
framework that gives an LLM access to shell commands.

**Cursor:** see [docs/cursor.md](docs/cursor.md) for how to wire `synthesist skill`,
sessions, and the phase state machine into Cursor rules / project instructions.

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

**Why sessions over git branches?** The `.synth/` directory is a binary blob to
git. Two git branches modifying `.synth/` concurrently will always conflict on
merge -- git cannot diff binary database files. Dolt sessions solve this by
branching the data inside the database, where merge operates at the row level.
Two sessions that touch different specs merge cleanly. Two sessions that touch
the same row produce a structured conflict that the binary can resolve or
surface, rather than a binary blob conflict that requires manual intervention.

**Why a workflow state machine?** Without enforcement, LLMs skip planning and
jump straight to execution. They present a plan and immediately start working
without waiting for human approval. The state machine makes the AGREE phase
mandatory -- the LLM cannot claim tasks until the human has explicitly approved
the plan. This is not a soft guideline; synthesist validates that the current
phase allows the attempted operation.

**Why a generated skill file?** Handwritten documentation drifts from code.
The skill file is generated from the same Kong structs that define the CLI,
so the command reference is always accurate. The behavioral rules are authored
content embedded from `docs/state-machine.md`, but they are versioned alongside
the code and included in the binary's output. One command, one source of truth.

**LLM simulation methodology.** Synthesist embodies a simulation approach to LLM
tool design: constrain the agent to well-formed operations, handle computation
externally, and let the agent focus on reasoning. This aligns with the Beads
framework (Yegge 2026) for structured agent interactions, the Graphiti/Zep
approach to temporal knowledge graphs, and the Howard & Matheson framing of
decision analysis as structured information flow.

## Sources and Influences

Synthesist combines ideas from agent memory systems, temporal knowledge
graphs, decision theory, open source social dynamics, and specification
frameworks. No single prior system unifies task execution with
stakeholder intelligence. The contribution is the combination.

### Task DAGs and agent memory

**[Beads](https://github.com/steveyegge/beads)** (Yegge, 2026) --
git-backed, Dolt-powered task tracker for AI agents. 19.9k stars. Typed
relationships (`relates_to`, `duplicates`, `supersedes`, `dep`), agent
queries via `bd ready --json`. Core insight we adopted: "markdown plans
cost the model GPU cycles to parse; structured, queryable,
dependency-aware data is cheaper and more reliable." Beads tracks
*tasks*. We extend this to track *people*.

**[Gastown](https://github.com/steveyegge/gastown)** (Yegge, 2026) --
multi-agent workspace orchestrator built on Beads. Validated Dolt
embedded as a storage backend for agent coordination. Design principle
we adopted: "findings survive context death." Our retrospective nodes
and pattern registry exist because of this.

**[PlugMem](https://www.microsoft.com/en-us/research/blog/from-raw-interaction-to-reusable-knowledge-rethinking-memory-for-ai-agents/)**
(Microsoft Research, 2025) -- transforms raw agent interactions into
propositional knowledge (facts) and prescriptive knowledge (reusable
skills). Maps directly to our separation of signals (raw observations)
from dispositions (assessed stances) and patterns (transferable
approaches).

### Temporal knowledge graphs

**[Graphiti/Zep](https://github.com/getzep/graphiti)**
([arXiv:2501.13956](https://arxiv.org/abs/2501.13956)) -- bi-temporal
knowledge graph where every edge carries validity windows: when a fact
became true (event time) and when it was recorded (transaction time).
94.8% accuracy on Deep Memory Retrieval benchmark. We adopted the
bi-temporal model directly for dispositions and signals.

**Graph-based Agent Memory survey**
([arXiv:2602.05665](https://arxiv.org/abs/2602.05665), Feb 2026) --
comprehensive taxonomy: knowledge graphs for static facts, temporal
graphs for time-sensitive information, hierarchical structures for task
decomposition, hypergraphs for n-ary relations. Identifies "sentiment
memory" and "user profiling" as categories but has no taxonomy for
stakeholder dynamics in collaborative development. This gap is what we
fill.

**[MAGMA](https://arxiv.org/abs/2601.03236)** (2025) -- four
orthogonal graph structures (semantic, temporal, causal, entity) with
policy-guided traversal. We chose a simpler approach: a single
relational schema with temporal validity on specific node types. The
complexity tradeoff is deliberate -- our consumer is an LLM calling CLI
commands, not a graph reasoning engine.

### Influence and decision theory

**Howard & Matheson** ("Influence Diagrams", Decision Analysis, 2005;
[originally 1981](https://pubsonline.informs.org/doi/10.1287/deca.1050.0020))
-- introduced influence diagrams for the Defense Intelligence Agency to
model political conflicts in the Persian Gulf. Three node types
(decisions, uncertainties, values) with arcs representing informational
influence. Our disposition model borrows the framing: stakeholder stances
are uncertainties that influence contribution strategy decisions.

**Influence maximization on temporal networks**
([Applied Network Science, 2024](https://link.springer.com/article/10.1007/s41109-024-00625-3))
-- the order and timing of interactions matters; influence propagation
differs on time-varying networks versus static ones. This supports our
design decision to make dispositions temporal rather than static.

### Open source social dynamics

**Crowston et al.** ("Social network analysis of open source software",
[IST 2020](https://www.sciencedirect.com/science/article/abs/pii/S0950584920301956))
-- systematic review identifying the temporal gap: "information on how
these structures appear and evolve over time is lacking." Our disposition
supersession chains are a direct response.

**[GitHub Blog](https://github.blog/open-source/maintainers/what-to-expect-for-open-source-in-2026/)**
(2026) -- documents widening gap between participants and maintainers.
"The gap between the number of participants and the number of maintainers
with a sense of ownership widens as new developers grow at record rates."
Confirms the practical need for contributor-side context modeling.

### Strategic mapping

**[Wardley Maps](https://www.wardleymaps.com/read)** (Wardley, 2017)
-- evolution axis models how components change from genesis to commodity.
Our direction nodes serve a similar function at the project level:
tracking where an upstream technology is heading so contributors can
align rather than invest in paths that will be replaced.

**Asahara** ("Beyond Ontologies: OODA Loop Knowledge Graph Structures",
[2025](https://eugeneasahara.com/2025/03/14/beyond-ontologies-ooda-knowledge-graph-structures/))
-- connects Boyd's observe-orient-decide-act cycle to graph query
patterns. Resonates with our cycle: observe (signals), orient
(dispositions), decide (task strategy), act (contribution).

## Building

### Prerequisites

- **Go 1.26+** with CGo enabled
- **ICU libraries** (required by Dolt):
  - macOS: `brew install icu4c@78` (or `brew install icu4c`)
  - Linux: `apt-get install libicu-dev` (Debian/Ubuntu) or `dnf install libicu-devel` (Fedora)
- **golangci-lint** (for `make lint`)

### Build commands

```bash
make build          # Build the binary (./synthesist)
make test           # Run all tests
make install        # Install to $GOPATH/bin
make lint           # golangci-lint (errcheck, staticcheck, bodyclose)
make check          # Build + run synthesist check against local specs
make dev            # Build + show help
make skill          # Build + output the LLM skill file
make golden-update  # Regenerate golden test files (tests/golden/)
make loc-check      # Fail if any non-generated Go file exceeds 650 LOC
make release        # Cross-compile for darwin/arm64, darwin/amd64, linux/amd64, linux/arm64
make clean          # Remove binary and build cache
```

The Makefile auto-detects ICU on macOS via Homebrew and sets the correct
`CGO_CFLAGS`, `CGO_CXXFLAGS`, and `CGO_LDFLAGS`.

## Version History

See [CHANGELOG.md](CHANGELOG.md) for the full history.

- **v5.3.2** (2026-03-31) -- Migrate mise from deprecated ubi: to http: backend with GitLab package registry URLs
- **v5.3.1** (2026-03-31) -- ORIENT mandates landscape/stance queries when stakeholders exist, AGREE requires ecosystem constraints in plan presentation
- **v5.3.0** (2026-03-31) -- Lock retry with backoff for concurrent sessions, scaffold generates Claude Code + Cursor agent configs, session-aware onboarding, merge dry-run, concurrent session protocol docs
- **v5.2.0** (2026-03-30) -- Auto-generated ecosystem audit task on spec create (when tree has stakeholders), ORIENT phase now requires landscape summary before PLAN
- **v5.1.2** (2026-03-30) -- Cursor harness guide (`docs/cursor.md`), contributed by @jmeekhof
- **v5.1.1** (2026-03-29) -- File splitting refactor (main.go → main.go + cli_types*.go, store.go → store.go + store_session.go), fix 35 unchecked DB query errors, fix readOnlySubcommands for `task ready` and `propagation check`, LOC limit 850 → 650
- **v5.1.0** (2026-03-29) -- Disposition `--detail` and `--evidence` flags, `landscape show` includes tree-wide dispositions from stakeholder-preferences pseudo-spec
- **v5.0.1** (2026-03-29) -- Stale Dolt LOCK file detection (auto-clear on Open if >60s old), date-independent golden tests
- **v5.0.0** (2026-03-29) -- Dolt embedded storage, Go CLI binary, temporal specification graphs, Kong CLI framework, concurrent sessions on Dolt branches, workflow state machine, LLM-maintainability refactor (errors.go catalog, package READMEs, golden tests, golangci-lint, LOC limit, file splitting)
- **v4** (2026-03-27) -- Concurrent session support with active threads
- **v3** (2026-03-21) -- Context trees, estate switchboard, campaign coordination
- **v2** (2026-03-18) -- Single primary agent, campaigns, concurrent sessions
- **v1** (2026-03-15) -- Spec format, agent roles, executable acceptance criteria

## License

MIT -- see [LICENSE](LICENSE).

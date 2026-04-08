![synthesist hero](hero.svg)

# Synthesist

[![pipeline](https://gitlab.com/nomograph/synthesist/badges/main/pipeline.svg)](https://gitlab.com/nomograph/synthesist/-/pipelines)
[![license](https://img.shields.io/badge/license-MIT-green)](LICENSE)
![built with GitLab](https://img.shields.io/badge/built_with-GitLab-FC6D26?logo=gitlab)

Specification graph manager for AI-augmented collaborative development.

AI coding agents model codebases but not the humans who maintain them.
The resulting context asymmetry -- where agents can generate code that
compiles and passes tests yet conflicts with architectural direction and
maintainer preferences -- is a representation problem. Agents have no
mechanism to structure, query, or reason about what stakeholders will
accept.

Synthesist fills this gap. It is an LLM-mediated tool: the human never
calls it directly. The LLM reads estate state, builds a shared mental
model with the human, presents plans, gets explicit approval, executes
work, and reports results. Under the hood, a Rust binary with an embedded
SQLite database tracks task DAGs, stakeholder dispositions, temporal
signals, and institutional memory. LLM agents interact exclusively
through CLI commands -- they never read or write data files directly.

Named for the role aboard the *Theseus* in Peter Watts' *Blindsight* --
the one crew member whose job isn't expertise, but coherence.

**Tool**: https://gitlab.com/nomograph/synthesist

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
make build    # requires Rust 1.85+, no other dependencies
```

## Quick Start

```bash
cd your-project
synthesist init
synthesist session start my-session
synthesist --session=my-session --force phase set plan
synthesist --session=my-session --force tree add upstream --description "Upstream project"
synthesist --session=my-session --force spec add upstream/auth --goal "Migrate auth API v2 to v3"
synthesist --session=my-session --force task add upstream/auth "Research API versioning strategy"
synthesist --session=my-session --force task claim upstream/auth t1
synthesist --session=my-session --force task done upstream/auth t1
synthesist session merge my-session
synthesist status
```

All output is JSON. The LLM formats it for human display.

## The Problem

Empirical studies of agent-authored pull requests show that 31% of
rejections are workflow-driven, not quality-driven -- technically correct
code rejected for social misalignment. Evaluations of flat-file context
approaches (AGENTS.md, CLAUDE.md) show they can reduce task success rates
while increasing inference cost by 20%, because agents ingest context
wholesale rather than querying selectively. The problem is not context
quantity but context type: more code context degrades frontier model
performance, while preference context -- what stakeholders will accept --
is absent entirely.

Synthesist addresses context asymmetry by making stakeholder preferences
explicit, queryable, and temporal. Instead of injecting a document of
rules, the agent asks a specific question ("what does this maintainer
think about API versioning?") and receives a specific, evidence-grounded
answer. The savings come not from compression but from asking the right
question before generating the wrong answer.

## Data Model

Estate > Trees > Specs > Tasks.

- **Tree**: a project domain (repository, service, subsystem)
- **Spec**: a unit of work with goal, constraints, and decisions
- **Task**: an atomic work item forming a dependency DAG within a spec
- **Discovery**: a timestamped finding recorded during work
- **Stakeholder**: a person relevant to the work
- **Disposition**: an assessed stance on a specific topic, with confidence
  tiers and temporal supersession chains
- **Signal**: immutable bi-temporal evidence (event time vs record time)
- **Campaign**: cross-tree spec coordination

## Disposition Graphs

The core abstraction is the disposition: a structured representation of
what implementation choices a specific stakeholder will accept on a
specific technical topic. A disposition is not sentiment. It is a
concrete, actionable claim: "on the topic of API versioning, this
maintainer prefers incremental migration over breaking rewrites, based
on evidence from PR #412 review, assessed with documented confidence."

```
Signal (evidence) -> Disposition (assessed stance) -> Agent (aligned contribution)
```

Dispositions are grounded in observable signals (PR comments, review
decisions, design documents), carry confidence tiers that distinguish
documented positions from inferred ones, and form temporal supersession
chains. When new evidence changes an assessment, the old disposition is
superseded -- not deleted. The full history is preserved and queryable.

```
synthesist stakeholder add upstream mwilson --context "lead maintainer"
synthesist signal add upstream/auth mwilson \
  --source "https://gitlab.com/project/-/merge_requests/412" \
  --source-type review \
  --content "Rejected breaking-change approach. Wants backward compatibility."
synthesist disposition add upstream/auth mwilson \
  --topic "migration strategy" --stance opposed --confidence documented \
  --preferred "incremental migration with feature flags"
synthesist stance mwilson
```

A 200-token stance query front-runs the context that would otherwise be
discovered through a multi-round rejection cycle consuming orders of
magnitude more tokens. The savings come from addressing a different axis
of context (preference) than the generation addresses (code).

See the companion paper: "Context Asymmetry Is a Representation Problem:
Disposition Graphs for AI-Augmented Collaborative Development" (Dunn, 2026).

## Workflow State Machine

LLM agents, left unconstrained, skip planning and proceed directly to
code generation. The human's role is reduced to approval or rejection.
The workflow state machine enforces a different pattern: the agent must
build context, obtain agreement, and reflect on outcomes -- with
enforcement that is algorithmic, not advisory.

```
ORIENT -> PLAN -> AGREE -> EXECUTE <-> REFLECT -> REPORT
                                   \-> REPLAN -> AGREE
```

| Phase | Allowed | Purpose |
|-------|---------|---------|
| ORIENT | Read status, query dispositions. No writes. | Build shared mental model from dispositions, prior work, estate state. |
| PLAN | Add tasks/specs, add dependencies. No claims. | Model the work before doing it. |
| AGREE | Present plan. No writes. Block until human approves. | Explicit human checkpoint. The agent halts. |
| EXECUTE | Claim tasks, complete tasks. No task creation. | Do the work in dependency order. |
| REFLECT | Assess plan validity, record discoveries. No claims. | After each task: does the plan still hold? |
| REPLAN | Modify task tree. Returns to AGREE. | Change the plan, then get re-approval. |
| REPORT | Summarize outcomes, record discoveries. | Close the session. |

The critical property is the AGREE gate: the agent cannot transition to
EXECUTE without explicit human approval. This is not "ready to proceed?"
followed by proceeding without a response. The agent presents its full
plan, states assumptions, identifies stakeholder constraints, and waits.
The human may approve, reject, or reshape. The human's modifications to
the plan are themselves signals about what they will accept.

Phase transitions are validated by the CLI. Attempting to claim a task
in PLAN phase returns an error, not a warning.

## Temporal Model

Dispositions and signals have validity windows. Signals are bi-temporal:
event time (when the stakeholder said it) vs record time (when the
contributor captured it). This separation matters for retroactive
discovery -- reading a two-week-old PR comment today.

When evidence changes an assessment, the old disposition is superseded
with a new one. The supersession chain preserves the full history:

```
d1 (cautious, Mar 1) --superseded_by--> d2 (supportive, Mar 20)

Query "stance on Mar 10?" -> d1 (cautious)
Query "current stance?"   -> d2 (supportive, valid_until is null)
```

This model satisfies AGM belief revision postulates (Kumiho, 2026):
new evidence triggers contraction of the old belief and expansion with
the revised assessment, while the full revision history is preserved.

## Sessions

Sessions provide isolation for concurrent work. Each session operates
on its own copy of the database; changes become visible to other sessions
only after merge.

```bash
synthesist session start factory-01
# ... work in the session ...
synthesist session merge factory-01           # three-way merge to main
synthesist session merge factory-01 --dry-run # preview without applying
synthesist session discard factory-01         # abandon changes
```

Merge uses EXCEPT-based three-way diff with primary-key-aware conflict
detection. Two sessions modifying different rows merge cleanly. Conflicts
(both sessions modified the same column of the same row) are reported and
can be resolved with `--ours` (keep main) or `--theirs` (keep session).

## Architecture

- **Storage**: SQLite via rusqlite (bundled). Single static binary, no
  runtime dependencies.
- **Data directory**: `synthesist/` (visible, full name, not hidden)
- **Journal mode**: DELETE (not WAL, for git compatibility)
- **Session isolation**: per-file copies with ATTACH-based merge
- **CLI framework**: clap (derive macros)
- **Skill emission**: `synthesist skill` outputs the complete LLM
  behavioral contract, generalized across execution systems (Claude Code,
  Cursor, IDE extensions)

The binary owns all writes. This enforces valid state transitions
(a task cannot be marked done unless it is in_progress), referential
integrity (a disposition must reference an existing stakeholder), and
temporal consistency (superseding a disposition atomically closes the old
and creates the new).

The architecture is documented in [docs/architecture-v1.md](docs/architecture-v1.md),
including the literature-informed cutline that determines what ships in
v1.0.0 versus what is deferred pending empirical validation.

## The Skill File

`synthesist skill` outputs the complete LLM behavioral contract: the
data model, workflow state machine, command reference with worked
examples, display conventions, and error handling. This is the primary
interface documentation for agents. The skill file is the UI
specification: it defines not just what commands exist, but how the LLM
should sequence them, what to show the human, and when to ask for
approval.

The skill file is execution-system agnostic. It works with Claude Code,
Cursor, or any framework that gives an LLM access to shell commands.

## Design Decisions

**Why a binary at all?** A CLI with typed commands provides a stable API
that decouples storage format from agent interface. The binary handles
computation LLMs are unreliable at -- date arithmetic, temporal queries,
graph traversal, referential integrity -- so agents focus on reasoning.

**Why dispositions?** The delta between proposed implementation and what
a maintainer will accept is the real cost of upstream contributions.
Disposition tracking makes that delta queryable so agents make informed
choices instead of contributing blind.

**Why temporal?** Stakeholder preferences evolve. A maintainer who opposed
an approach in March may accept it in April after a design review changed
their position. Static representations miss this evolution.
Supersession chains preserve the arc.

**Why a workflow state machine?** Without enforcement, LLMs skip planning
and jump to execution. They present a plan and immediately start working
without waiting for human approval. The state machine makes the AGREE
phase mandatory and algorithmic, not advisory.

**Why sessions over git branches for isolation?** The database is a
binary file; git cannot diff or merge it at the row level. Sessions
isolate writes at the application level, where the merge engine
understands primary keys and can perform cell-level three-way merge.

## Building

```bash
make build          # release binary -> ./synthesist
make test           # cargo test
make lint           # cargo clippy -D warnings
make skill          # output LLM skill file
cargo build         # dev build
```

Requires Rust 1.85+. No system dependencies beyond a C compiler (SQLite
is bundled via the `cc` crate).

## License

MIT. See [LICENSE](LICENSE).

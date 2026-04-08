![synthesist hero](hero.svg)

# Synthesist

[![pipeline](https://gitlab.com/nomograph/synthesist/badges/main/pipeline.svg)](https://gitlab.com/nomograph/synthesist/-/pipelines)
[![license](https://img.shields.io/badge/license-MIT-green)](LICENSE)
![built with GitLab](https://img.shields.io/badge/built_with-GitLab-FC6D26?logo=gitlab)

Specification graph manager for AI-augmented collaborative development.

AI coding agents produce technically correct contributions that get
rejected. Studies of agent-authored pull requests find that a third of
rejections are driven by workflow constraints -- scope violations,
architectural misalignment, process expectations -- not code quality.
The agent wrote correct code for the wrong context.

The missing context is not about code. It is about the humans who
govern the code: what they will accept, what direction they are
committed to, and what approaches they have already considered and
rejected. Current tools give agents more information about code
(syntax trees, type systems, call graphs) when the gap is about
people.

Synthesist makes stakeholder preferences explicit, queryable, and
temporal. An agent asks "what does this maintainer think about API
versioning?" and receives a structured, evidence-grounded answer
before writing a line of code. This shifts context acquisition from
the review phase (where rejection is expensive) to the orient phase
(where a query is cheap).

Named for the role aboard the *Theseus* in Peter Watts' *Blindsight* --
the crew member whose job is not expertise, but coherence.

## Install

### mise

```toml
[tools."http:synthesist"]
version = "1.0.0"

[tools."http:synthesist".platforms]
macos-arm64 = { url = "https://gitlab.com/api/v4/projects/80084971/packages/generic/synthesist/v{{version}}/synthesist-darwin-arm64", bin = "synthesist" }
linux-x64 = { url = "https://gitlab.com/api/v4/projects/80084971/packages/generic/synthesist/v{{version}}/synthesist-linux-amd64", bin = "synthesist" }
```

### Source

```bash
git clone https://gitlab.com/nomograph/synthesist.git
cd synthesist && make build
```

Requires Rust 1.88+. No system dependencies beyond a C compiler.

## How It Works

Synthesist is an LLM-mediated tool. The human interacts with an LLM
agent; the agent interacts with synthesist. The human never calls
synthesist directly. The LLM reads state, builds a shared mental
model, presents plans, obtains approval, executes work, and reports
results. The binary enforces structure on this process.

```bash
synthesist init
synthesist session start work
export SYNTHESIST_SESSION=work

# Orient: read the landscape
synthesist --force phase set plan
synthesist status
synthesist stance mwilson

# Plan: model the work
synthesist spec add upstream/auth --goal "Migrate auth API v2 to v3"
synthesist task add upstream/auth "Research versioning strategy"
synthesist task add upstream/auth "Implement migration" --depends-on t1
synthesist task add upstream/auth "Write tests" --depends-on t2 --gate human

# Agree: present to human, wait for approval
synthesist phase set agree

# Execute: do the work in dependency order
synthesist phase set execute
synthesist task claim upstream/auth t1
synthesist task done upstream/auth t1
synthesist task ready upstream/auth    # shows t2 is now unblocked

# Report and merge
synthesist phase set report
synthesist session merge work
```

## Disposition Graphs

The core abstraction is the disposition: a structured representation
of what implementation choices a specific stakeholder will accept on a
specific technical topic. A disposition is not sentiment. It is a
concrete claim grounded in observable evidence:

> On the topic of API versioning, this maintainer prefers incremental
> migration over breaking rewrites, based on evidence from PR #412
> review, assessed with documented confidence.

Dispositions are scoped to topics (not globally applied), grounded in
signals (PR comments, review decisions, design documents), carry
confidence tiers (documented, verified, inferred, speculative), and
form temporal supersession chains. When new evidence changes an
assessment, the old disposition is superseded -- never deleted. The
full history is preserved and queryable.

```bash
# Record evidence
synthesist signal add upstream/auth mwilson \
  --source "https://gitlab.com/project/-/merge_requests/412" \
  --source-type review \
  --content "Rejected breaking-change approach. Wants backward compat."

# Assess the stance
synthesist disposition add upstream/auth mwilson \
  --topic "migration strategy" --stance opposed --confidence documented \
  --preferred "incremental migration with feature flags"

# Query before contributing
synthesist stance mwilson
# Returns structured JSON: stance, topic, confidence, evidence chain
```

The separation of signal from disposition mirrors the structure of
implicit feedback systems: signals are the objective record (what
someone said), dispositions are the interpretive assessment (what we
believe they will accept). The confidence tier makes the epistemic
status explicit -- an inferred disposition from three signals is
different from a documented position stated in a design review.

See the companion paper: "Context Asymmetry Is a Representation
Problem: Disposition Graphs for AI-Augmented Collaborative
Development" (Dunn, 2026).

## Workflow State Machine

LLM agents left unconstrained skip planning and proceed directly to
code generation. The workflow state machine enforces a different
pattern with algorithmic enforcement -- the CLI rejects operations
that violate the current phase.

| Phase | What happens | What is forbidden |
|-------|-------------|-------------------|
| ORIENT | Read status, query dispositions, read discoveries. Build a shared mental model from the disposition landscape. | All writes. |
| PLAN | Create specs and tasks, define dependencies, research. | Task claims. No executing before agreeing. |
| AGREE | Present the plan. State assumptions. Surface stakeholder constraints. Halt and wait for human approval. | All writes. The agent stops. |
| EXECUTE | Claim and complete tasks in dependency order. | Task creation or cancellation. The plan is fixed. |
| REFLECT | After each task, assess: does the plan still hold? Record discoveries. | Task claims. Step back before stepping forward. |
| REPLAN | Modify the task tree. Returns to AGREE -- the human must re-approve. | Task claims. Changed plans need fresh consent. |
| REPORT | Summarize outcomes, record institutional memory, close the session. | -- |

The critical property is AGREE. The agent presents its full plan,
identifies which tasks need human gates, surfaces relevant stakeholder
dispositions, and waits. The human may approve, reject, or reshape.
The human's modifications are themselves signals about preference.

Phase transitions are validated:

```
synthesist phase set execute
# error: invalid phase transition: plan -> execute (valid: agree)
```

## Temporal Model

Dispositions and signals carry validity windows. When a stakeholder
changes position, the old disposition is superseded with a new one:

```
d1 (cautious, Mar 1) --superseded_by--> d2 (supportive, Mar 20)

"stance on Mar 10?" -> d1 (cautious)
"current stance?"   -> d2 (valid_until is null)
```

Signals are bi-temporal: `date` is when the event happened, `recorded_date`
is when it was captured. A PR comment from two weeks ago discovered today
has both dates -- this matters for reconstructing the order of evidence
acquisition vs the order of stakeholder action.

## Sessions

Sessions provide isolation for concurrent work. Each session operates
on its own copy of the database. Changes are invisible to other sessions
until merge.

```bash
synthesist session start research
# writes go to sessions/research.db, reads see session data
synthesist session merge research          # three-way merge to main
synthesist session merge research --dry-run  # preview
synthesist session discard research        # abandon
```

Merge is PK-aware: two sessions modifying different rows merge cleanly.
Conflicts (same row, same column, different values) are reported with
`--ours` / `--theirs` resolution.

## Architecture

Rust binary with embedded SQLite. Single static binary, no runtime
dependencies. Data directory is `synthesist/` (visible, not hidden).

The binary owns all writes. State transitions are enforced (a task
cannot be marked done unless it is in_progress), referential integrity
is maintained (a disposition must reference an existing stakeholder),
and temporal consistency is guaranteed (superseding a disposition
atomically closes the old and creates the new). LLMs produce better
results when constrained to well-formed operations.

The schema is documented in [docs/architecture-v1.md](docs/architecture-v1.md),
including the literature-informed cutline that separates validated
features from deferred ones, with re-entry criteria for each.

## The Skill File

`synthesist skill` outputs the complete behavioral contract: data
model, workflow state machine, command reference with worked examples,
error handling, and display conventions. This is the primary interface
for LLM agents. It is execution-system agnostic -- works with Claude
Code, Cursor, or any framework that gives an LLM shell access.

## Building

```bash
make build    # release binary
make test     # integration tests
make lint     # clippy -D warnings
make skill    # emit skill file
```

## License

MIT. See [LICENSE](LICENSE).

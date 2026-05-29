# Implementation Plan: v3.0-alpha (Companion to Proposal 002)

**Status**: Draft
**Date**: 2026-05-28
**Companion to**: `docs/proposals/002-graph-substrate.md`
**Author**: Andrew Dunn

## How to use this document

This document decomposes the v3.0-alpha work into discrete, independently
implementable tasks suitable for parallel dispatch to a fleet of worker
agents. The coordinator role holds the plan and verifies completion; the
worker role picks one task, executes it against a single branch, and
reports.

**Coordinator role**: read proposal 002 in full, then this document.
Dispatch tasks per the phase ordering. Verify acceptance criteria before
marking a task complete. Resolve cross-task questions when workers
surface them.

**Worker role**: you may be dispatched with only a single task block from
this document plus the relevant sections of proposal 002. The task block
is self-contained. You do not need the full conversation history. If
context is missing, surface the gap rather than guessing.

## Conventions

- **Task ID**: `T<phase>.<index>`, e.g., `T1.3`.
- **Branch naming**: `v3-alpha/<task-id>-<kebab-summary>`,
  e.g., `v3-alpha/T1.3-log-reader`.
- **Commit prefix**: `[v3-alpha T1.3]` so the dispatch trail is grep-able.
- **PR title prefix**: same.
- **Dependencies**: declared per task. A task with `requires: T1.2` cannot
  start until T1.2 is merged.
- **Concurrent-safe**: tasks within the same phase that share no `requires`
  can be dispatched in parallel.
- **Reviewer**: every task PR requires Andrew or Josh review before merge.
  Workers do not merge their own PRs.
- **Test discipline**: every task ships with tests. Tests are the
  acceptance evidence; the coordinator verifies by reading the tests, not
  by re-running the implementation.
- **No em dashes** in code comments, docs, or commit messages.

## Branch and merge model

- All v3-alpha work happens on a long-lived integration branch:
  `v3-alpha-integration`. Cut from `main` at the start of Phase 1.
- Worker PRs target `v3-alpha-integration`, not `main`.
- When v3.0-alpha.0 is ready to tag, `v3-alpha-integration` merges to
  `main` as one squashed commit per phase or as a multi-commit fast-forward,
  at the proposal owner's discretion.
- `main` continues to receive v2.5.x patches independently if needed
  during the v3 build.

## Task status legend

- `TODO` -- not started
- `READY` -- dependencies met, can be dispatched
- `IN PROGRESS` -- a worker has claimed it
- `IN REVIEW` -- PR open, awaiting review
- `DONE` -- merged to `v3-alpha-integration`
- `BLOCKED` -- needs the coordinator to resolve

The coordinator maintains the live status in a separate file
(`v3-alpha-status.json` at repo root, gitignored) so this document stays
the contract.

---

# Phase 1: Substrate runtime (`nomograph-claim`)

Goal: a Rust crate that reads, writes, and indexes JSON-LD claim logs.
Does not depend on any synthesist code. Used by synthesist (and any
future module) as the substrate API.

**Coordinator note**: T1.1 must complete before any other Phase 1 task.
After T1.1, T1.2 through T1.5 can run concurrently. T1.6 and T1.7 wait
for the others.

## T1.1: JSON-LD compact form specification

**Status**: TODO
**Requires**: nothing.
**Branch**: `v3-alpha/T1.1-jsonld-spec`

**Scope**: Define and document the compact JSON-LD form synthesist will
write. Produce a constants module in `nomograph-claim/src/jsonld.rs` plus
a markdown spec document.

**Inputs**:
- Proposal 002 sections "On-disk layout" and "Schema".
- The `@context` payload exemplified in the spike at
  `keaton/research/graph-primitive/spike-v3-oxigraph/src/bin/spike.rs`
  lines 23 to 37.

**Outputs**:
- `nomograph-claim/src/jsonld.rs` exposing `pub const BASE_CONTEXT_URI`,
  `pub const BASE_CONTEXT_BODY` (the inline @context JSON), and
  `pub fn base_context_value() -> serde_json::Value`.
- `nomograph-claim/docs/jsonld-form.md` documenting the rules:
  compact form, IRI prefixes, datatype coercion, supersession predicate
  per module, asserter IRI format, prov predicates.

**Acceptance criteria**:
- `BASE_CONTEXT_BODY` parses as valid JSON-LD context per a round-trip
  test using `oxjsonld`.
- The doc names every IRI prefix (`synth`, `prov`, `xsd`, `nomograph`)
  and the form of `@id` for claims (`<module>:claim/<hash>`).
- Test: build a minimal JSON-LD doc using only the base context, parse
  it with oxjsonld, assert the resulting triples include
  `prov:generatedAtTime` and `prov:wasAttributedTo` correctly typed.

**Gotchas**:
- Compact form keeps human readability. Do not switch to expanded form
  even if intermediate code is simpler with it.
- The base context covers only universal predicates. Per-module
  predicates (e.g., `synth:depends_on`) live in synthesist's context file
  and are layered on at write time.

---

## T1.2: Log writer (append claim to per-asserter log)

**Status**: TODO
**Requires**: T1.1.
**Branch**: `v3-alpha/T1.2-log-writer`

**Scope**: Implement `LogWriter` in `nomograph-claim/src/log.rs` that
appends one JSON-LD doc per call to the appropriate
`claims/<asserter>/log.jsonl` file.

**Inputs**:
- Proposal 002 section "Multi-user model".
- T1.1's base context.

**Outputs**:
- `pub struct LogWriter { claims_dir: PathBuf }`
- `pub fn new(claims_dir: &Path) -> Result<Self>`
- `pub fn append(&self, asserter: &str, doc: &serde_json::Value) -> Result<ClaimId>`

The writer:
- Validates the doc is a JSON-LD object with `@id`, `@type`,
  `prov:generatedAtTime`, `prov:wasAttributedTo`.
- Routes by asserter: writes to `claims/<asserter>/log.jsonl`. Creates
  the asserter directory if missing.
- Uses atomic write (write to `.tmp` then rename, fsync the directory
  after).
- Appends one line per claim. Always trailing newline.
- Returns the claim id (`@id` value).

**Acceptance criteria**:
- Test: append 100 claims with the same asserter, read the file back,
  count 100 lines, verify each parses as JSON-LD.
- Test: append claims for two different asserters in interleaved order,
  verify two files exist with correct line counts each.
- Test: kill the writer mid-call (use a test that interrupts before
  the rename), verify the log file is either at the prior state or at
  the new state, never partial.

**Gotchas**:
- The atomic-write contract is load-bearing for crash safety. Do not
  skip it.
- Asserter strings can contain colons (`user:local:agd:edc-bootstrap`).
  The directory name is the full asserter string with colons replaced
  by hyphens or kept as is depending on filesystem. Pick one; document
  it; cover with a test on macOS and Linux.

---

## T1.3: Log reader (iterate claims from log union)

**Status**: TODO
**Requires**: T1.1.
**Branch**: `v3-alpha/T1.3-log-reader`

**Scope**: Implement `LogReader` in `nomograph-claim/src/log.rs` that
walks all `<asserter>/log.jsonl` files under a claims directory and
yields one claim at a time.

**Outputs**:
- `pub struct LogReader { claims_dir: PathBuf }`
- `pub fn new(claims_dir: &Path) -> Result<Self>`
- `pub fn iter_claims(&self) -> impl Iterator<Item = Result<Claim>>`

Where `Claim` is:
- `pub struct Claim { pub id: String, pub raw: serde_json::Value }`
- the raw value is the parsed JSON-LD doc.

The reader:
- Lists all `<asserter>` subdirectories under `claims/`.
- For each, opens `log.jsonl` and yields one line per claim.
- Order: claim files are walked in a deterministic order
  (asserter name lexicographic, then by line order). Document this.

**Acceptance criteria**:
- Test: write 50 claims via `LogWriter` across 3 asserters, then
  `LogReader::iter_claims()` yields all 50 with no duplicates.
- Test: a malformed line in the middle of a log file produces a `Result`
  error for that line; iteration continues for subsequent lines.
- Test: an empty `claims/` directory yields zero claims with no error.

**Gotchas**:
- The reader yields per-file order, not per-time order. Time ordering
  across asserters comes from sorting on `prov:generatedAtTime` at the
  view layer, not here.
- Genesis claim handling: `genesis.jsonld` is at the top level of
  `claims/`, not in an asserter subdirectory. Include it in the
  iteration with a fixed pseudo-asserter `bootstrap`.

---

## T1.4: Per-asserter routing

**Status**: TODO
**Requires**: T1.1.
**Branch**: `v3-alpha/T1.4-asserter-routing`

**Scope**: Implement asserter parsing and validation in
`nomograph-claim/src/asserter.rs`.

**Outputs**:
- `pub struct Asserter { class: AsserterClass, scope: String, id: String, session: Option<String> }`
- `pub enum AsserterClass { User, Agent, Ingest }`
- `pub fn parse(s: &str) -> Result<Asserter>`
- `pub fn to_iri(&self) -> String` returning the `asserter:` IRI form.
- `pub fn dir_name(&self) -> String` returning a filesystem-safe
  directory name for use by the log writer.

Asserter string format:
- `user:<scope>:<id>[:<session>]`
- `agent:<scope>:<id>[:<session>]`
- `ingest:<scope>:<id>`

Where `<scope>` is typically `local`, a forge name (`gitlab`, `github`),
or an adapter name.

**Acceptance criteria**:
- Test: round-trip parse + to_iri for a dozen representative strings.
- Test: invalid forms produce structured errors naming the field.
- Test: `dir_name()` for the same asserter produces the same directory
  on macOS and Linux.

**Gotchas**:
- The session suffix is optional. `user:local:agd` and
  `user:local:agd:edc-bootstrap` are both valid; the former is a top-level
  user identity, the latter is that user under a named synthesist session.
- Asserter strings are user-visible (appear in git logs, in
  `synthesist status`). Do not transform them for storage convenience;
  store as written.

---

## T1.5: PROV-O constants and helper

**Status**: TODO
**Requires**: T1.1.
**Branch**: `v3-alpha/T1.5-prov-constants`

**Scope**: Tiny module providing PROV-O IRI constants used across the
substrate.

**Outputs**:
- `nomograph-claim/src/prov.rs` with `pub const GENERATED_AT_TIME`,
  `pub const WAS_ATTRIBUTED_TO`, `pub const WAS_REVISION_OF`, and
  helper `pub fn now_iso() -> String` returning the current time in
  the canonical xsd:dateTime form synthesist uses.

**Acceptance criteria**:
- Test: `now_iso()` produces a string parseable as RFC 3339 / xsd:dateTime
  with millisecond precision and trailing `Z`.

**Gotchas**:
- Use millisecond precision to match v2.5's `asserted_at` ms-precision
  history. The migration tool relies on this.

---

## T1.6: Embedded base ontology

**Status**: TODO
**Requires**: T1.1, T1.4, T1.5.
**Branch**: `v3-alpha/T1.6-embedded-ontology`

**Scope**: Author the base ontology Turtle file and embed it in the
crate via `include_str!()`.

**Outputs**:
- `nomograph-claim/ontology/base.ttl` declaring the `nomograph:` vocabulary:
  `Asserter`, `Supersedes`, `AssertedAt`, `ParentAsserter`, the asserter
  class taxonomy (`User`, `Agent`, `Ingest`).
- `nomograph-claim/ontology/base.shacl.ttl` with structural shapes for
  the universal envelope: every claim must have `@id`, `@type`,
  `prov:generatedAtTime`, `prov:wasAttributedTo`.
- `nomograph-claim/src/ontology.rs` exposing `pub const BASE_TTL`,
  `pub const BASE_SHACL_TTL` via `include_str!()`.

**Acceptance criteria**:
- Test: `BASE_TTL` parses with `oxigraph::io::RdfParser` as valid Turtle.
- Test: `BASE_SHACL_TTL` parses similarly.
- Test: the substrate's `serialize_ontology()` helper writes both to a
  given directory.

**Gotchas**:
- Keep the base small. Anything specific to synthesist or to a future
  module does not belong here. Resist the urge to add `Claim` as a base
  class; the type system per-module names its own root.

---

## T1.7: Storage layer integration test

**Status**: TODO
**Requires**: T1.2, T1.3, T1.4, T1.5, T1.6.
**Branch**: `v3-alpha/T1.7-storage-integration`

**Scope**: End-to-end test for the storage layer alone.

**Outputs**:
- `nomograph-claim/tests/storage.rs` covering:
  - Init a fresh claims dir.
  - Append 200 claims across 5 asserters.
  - Read them back via `LogReader::iter_claims()`.
  - Verify count, asserter distribution, claim IDs.
  - Verify file layout matches the spec.

**Acceptance criteria**:
- Test passes under `cargo test --package nomograph-claim`.
- No external deps beyond what's already in the workspace.

---

# Phase 2: Query layer (Oxigraph integration)

Goal: layer Oxigraph onto the storage layer so the substrate exposes
SPARQL queries against the log union.

**Coordinator note**: T2.1 must complete before any other Phase 2 task.
T2.2 through T2.5 can run concurrently after T2.1.

## T2.1: Oxigraph dependency and store initialization

**Status**: TODO
**Requires**: T1.7.
**Branch**: `v3-alpha/T2.1-oxigraph-init`

**Scope**: Add Oxigraph to `nomograph-claim/Cargo.toml` and write the
store initialization helper.

**Outputs**:
- `nomograph-claim` depends on `oxigraph = "0.4"` and `oxjsonld = "0.1"`.
- `nomograph-claim/src/view.rs` with `pub struct View { store: Store }`,
  `pub fn open(view_dir: &Path) -> Result<Self>`, and
  `pub fn open_in_memory() -> Result<Self>`.

**Acceptance criteria**:
- Test: open a view in a temp dir, close, reopen, verify it persists.
- Test: open an in-memory view, verify it does not touch disk.

**Gotchas**:
- The on-disk view lives at `claims/_view.oxigraph/`. It is gitignored.
- Oxigraph's RocksDB backend creates many small files; do not let any
  of them slip into git.

---

## T2.2: View rebuild from log union

**Status**: TODO
**Requires**: T2.1.
**Branch**: `v3-alpha/T2.2-view-rebuild`

**Scope**: Implement the function that reads all asserter logs and loads
them into the Oxigraph store.

**Outputs**:
- `pub fn rebuild(view: &View, claims_dir: &Path) -> Result<RebuildStats>`
- `pub struct RebuildStats { claims_loaded: usize, triples_count: u64, duration_ms: u64 }`

The function:
- Clears the view (drops all triples).
- Iterates `LogReader::iter_claims()`.
- For each claim, parses the JSON-LD doc and inserts into the store.
- Records stats.

**Acceptance criteria**:
- Test: rebuild against a 100-claim log produces a view with the expected
  triple count (claims_count * 7 to 10, since 7.7 triples per claim is the
  spike measurement).
- Test: rebuild is idempotent (calling rebuild twice yields the same view).
- Test: rebuild against an empty claims dir produces an empty view with
  no error.

**Gotchas**:
- Use the @context inlining trick from the spike: rather than relying on
  Oxigraph to resolve the @context URI over the network, parse each line
  as JSON, inject the inline @context from T1.1, and re-serialize before
  handing to Oxigraph. The spike does this at lines 54 to 78.

---

## T2.3: SPARQL query interface

**Status**: TODO
**Requires**: T2.1.
**Branch**: `v3-alpha/T2.3-sparql-query`

**Scope**: Expose SPARQL SELECT and ASK queries on the View.

**Outputs**:
- `pub fn select(view: &View, query: &str) -> Result<SelectResults>`
- `pub fn ask(view: &View, query: &str) -> Result<bool>`
- `pub struct SelectResults { columns: Vec<String>, rows: Vec<Vec<Term>> }`

Where `Term` is a normalized form: IRIs as strings, literals as
`{value, datatype, language?}` structs.

**Acceptance criteria**:
- Test: load 10 claims, run the status-shape query from the spike (count
  by type), verify the result.
- Test: run a query that returns zero results, verify empty rows.
- Test: malformed SPARQL produces a structured error.

**Gotchas**:
- Do not expose raw Oxigraph types in the public API. Use the normalized
  `Term` so we can swap the backend in gamma without breaking callers.

---

## T2.4: Named graph routing

**Status**: TODO
**Requires**: T2.1.
**Branch**: `v3-alpha/T2.4-named-graphs`

**Scope**: Route claim insertion into per-module named graphs based on
the claim's `@type` IRI prefix.

**Outputs**:
- Modify the rebuild path from T2.2 to detect each claim's namespace
  (e.g., `synth:Task` goes into the `<https://nomograph.org/graphs/synth>`
  named graph).
- `pub fn modules_in_view(view: &View) -> Result<Vec<String>>` returning
  the named graph IRIs present.

**Acceptance criteria**:
- Test: load claims from two namespace prefixes, verify both named graphs
  appear in `modules_in_view`.
- Test: a SPARQL query against `GRAPH <https://nomograph.org/graphs/synth>`
  returns only synth claims.

**Gotchas**:
- The default graph (`SELECT ?s ?p ?o WHERE { ?s ?p ?o }`) returns the
  union across all named graphs. Document this.

---

## T2.5: View staleness check (heads file)

**Status**: TODO
**Requires**: T2.2.
**Branch**: `v3-alpha/T2.5-heads-check`

**Scope**: Maintain a `claims/_view.heads` file that records the hash of
the log union. On open, compare against the current log union; if they
differ, the view is stale and needs rebuild.

**Outputs**:
- `pub fn current_heads(claims_dir: &Path) -> Result<String>` computing
  a blake3 hash over the sorted list of asserter dirs and their
  per-file line counts.
- `pub fn heads_match(view_dir: &Path, claims_dir: &Path) -> Result<bool>`
- `pub fn write_heads(view_dir: &Path, claims_dir: &Path) -> Result<()>`

**Acceptance criteria**:
- Test: after rebuild, `heads_match` returns true.
- Test: after appending one claim, `heads_match` returns false.
- Test: after re-rebuilding, `heads_match` returns true again.

**Gotchas**:
- The heads computation must be deterministic. Sort asserter directory
  names before hashing.
- This is what `view.heads` in v2.5 does for the SQLite cache. Same
  pattern.

---

## T2.6: Query layer integration test

**Status**: TODO
**Requires**: T2.2, T2.3, T2.4, T2.5.
**Branch**: `v3-alpha/T2.6-query-integration`

**Scope**: End-to-end test exercising the query layer.

**Outputs**:
- `nomograph-claim/tests/query.rs` covering:
  - Build claims, rebuild view, run SPARQL queries from the spike,
    verify results.
  - Detect staleness after append, rebuild, verify heads match.

**Acceptance criteria**:
- All queries from the spike (status, task-list, asserter audit) work
  correctly.
- Test runs in under 10 seconds.

---

# Phase 3: Synthesist CLI on new substrate

Goal: repoint synthesist's CLI from `nomograph-workflow` 0.3 to the new
`nomograph-claim` v3. Carry forward all v2.5 imperative validators.

**Coordinator note**: T3.1 is the critical path. T3.2 and T3.3 are
parallelizable after T3.1. T3.4, T3.5, T3.6 follow.

## T3.1: Substrate dependency swap

**Status**: TODO
**Requires**: T2.6.
**Branch**: `v3-alpha/T3.1-substrate-swap`

**Scope**: Replace `nomograph-claim = "0.2"` and `nomograph-workflow =
"0.3"` in `synthesist/Cargo.toml` with the v3 `nomograph-claim` (path
dep during alpha, registry dep later). Update `src/store.rs` (the
`SynthStore` wrapper) to use the new substrate API.

**Outputs**:
- `Cargo.toml` updated.
- `src/store.rs` rewritten to use `LogWriter`, `LogReader`, `View`.
- All call sites of the old `nomograph_workflow::Store` API compile.
- The `nomograph_workflow` dep is dropped entirely.

**Acceptance criteria**:
- `cargo build --release` succeeds for synthesist with the new substrate.
- All v2.5 CLI commands compile and exit cleanly on `--help`.
- Compile-time only at this stage; runtime tests come in T3.6.

**Gotchas**:
- `nomograph_workflow::Store::with_asserter()` becomes a method on
  `SynthStore` that captures the asserter for subsequent appends; the
  log writer needs the asserter on each call.
- `parse_tree_spec`, `today`, `json_out` and similar workflow re-exports
  move into synthesist directly. They were in `nomograph-workflow`;
  inline them.

---

## T3.2: Imperative validators carry-forward

**Status**: TODO
**Requires**: T3.1.
**Branch**: `v3-alpha/T3.2-validators`

**Scope**: Adapt the existing v2.5 imperative validators in
`src/schema/*.rs` to the new substrate. The validation logic itself
does not change; only the call sites and the props extraction do.

**Outputs**:
- `src/schema/mod.rs` updated to take `&serde_json::Value` (the JSON-LD
  props) instead of the v2 `Claim` struct.
- Each per-type validator (`spec.rs`, `task.rs`, `discovery.rs`, etc.)
  updated to expand `synth:` prefixes when reading field values.
- DAG cycle detection in `task_dag.rs` updated to query the new view.
- Phase state machine in `cmd_phase.rs` unchanged in logic; only the
  read path changes.

**Acceptance criteria**:
- All existing schema tests pass against the new substrate.
- The phase state machine rejects invalid transitions with the same
  error messages as v2.5.

**Gotchas**:
- JSON-LD compacts predicates; when reading `props["synth:status"]`,
  workers need to handle both compacted (`synth:status`) and expanded
  (`https://nomograph.org/synth/status`) forms. Pick one and normalize
  on read.
- The current `nomograph_claim::ClaimType` enum from v2 stays in spirit
  as the per-type dispatch. Move it into synthesist or replace with a
  string-keyed match.

---

## T3.3: SHACL Turtle emitter

**Status**: TODO
**Requires**: T3.1.
**Branch**: `v3-alpha/T3.3-shacl-emitter`

**Scope**: A build-step that emits `_schema/synth.shacl.ttl` from the
Rust schema definitions in `src/schema/*.rs`. The Turtle is documentation,
not a runtime gate.

**Outputs**:
- `src/bin/emit-shacl.rs` (a synthesist binary, not part of the main CLI)
  that walks the schema modules and prints a SHACL Turtle document to
  stdout.
- `make shacl` target that runs the binary and writes to
  `synthesist/ontology/synth.shacl.ttl`.
- The generated Turtle includes one `sh:NodeShape` per claim type, with
  required predicates, cardinality, and value-range constraints derived
  from the Rust schema.

**Acceptance criteria**:
- `make shacl` produces a file that parses as valid Turtle (use
  `oxttl` or the spike's parser).
- The generated shapes contain entries for every claim type synthesist
  defines.
- Manual inspection: a Task shape declares `synth:status` with
  `sh:in (pending in_progress done cancelled blocked waiting)`.

**Gotchas**:
- This is structural-only. Behavioral constraints (DAG cycles, phase
  state machine, asserter scope) do not translate to SHACL and stay in
  the imperative validators.
- The emitter is run during the release pipeline, not on every CLI
  invocation. It is part of the docs, not the binary.

---

## T3.4: Synthesist skill file regeneration

**Status**: TODO
**Requires**: T3.2, T3.3.
**Branch**: `v3-alpha/T3.4-skill-regen`

**Scope**: Update `src/skill.rs` to regenerate the skill file from the
new substrate's surface. References the SHACL Turtle artifact from T3.3.

**Outputs**:
- `synthesist skill` produces the v3-shaped skill document.
- The skill file references the `synth.shacl.ttl` schema.
- All v2.5 commands and conventions remain documented.

**Acceptance criteria**:
- `synthesist skill --help` runs cleanly.
- The output skill file parses as markdown and contains every CLI
  command's usage block.

**Gotchas**:
- The skill file is referenced by external harnesses. Do not change its
  top-level structure without coordination.

---

## T3.5: `synthesist serve` repoint

**Status**: TODO
**Requires**: T3.2.
**Branch**: `v3-alpha/T3.5-serve-repoint`

**Scope**: Repoint `src/cmd_serve.rs` (the HTML dashboard) at the new
substrate. The SSE filesystem watcher now watches
`claims/<asserter>/log.jsonl` files rather than `claims/changes/*.amc`.

**Outputs**:
- The watcher detects log file changes and pushes SSE events.
- The HTML views (trees and network) render correctly against the new
  substrate.

**Acceptance criteria**:
- `synthesist serve` against a populated claims tree renders the spec
  tree view identically to v2.5.
- Appending a claim triggers an SSE event within 1 second.

**Gotchas**:
- Watching JSONL files for append events is simpler than the .amc
  hash-named churn. Read the new lines tail-style.

---

## T3.6: CLI integration test

**Status**: TODO
**Requires**: T3.2, T3.3, T3.4, T3.5.
**Branch**: `v3-alpha/T3.6-cli-integration`

**Scope**: End-to-end test exercising the synthesist CLI on the new
substrate. Mirrors `tests/integration.rs` from v2.5.

**Outputs**:
- `tests/integration.rs` running all v2.5 happy-path scenarios:
  `init`, `session start`, `tree add`, `spec add`, `task add`,
  `task claim`, `task done`, `task ready`, `status`, `phase set`,
  `discovery add`, `session close`.

**Acceptance criteria**:
- All scenarios pass.
- The CLI surface is byte-identical to v2.5 (same JSON shapes, same
  exit codes).

---

# Phase 4: Migration tool

Goal: a one-shot tool that reads v2.5 `.amc` files and emits v3 JSON-LD
per-asserter logs. Idempotent, dry-run first, validated.

**Coordinator note**: T4.1 and T4.2 can run concurrently. T4.3 through
T4.6 follow.

## T4.1: v2 .amc reader

**Status**: TODO
**Requires**: T3.6.
**Branch**: `v3-alpha/T4.1-v2-reader`

**Scope**: A reader that takes a path to a v2.5 `claims/` directory and
yields claims in causal order.

**Outputs**:
- `synthesist/src/migrate/v2_reader.rs` with
  `pub fn read_v2_claims(claims_dir: &Path) -> Result<Vec<V2Claim>>`.
- `V2Claim` mirrors the v2 schema: `id, claim_type, props, asserted_at,
  asserted_by, supersedes, valid_from?, valid_until?`.

**Acceptance criteria**:
- Test against the storr corpus: 143 claims read in causal order.
- The asserter distribution matches what `synthesist sql` reports:
  edc-bootstrap 123, agd 13, andrewdunn 3, andrewdunn:overnight-deploy 2,
  secondbreakfast 1, cms-t5 1.

**Gotchas**:
- Use `automerge = "0.8"` (v2's substrate). Add as a one-shot dep for
  the migration tool only; remove from synthesist after migration is
  shipped.
- "Causal order" comes from walking the Automerge document's change
  history, not from `asserted_at`.

---

## T4.2: v2 to v3 claim translation

**Status**: TODO
**Requires**: T4.1.
**Branch**: `v3-alpha/T4.2-claim-translation`

**Scope**: A pure function that maps a `V2Claim` to a v3 JSON-LD doc.

**Outputs**:
- `pub fn v2_to_v3(claim: &V2Claim) -> Result<serde_json::Value>`
- Maps fields:
  - `claim.id` -> `@id: "synth:claim/<id>"` (truncated to first 16 chars,
    matching the spike's convention)
  - `claim.claim_type` -> `@type: "synth:<TitleCased>"`
  - `claim.asserted_at` -> `prov:generatedAtTime` ISO 8601 ms-precision
  - `claim.asserted_by` -> `prov:wasAttributedTo: "asserter:<full>"`
  - `claim.supersedes` -> `synth:supersedes: "synth:claim/<id>"`
  - `claim.props` fields expanded as `synth:<key>` predicates

**Acceptance criteria**:
- Test against the storr corpus: every v2 claim translates without
  error.
- Test: round-trip a v2 claim through translation, parse as JSON-LD
  via oxjsonld, verify expected triples emerge.

**Gotchas**:
- ISO 8601 with `Z` suffix and millisecond precision, matching the spike.
- Supersession edges become outbound predicates on the new claim, not
  inbound on the superseded claim.

---

## T4.3: Per-asserter routing during migration

**Status**: TODO
**Requires**: T4.2.
**Branch**: `v3-alpha/T4.3-migrate-routing`

**Scope**: The migration driver routes each translated claim to the
correct asserter log file using `LogWriter::append`.

**Outputs**:
- `synthesist/src/migrate/driver.rs` with the migration loop.

**Acceptance criteria**:
- Test: migrate the storr corpus, verify 6 asserter dirs created with
  correct claim counts each.

---

## T4.4: Dry-run with per-claim validation

**Status**: TODO
**Requires**: T4.2.
**Branch**: `v3-alpha/T4.4-dry-run`

**Scope**: `synthesist migrate v2-to-v3 --dry-run` reads, translates,
and validates every claim without writing. Prints a summary and any
validation failures.

**Outputs**:
- `cmd_migrate.rs` handles the `v2-to-v3` subcommand.
- The dry-run summary matches the format documented in proposal 002
  section "Dry-run report".
- Validation failures are reported with claim id, failure reason, and
  required fix.

**Acceptance criteria**:
- Dry-run on the storr corpus reports 143 claims, 0 validation failures.
- Inject a deliberately-broken claim into a test fixture; dry-run
  reports the failure cleanly.

---

## T4.5: Backup tarball creation

**Status**: TODO
**Requires**: T4.3.
**Branch**: `v3-alpha/T4.5-backup-tarball`

**Scope**: Real-run wraps the migration in a backup step that tars the
existing `claims/` tree to `.synthesist-v2-backup.tar.gz`.

**Outputs**:
- Migration writes the tarball before touching the claims tree.
- The tarball name is in `.gitignore` after migration.
- The migration is idempotent: re-running on an already-migrated repo
  is a no-op (detected by checking for v3 layout markers).

**Acceptance criteria**:
- Test: real run on a fixture, verify tarball exists and contains the
  v2 tree.
- Test: re-run real on the same fixture, verify it is a no-op (no new
  tarball, no claim duplication).

**Gotchas**:
- Add the tarball pattern to the project's `.gitignore` during migration
  if it is not already there. Do not silently bloat the repo.

---

## T4.6: Migration integration test

**Status**: TODO
**Requires**: T4.3, T4.4, T4.5.
**Branch**: `v3-alpha/T4.6-migrate-integration`

**Scope**: End-to-end test of the migration against a real v2 corpus.

**Outputs**:
- `tests/migrate.rs` running the full migration against a checked-in
  fixture (a small v2 claims tree, anonymized).
- Verifies: claim count matches, asserter distribution matches, dry-run
  validation passes, real-run writes the tarball, idempotency holds.

**Acceptance criteria**:
- Test passes under `cargo test`.
- Migration on the actual storr corpus (out-of-tree) produces the
  expected v3 layout with no claim loss.

---

# Phase 5: Surface manifest mechanism

Goal: the CLI command registry is manifest-driven. `synthesist skill
--manifest <path>` produces the skill for that manifest.

**Coordinator note**: T5.1 and T5.2 must complete before T5.3 and T5.4.

## T5.1: Manifest TOML format

**Status**: TODO
**Requires**: T3.6.
**Branch**: `v3-alpha/T5.1-manifest-format`

**Scope**: Define the manifest schema and a parser.

**Outputs**:
- `synthesist/src/surface/manifest.rs` with
  `pub struct Manifest { name: String, description: String, include: Vec<String>, exclude: Vec<String>, add: Vec<String> }`.
- `pub fn load(path: &Path) -> Result<Manifest>`.
- Documentation block in `docs/surface-manifests.md`.

**Acceptance criteria**:
- Test: parse the three baseline manifests (committed in T5.4) cleanly.
- Test: malformed TOML produces a structured error.

---

## T5.2: CLI command registry refactor

**Status**: TODO
**Requires**: T5.1.
**Branch**: `v3-alpha/T5.2-cli-registry`

**Scope**: Move the hardcoded CLI command list in `src/cli.rs` into a
registry that can be filtered by manifest.

**Outputs**:
- `src/cli.rs` exposes `pub fn build_app(manifest: &Manifest) -> clap::Command`.
- Each existing subcommand is registered with metadata identifying its
  manifest key.

**Acceptance criteria**:
- Test: build the app with the baseline manifest, verify all v2.5
  commands are present.
- Test: build with a pruned manifest, verify the excluded commands are
  absent.

---

## T5.3: `synthesist skill --manifest` flag

**Status**: TODO
**Requires**: T5.2.
**Branch**: `v3-alpha/T5.3-skill-manifest-flag`

**Scope**: Add `--manifest <path>` to `synthesist skill`. The generated
skill document reflects exactly the surface declared by the manifest.

**Outputs**:
- The flag works.
- Default (no flag) uses the baseline manifest.

**Acceptance criteria**:
- Test: skill generated under baseline matches skill from v2.5
  byte-for-byte (modulo intentional v3 additions).
- Test: skill generated under `sparql-exposed` includes the SPARQL
  surface commands.

---

## T5.4: Initial manifests committed

**Status**: TODO
**Requires**: T5.3.
**Branch**: `v3-alpha/T5.4-initial-manifests`

**Scope**: Commit the initial 5 manifests for jig experimentation.

**Outputs**:
- `synthesist/surface/baseline-v25.toml`
- `synthesist/surface/sparql-exposed.toml`
- `synthesist/surface/overlay-first-class.toml`
- `synthesist/surface/composite-commands.toml`
- `synthesist/surface/pruned.toml`

Each has a name, description, and the command include/exclude/add lists.

**Acceptance criteria**:
- All 5 parse cleanly.
- Each generates a coherent skill file.

---

# Phase 6: SPARQL + telemetry surfaces

Goal: expose the graph-query surface that telemetry will measure during
the alpha phase.

**Coordinator note**: T6.1 through T6.4 can run concurrently after T2.6
is merged. T6.5 and T6.6 depend on the surface commands existing.

## T6.1: `synthesist query --sparql`

**Status**: TODO
**Requires**: T2.6, T3.6.
**Branch**: `v3-alpha/T6.1-query-sparql`

**Scope**: A read-only subcommand that runs SPARQL against the local
view and returns results.

**Outputs**:
- `src/cmd_query.rs` with `--sparql <query>` and `--file <path>` modes.
- Output: JSON with `columns` and `rows`, matching the View's
  `SelectResults` shape.

**Acceptance criteria**:
- Test: run the spike's status-shape query against a populated test
  view, verify results.
- Test: invalid SPARQL produces a structured error.
- Test: registered behind the `sparql-exposed` manifest, hidden under
  `baseline-v25`.

---

## T6.2: `synthesist overlay run <name>`

**Status**: TODO
**Requires**: T2.6, T3.6.
**Branch**: `v3-alpha/T6.2-overlay-run`

**Scope**: An overlay registry that maps named overlays to SPARQL
CONSTRUCT queries. `overlay run <name>` executes the construct against
the current view and returns hits.

**Outputs**:
- `src/overlay/mod.rs` with `pub trait Overlay { fn name(&self) -> &str;
  fn run(&self, view: &View) -> Result<Vec<OverlayResult>> }`.
- One starter overlay registered (the actual implementation lands in
  Phase 8).

**Acceptance criteria**:
- Test: `synthesist overlay list` reports registered overlays.
- Test: `synthesist overlay run <bogus>` produces a structured error.

---

## T6.3: `synthesist serve` SPARQL endpoint

**Status**: TODO
**Requires**: T3.5, T6.1.
**Branch**: `v3-alpha/T6.3-serve-sparql`

**Scope**: Add a `/sparql` route to the axum server in `cmd_serve.rs`
following W3C SPARQL Protocol conventions (read-only).

**Outputs**:
- POST and GET methods supported per the SPARQL Protocol.
- Returns SPARQL JSON Results format on success.

**Acceptance criteria**:
- Test: send a SPARQL query via curl, get JSON results back.
- Test: read-only enforced (UPDATE rejected).

---

## T6.4: MCP method `synthesist.query`

**Status**: TODO
**Requires**: T6.1.
**Branch**: `v3-alpha/T6.4-mcp-query`

**Scope**: Expose `synthesist.query(sparql)` over the MCP server.

**Outputs**:
- If synthesist already exposes MCP (verify in v2.5), add the new method.
  If not, this task is gated on an MCP-server foundation task to be
  defined.

**Acceptance criteria**:
- An MCP-aware client can call the method and get results.

**Gotchas**:
- This task is BLOCKED until the coordinator confirms synthesist's MCP
  surface state. May be deferred to v3.0-alpha.1.

---

## T6.5: Telemetry writer

**Status**: TODO
**Requires**: T6.1, T6.2.
**Branch**: `v3-alpha/T6.5-telemetry-writer`

**Scope**: Every query through `query`, `overlay run`, `/sparql`, and
the MCP method writes one line to `claims/_telemetry/queries.jsonl`.

**Outputs**:
- `src/telemetry.rs` with `pub fn record_query(surface: Surface, sparql:
  &str, result_count: usize, latency_ms: f64, errored: bool) -> Result<()>`.
- The recorder derives `bgp_shape` and `filter_kinds` from the parsed
  query AST.
- Atomic append (same discipline as the log writer).

**Acceptance criteria**:
- Test: 10 queries produce 10 telemetry lines.
- Test: bgp_shape is consistent for the same query.
- Test: variable names and IRI literals do not appear in bgp_shape.

---

## T6.6: Query hash and shape derivation

**Status**: TODO
**Requires**: T6.5.
**Branch**: `v3-alpha/T6.6-query-shape`

**Scope**: The derived telemetry fields. Implement the algorithm that
normalizes a SPARQL query to a stable `query_hash` and a `bgp_shape`
description.

**Outputs**:
- `pub fn canonicalize(sparql: &str) -> Result<CanonForm>` returning
  hash and shape.

**Acceptance criteria**:
- Test: two queries that differ only in variable names produce the same
  hash and shape.
- Test: two queries that differ in IRI literals or filter values
  produce the same hash and shape but different result counts at
  query time.

---

# Phase 7: Jig

Goal: a framework to run canonical scenarios under different surface
manifests and aggregate outcomes.

**Coordinator note**: T7.1 must complete before others. T7.2 through T7.5
follow.

## T7.1: `synthesist jig` subcommand

**Status**: TODO
**Requires**: T5.4, T6.5.
**Branch**: `v3-alpha/T7.1-jig-subcommand`

**Scope**: A subcommand that runs a named scenario under a named
manifest and writes a result JSON.

**Outputs**:
- `src/cmd_jig.rs` with
  `synthesist jig run --scenario <name> --manifest <name>`.
- Output: `claims/_jig/<run_id>.json` with the result schema documented
  in proposal 002 section "Jig".

---

## T7.2: Canonical scenario format

**Status**: TODO
**Requires**: T7.1.
**Branch**: `v3-alpha/T7.2-scenario-format`

**Scope**: Define a TOML or markdown format for canonical scenarios.
Each scenario describes a starting state, a goal, and a scoring rubric.

**Outputs**:
- `docs/jig-scenarios.md` documenting the format.
- `synthesist/jig/scenarios/` directory committed with the format
  template.

---

## T7.3: Initial 2-3 canonical scenarios

**Status**: TODO
**Requires**: T7.2.
**Branch**: `v3-alpha/T7.3-initial-scenarios`

**Scope**: Author the first scenarios. Candidates:

- `plan-a-spec`: given a brief, produce a synthesist spec with 5-10
  tasks and lock the plan at AGREE.
- `execute-a-task`: given a synthesist task in EXECUTE phase, complete
  it and emit a Discovery.
- `triage-pending`: given a tree with 20 pending tasks, identify the
  ready ones and surface the dependency root causes.

**Outputs**:
- Three scenarios committed under `synthesist/jig/scenarios/`.

**Acceptance criteria**:
- Each scenario has a clear goal, a starting fixture, and a scoring
  rubric.

---

## T7.4: Result JSON aggregation

**Status**: TODO
**Requires**: T7.1.
**Branch**: `v3-alpha/T7.4-jig-aggregation`

**Scope**: A helper that aggregates `claims/_jig/*.json` results into
a comparison table across manifests.

**Outputs**:
- `synthesist jig aggregate` produces a markdown or CSV table.

---

## T7.5: Harness wiring (optional)

**Status**: TODO
**Requires**: T7.4.
**Branch**: `v3-alpha/T7.5-harness-wiring`

**Scope**: A small script in the keaton harness (out of tree from
synthesist) that loops manifests over scenarios and dispatches the
runs. Tracks results across the alpha window.

**Outputs**:
- `keaton/campaigns/factory/notes/jig-runner.sh` or equivalent.

**Acceptance criteria**:
- Running the script over the initial 3 scenarios and 5 manifests
  produces 15 result files.

**Gotchas**:
- This is out of the synthesist repo. It is a coordination task in the
  harness. The synthesist binary does not depend on it.

---

# Phase 8: First overlay (acceptance overlay for v3.0-alpha)

Goal: one named overlay implemented end to end against real data. The
acceptance criterion for v3.0-alpha.

**Coordinator note**: All earlier phases must be substantially complete.
T8.1 and T8.2 are sequential. T8.3 is the integration test.

## T8.1: Plan-at-risk overlay

**Status**: TODO
**Requires**: T6.2.
**Branch**: `v3-alpha/T8.1-plan-at-risk`

**Scope**: Implement the plan-at-risk overlay as a `Box<dyn Overlay>`.

The overlay walks each spec with `synth:agreeSnapshot` set. For each
claim ID in the snapshot, query the view for any newer claim with
`synth:supersedes <that ID>` whose `prov:generatedAtTime` is after
the spec's AGREE timestamp. If found, the spec is plan-at-risk.

**Outputs**:
- `src/overlay/plan_at_risk.rs` implementing the overlay.
- Registered with the overlay registry from T6.2.

**Acceptance criteria**:
- Test fixture: a spec with `agreeSnapshot` referencing 3 claims; later
  supersession of one. The overlay reports 1 hit naming the spec and the
  superseded claim.
- Test: spec with no `agreeSnapshot` returns no hits.
- Test: latency under 50 ms against a 1500-claim view.

---

## T8.2: Integration with `synthesist task ready`

**Status**: TODO
**Requires**: T8.1.
**Branch**: `v3-alpha/T8.2-task-ready-overlay`

**Scope**: `synthesist task ready` invokes the plan-at-risk overlay and
annotates each task's parent spec with a flag if the spec is at risk.

**Outputs**:
- `cmd_task.rs` updated.
- JSON output includes a per-spec `plan_at_risk: bool` field when
  applicable.

**Acceptance criteria**:
- Test: ready output for an at-risk spec includes the flag.
- Test: ready output for a non-risk spec omits the flag.

---

## T8.3: End-to-end acceptance test

**Status**: TODO
**Requires**: T8.2.
**Branch**: `v3-alpha/T8.3-overlay-e2e`

**Scope**: A scripted end-to-end test that mirrors a realistic workflow:
create a spec, AGREE the plan, supersede one of the agree-snapshot claims,
run `task ready`, verify the warning surfaces.

**Outputs**:
- `tests/overlay_e2e.rs`.

**Acceptance criteria**:
- The test runs in under 5 seconds.
- The warning is reported correctly.

---

# Coordinator playbook

## Dispatch order

A reasonable dispatch sequence for a small worker pool (e.g., 3 Sonnets
running concurrently):

1. **Sonnet A** picks T1.1 alone (blocks everything).
2. After T1.1 merges, dispatch T1.2, T1.3, T1.4, T1.5 to Sonnets A, B, C
   (parallel).
3. T1.6 picks up when T1.4 and T1.5 land.
4. T1.7 closes Phase 1.
5. Phase 2 starts at T2.1, then T2.2 through T2.5 parallel.
6. Phase 3 starts at T3.1, then T3.2 through T3.5 parallel.
7. Phase 4 sequence: T4.1 and T4.2 parallel, then T4.3, T4.4, T4.5
   parallel, then T4.6.
8. Phase 5 sequence: T5.1, T5.2, T5.3 sequential, T5.4 last.
9. Phase 6 sequence: T6.1 through T6.4 parallel (T6.4 may block on MCP
   foundation), then T6.5 and T6.6.
10. Phase 7 sequence: T7.1 through T7.5 mostly sequential.
11. Phase 8 sequence: T8.1, T8.2, T8.3 sequential.

A 3-Sonnet pool can complete v3.0-alpha in roughly 4-5 weeks of
calendar time, assuming clean reviews and no architectural surprises.

## Verification discipline

For each task, the coordinator verifies:
- Acceptance criteria from the task block are met.
- Tests pass under CI.
- The PR description references the task ID.
- The branch follows the naming convention.
- No em dashes in any added file.
- No unrelated changes leak into the PR.

If a worker surfaces a blocker not anticipated in the task block, the
coordinator updates this document with the resolution and dispatches a
follow-up task or adjusts the block.

## Handling architectural surprises

If a worker discovers that the design in proposal 002 needs adjustment
(e.g., Oxigraph cannot do what the substrate requires), the worker
surfaces this rather than working around it. The coordinator decides
whether to:
- Adjust this implementation plan (in-flight task block updated).
- Adjust proposal 002 (more significant: requires the proposal owner
  to amend the design).
- Park the task and revisit (if the surprise blocks the broader
  approach).

Workers do not silently adapt the design.

## Merge conflict policy

Parallel tasks within the same phase should not conflict if the file
ownership in each task block is respected. If two parallel tasks both
touch `src/cli.rs` (likely in Phase 5 or 6), the coordinator serializes
them or splits the file ownership more granularly.

When `v3-alpha-integration` merges to `main`, any v2.5.x patches that
landed on `main` during the build need to be merged forward. The
coordinator owns this rebase.

## Out-of-scope rejections

If a worker proposes additions to a task beyond its scope, the
coordinator rejects them and either creates a follow-up task or marks
them as v3.x. The temptation to refactor adjacent code is real and must
be resisted to keep the phasing tractable.

## When to escalate to the proposal owner

- Any change that requires editing proposal 002.
- Any architectural surprise that has implications for gamma.
- Any task that cannot meet its acceptance criteria without scope creep.
- Any worker disagreement about interpretation of a task block that the
  coordinator cannot resolve from the proposal text.

# Closing

This document is a contract between the proposal owner, the coordinator,
and the workers. It can be amended; amendments should be explicit and
versioned (e.g., commit messages reference the task IDs being changed).
The phasing is a guide; the dependency graph is the truth. If a worker
identifies a missing dependency or an extra one, surface it and the
coordinator updates the block.

When v3.0-alpha.0 ships at the end of Phase 4, the substrate is real
even before the experimentation surfaces (Phases 5 through 8) land.
That ordering is intentional: storage and migration are the load-bearing
risk, and shipping them first lets us pull people forward while
experimentation matures.

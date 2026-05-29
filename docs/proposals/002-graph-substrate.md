# Proposal 002: Graph Substrate, Alpha-then-Gamma

**Status**: Draft
**Date**: 2026-05-28
**Author**: Andrew Dunn

## Summary

Replace the v2.x Automerge claim store with per-asserter JSON-LD logs queried through an embedded RDF triple store. Ship in two phases:

- **v3.0-alpha**: full graph stack (Oxigraph plus its transitive deps) to validate the integration thesis empirically. Surface is manifest-driven from day one and identical to v2.5 by default; SPARQL is exposed as an additional surface that LLMs can reach for.
- **v3.0 stable (gamma)**: custom triple index plus query functions over a constrained JSON-LD format. Implemented to the *measured* demand from alpha telemetry and from a jig that runs canonical scenarios against alternate surface manifests.

The substrate contract (JSON-LD on disk, per-asserter logs, PROV-O attribution, supersession edges) is invariant between alpha and gamma. Only the query engine changes.

Acceptance criteria, kill-switches, and out-of-scope items are explicit in the relevant sections below.

## Why v3

Three pressures converged on 2026-05-28:

1. **Empirical bytes pain in production.** Teams running synthesist 2.5.1 in active use generate hundreds of megabytes to gigabytes of `claims/changes/` traffic per day, breaking git transactions and making `git diff` useless for review. Measurement on a single-user instance (143 claims, light use) shows ~240 KB per Automerge `.amc` change file for ~350 B of actual JSON payload content. The Automerge per-change envelope (vector clocks, dep hashes, op-counter state) dominates by ~700x over actual content. This is fundamental to the Automerge change format, not a tuning knob.

2. **Future cross-graph overlay capability is now load-bearing.** Synthesist's claim graph is the right shape for queries that traverse spec/task/discovery/supersession relationships. Today those queries go through SQLite projection and ad-hoc Rust. As the workflow surface grows (named overlays, cross-module integrations with future sibling tools, agent-readable derivations), the substrate needs a query layer that can answer graph-shaped questions cheaply. v3 is the place to add that capability.

3. **The "operators read raw files" intuition was wrong.** v2 picked Automerge in part for substrate-level merge semantics; the cost was that operators and LLMs cannot read or edit the on-disk format directly. Tonight's analysis concluded that conformity (preventing LLMs from inventing fields or bypassing the validator) belongs at the harness-hook level, not at the storage-opacity level. JSON-LD on disk gives the LLM grep-able state, the harness gives mediation, and the substrate enforces structural validity through imperative validators.

## Empirical findings from 2026-05-28

All measurements were taken against a real single-asserter synthesist 2.5.1 instance with 143 claims of light volume.

### Finding 1: the compaction kill-switch did not fire

`synthesist claims compact --yes` collapsed 143 change files into one 135 KB `snapshot.amc`. Local on-disk footprint dropped from 34 MB to 344 KB (99% local reduction).

The git-tracked footprint did not change. `claims/snapshot.amc` is in the project `.gitignore`. The 143 deleted change files appeared in `git status` as `deleted:`. Committing that deletion would leave a fresh clone with only `genesis.amc` and no claim history. The substrate design treats `claims/changes/*.amc` as source of truth and `snapshot.amc` as a local-only cache.

**Conclusion**: `claims compact` is a local `Store::open` speedup. It does not solve the git-bytes problem and was never going to. v3 is justified by bytes urgency.

### Finding 2: v3 JSON-LD size projection holds

A Python script transformed the 143 claims (extracted via `synthesist sql`) into compact JSON-LD with `@context` referenced by URI, prov-predicated timestamps, asserter attribution, supersession edges, and props expanded as `synthesist:<key>` predicates. One JSON-LD doc per line, one file.

| Metric | v2.5 `.amc` | v3 JSON-LD | Ratio |
|---|---|---|---|
| Total on disk | 34 MB | 92 KB | **378x** |
| Per claim (avg) | ~240 KB | 641 B | 374x |

Per-type breakdown of v3 JSON-LD sizes:

| Type | n | avg bytes | max bytes |
|---|---|---|---|
| campaign | 4 | 385 | 509 |
| discovery | 4 | 1849 | 3918 |
| phase | 16 | 302 | 318 |
| session | 4 | 279 | 362 |
| spec | 6 | 487 | 684 |
| task | 107 | 683 | 1136 |
| tree | 2 | 316 | 324 |

No payload cap applied. The corpus includes discoveries with substantive narrative content (max 3.9 KB, a real institutional-memory note). The 378x ratio is measured against this real distribution. At heavier discovery payloads in larger workloads the relative improvement shrinks; absolute storage gets closer to "actual content bytes" rather than "Automerge envelope plus content."

### Finding 3: Oxigraph spike works end to end

A Rust spike loaded the JSON-LD into an embedded Oxigraph store and ran synthesist-shaped queries:

| Query | Result | Latency |
|---|---|---|
| Triple count | 1099 triples (7.7 per claim) | 892 us |
| Count by type (status-shape) | 7 types, matches `synthesist status` | 100 us |
| Pending tasks (task-list-shape) | 60 | 147 us |
| Asserter audit | 6 distinct asserters | 52 us |
| Cross-graph overlay | 4 hits joining synth with a synthetic external module | 86 us |

The overlay query validated cross-graph join mechanics. A synthetic external observation claim was injected under a separate named graph; tasks containing a topic that matched the external observation's subject were flagged via one SPARQL query joining the two graphs. Result returned in 86 microseconds with four correct hits. **This is the load-bearing v3 capability working end to end on real data.**

Load time for all 143 JSON-LD docs into Oxigraph: 4 ms. Zero parse failures.

### Finding 4: Rust SHACL ecosystem is fractured

The conformance-validator side of the spike did not land cleanly. `shacl_validation` 0.2 plus `shacl_ast` 0.2 plus `srdf` failed to compile together on stable Rust 1.94. `oxirs-shacl` 0.3 compiled but pulled in tokio, hyper, reqwest, rustls, scirs2-linalg, and scirs2-stats. The transitive cloud is incompatible with synthesist's single-binary discipline at v3.0 scale.

**Conclusion**: SHACL as a runtime gate is not the right tool in Rust today. The right pattern is to keep imperative validators (the synthesist v2.5 pattern in `src/schema/*.rs`) and emit a Turtle/SHACL artifact as a build-time documentation product for LLMs and external consumers.

## Proposed substrate shape

### On-disk layout

```
claims/
  genesis.jsonld                # git-tracked, bootstrap
  <asserter-1>/log.jsonl        # git-tracked, append-only,
  <asserter-2>/log.jsonl        # one writer per file
  <asserter-N>/log.jsonl
  _schema/
    synth.shacl.ttl             # emitted documentation, not runtime gate
  _telemetry/
    queries.jsonl               # gitignored
  _view.oxigraph/               # gitignored, RocksDB index
  config.toml                   # git-tracked, schema_version, format
```

The substrate's source of truth is the union of `<asserter>/log.jsonl` files. All other artifacts (view, telemetry, schema documentation) are derived and gitignored except the schema-doc Turtle, which is emitted on release for downstream consumers.

Each line in a `<asserter>/log.jsonl` is one compact JSON-LD document representing one claim. Example:

```jsonld
{"@context":"https://nomograph.org/v3","@id":"synthesist:claim/cf42b88aaf17276c","@type":"synthesist:Task","prov:generatedAtTime":"2026-05-05T17:43:11.695Z","prov:wasAttributedTo":"asserter:user:local:agd:edc-bootstrap","synthesist:depends_on":[],"synthesist:files":[],"synthesist:gate":"human","synthesist:id":"t1","synthesist:spec":"deploy","synthesist:status":"pending","synthesist:summary":"...","synthesist:tree":"edc"}
```

The `@context` is referenced by URI to the `nomograph-ontology` artifact. Tools embed a local copy of the context for offline use.

### Multi-user model

Per-asserter logs have exactly one writer each. Different asserters never touch the same file. This gives:

- **Git as the default sync transport.** Push commits, pull peers' logs, the local view rebuilds from the union.
- **Syncthing, Dropbox, NFS, shared mounts** work without modification.

Multi-user is **a property of the file layout, not a substrate feature**. A future realtime sync layer can ship log files between peers when teams outgrow git-pull cadence, but no such sync layer is in scope for v3.

### Mono-temporal substrate with schema-level bi-temporal opt-in

The substrate commits to one timestamp predicate: `prov:generatedAtTime` on every claim. The supersession chain (each claim points at the one it supersedes via `synthesist:supersedes`) captures causality at the semantic layer, independent of clocks.

Modules that need valid-time semantics add their own predicates (e.g., `<module>:effectiveStart` and `<module>:effectiveEnd`) in their own SHACL shapes. SPARQL queries filter on those when needed. The substrate is unaware; it sees triples.

Bi-temporal expressiveness in the v2 README was aspirational and never delivered in the v2.x schema (which carries only `asserted_at`). v3 drops that aspiration honestly and replaces it with the schema-level opt-in pattern.

### Validation: imperative gate, declarative artifact

The synthesist v2.5 imperative validators in `src/schema/*.rs` migrate forward into v3 unchanged. They handle:

- DAG cycle detection on task dependencies
- Phase state-machine enforcement
- Status transition rules
- Asserter scope rules
- Cardinality and required-field checks
- Enum membership for claim-type-specific fields

In addition, v3 ships a build-step emitter that produces `_schema/synth.shacl.ttl` from the same Rust schema definitions. The Turtle artifact is **documentation only**, consumed by:

- The LLM's skill file (synthesist's skill output references it)
- External tools that want to know synthesist's vocabulary
- The Oxigraph store (in alpha) can run SHACL validation if the Rust SHACL ecosystem matures and a lighter dep cloud emerges

The Turtle artifact is not a runtime gate. The imperative validators are the gate.

## Phasing: alpha then gamma

### v3.0-alpha: validate the thesis

**Stack**: Oxigraph 0.4+ for the in-memory triple store and SPARQL engine. JSON-LD parsing via `oxjsonld`. Imperative validators unchanged from v2.5. Roughly 100 transitive Rust dependencies in the alpha binary.

**Goal**: empirically determine whether the graph-query surface (SPARQL plus named-graph overlays) earns its keep in real LLM-mediated workflows.

**Default surface manifest**: identical to v2.5 (operator continuity). All existing CLI commands behave as in v2.5.

**Added surfaces** (available, not in the default skill file):
- `synthesist query --sparql '<query>'` -- read-only SPARQL against the local store, returns JSON
- `synthesist overlay run <name>` -- invoke a named overlay (internally a SPARQL CONSTRUCT)
- `synthesist serve`'s `/sparql` HTTP endpoint -- W3C-conventional SPARQL endpoint, read-only
- MCP method `synthesist.query(sparql)` -- same surface exposed over MCP for runtimes that talk it

These surfaces are deliberately *available but undocumented in the default skill*. They become discoverable when an LLM is run under an alternate surface manifest (see jig section below).

**Versioning discipline**: tagged as `3.0.0-alpha.N`. The CHANGELOG and README flag alpha as substrate-evaluation-only. Operators who require stability stay on 2.5.x. The dogfood population is intentional and opt-in. No other users.

**Lifetime**: 2-3 months of dogfooding plus jig experimentation, then a gate decision.

### The alpha-to-gamma gate

After 2-3 months of telemetry, the proposal owner decides between three outcomes:

**Outcome A: graph surface earns its keep.** Telemetry shows LLMs using the SPARQL surface meaningfully (rough heuristic: 10%+ of synthesist tool invocations in well-tuned harnesses), cross-module queries are non-trivial, and overlays shape real decisions (operators report behavior change traceable to overlay output). **Action**: commit to gamma. Implement the measured query patterns as Rust functions over a custom triple index. Drop Oxigraph and most transitive deps.

**Outcome B: graph surface is ignored.** Telemetry shows LLMs sticking to CLI commands, SPARQL surface unused, overlays unused or noise. **Action**: drop back to a smaller v3.0 (call it "beta" if it ships). Keep JSON-LD storage and per-asserter logs. Drop Oxigraph. Build a SQLite projection from the JSONL like v2.5 did from `.amc`. Cross-module overlays (if any are wanted later) become SQL joins across projections. Substrate savings still land; the graph stack is the thing we cut.

**Outcome C: signal is mixed.** Some patterns earn their keep, others do not. **Action**: implement gamma sized to the demand we can name. The kill-switch is not binary; the implementation effort is.

### v3.0 stable (gamma)

**Stack**: custom triple index (BTreeMap-based, ~400 LOC). Constrained JSON-LD parser (~500 LOC, with a CI gate that round-trips against a conformant parser). Query functions in Rust for each pattern the alpha telemetry showed (~500-2000 LOC depending on demand). Imperative validators unchanged. SHACL Turtle emitter unchanged. Zero external graph deps.

**Goal**: production-grade substrate for the long haul.

**Surface manifest**: the winning manifest from jig experimentation.

**Migration from alpha to gamma**: trivial. The JSON-LD on-disk format is identical. Only the query engine changes. Operators upgrade the binary and continue.

## Surface manifests and the jig

### Manifest

Synthesist's CLI command registry, currently hardcoded, becomes manifest-driven. A surface manifest is a TOML file declaring which commands are exposed in the default skill, which are hidden, and which alternate skill files are available.

```toml
# synthesist.surface.toml -- variant: baseline-v25
[manifest]
name = "baseline-v25"
description = "v2.5-identical surface"

[commands]
include = ["status", "task add", "task ready", "task done",
           "spec add", "spec show", "discovery add", "phase set",
           "session start", "session close", "tree add",
           "campaign add"]
exclude = []
add = []
```

```toml
# synthesist.surface.toml -- variant: sparql-exposed
[manifest]
name = "sparql-exposed"
description = "v2.5 baseline plus graph query surface"

[commands]
include = [<v2.5 baseline>]
add = ["query", "overlay run", "spec hierarchy"]
```

The skill file is generated per manifest: `synthesist skill --manifest <path>` emits the skill that documents exactly that surface.

### Jig

A `synthesist jig` subcommand runs canonical task scenarios against named manifests and records outcomes to `_jig/<run>.json`. Outcomes include:

- Tool-invocation count and shape per session
- Errors and self-corrections
- Time to scenario completion
- Frequency of `synthesist query --sparql` vs CLI commands
- Final artifact (the produced spec/plan/code) for human rating
- Aggregated telemetry from the alpha query surface

A small harness loops manifests over scenarios and aggregates. The user runs scenarios as part of their normal day; the jig observes.

### Initial manifests to compare

Once alpha ships, the first jig experiments compare:

- `baseline-v25` -- surface identical to v2.5
- `sparql-exposed` -- adds SPARQL surface to the skill
- `overlay-first-class` -- overlays promoted from query subcommand to dedicated `synthesist warn ...` commands
- `composite-commands` -- high-level commands like `synthesist plan-review` that compose multiple v2.5 ops
- `pruned` -- rarely-used v2.5 commands removed from the skill to reduce noise

Each experiment runs on the same 2-3 canonical scenarios. We learn which expansions yield measurable improvement in LLM session quality. The winning manifest becomes the default in v3.0 stable.

## Telemetry

### Local-only by default

Every query through the alpha surfaces (`synthesist query`, `/sparql`, MCP method) writes one line to `claims/_telemetry/queries.jsonl`. Local-only. Gitignored. Audit-friendly for the operator.

```jsonld
{"ts": "2026-06-15T09:14:33Z",
 "surface": "cli|http|mcp",
 "query_hash": "<blake3 of canonical query>",
 "bgp_shape": "?c rdf:type ?t . ?c synthesist:status ?s",
 "filter_kinds": ["literal-eq"],
 "result_count": 60,
 "latency_ms": 0.147,
 "errored": false}
```

The `query_hash` lets us count repeated patterns without storing raw queries forever. The `bgp_shape` and `filter_kinds` are derived from the parsed AST so we can aggregate over query shape without storing variable names or IRI literals.

### Opt-in export

If the operator opts in (a one-line config in `claims/config.toml`), an export tool ships aggregated telemetry to a designated endpoint. The export strips raw query strings and keeps only `(query_hash, bgp_shape, filter_kinds, result_count_bucket, latency_bucket, count_in_window)`. Audit-able privacy posture: no IRI literals, no asserter names, no claim payloads ever leave the local machine.

This is the data that informs gamma's query-function set.

## Schema

The v2.5 synthesist primitives migrate forward into v3 unchanged. The substrate adds a universal claim envelope; synthesist's own claim types stay in the `synthesist:` namespace.

**Universal claim envelope** (in `nomograph-ontology`, embedded in `nomograph-claim`):
- `@id`: stable IRI (`synthesist:claim/<hash>`)
- `@type`: claim type IRI
- `prov:generatedAtTime`: xsd:dateTime
- `prov:wasAttributedTo`: asserter IRI
- `synthesist:supersedes`: optional, prior claim IRI
- `nomograph:parentAsserter`: optional, for agent hierarchy audit

**Synthesist module** (`synthesist:` namespace):
- Tree, Spec, Task, Discovery, Session, Phase, Outcome, Campaign (per v2.5)
- Two additive field plans carried forward from earlier internal design: `Spec.topics` (a free-text array for cross-module join key) and `Spec.agree_snapshot` (an array of claim IRIs captured at AGREE commit, for plan-at-risk detection in future overlays). Both are optional fields; existing claims validate cleanly without them.

**Asserter model**: `<class>:<scope>:<id>` strings, matching v2.5's existing convention (`user:local:agd`, `agent:claude-opus-4-7:<session>`, `ingest:gitlab:<source>`). Trust derives from git push access at v3-alpha. Cryptographic signing is flagged as a v3.x decision.

The substrate is module-agnostic. Future companion modules (whatever they end up being) carry their own namespaces and SHACL shapes, ship with their own tools, and compose via named graphs in the union store. The substrate does not enforce or govern cross-module schemas.

## Crate layout

### Phase 1 (v3.0-alpha)

```
nomograph-claim/                # substrate runtime (Rust)
  src/                          # JSON-LD I/O, log writer/reader,
                                # Oxigraph integration (alpha),
                                # PROV-O constants, identity
  ontology/                     # base vocabulary, embedded
    base.ttl
    base.shacl.ttl
  (no separate nomograph-ontology crate yet)

synthesist/                     # binary, depends on nomograph-claim
  src/                          # CLI, imperative validators
  ontology/                     # synthesist: vocabulary, embedded
    synth.ttl
    synth.shacl.ttl
  surface/                      # surface manifests
    baseline-v25.toml
    sparql-exposed.toml
    ...

(future tools sit alongside synthesist with the same shape:
 their own crate, their own embedded ontology, depending on
 nomograph-claim only)
```

`nomograph-workflow` (v0.3.x today) folds into synthesist directly. Its concepts (Session, Phase) are synthesist-specific in v3.

### Phase 2 (post-alpha, before gamma stable)

When the base vocabulary stabilizes after 2-3 months of real use, extract `nomograph-ontology` as a data-only crate:

```
nomograph-claim/                # runtime, no ontology data
nomograph-ontology/             # versioned data crate, published
                                # at https://nomograph.org/ontology/v1/
synthesist/                     # depends on both
```

This extraction is a Phase 2 cleanup, not a v3.0 blocker.

## Migration from v2.5 to v3-alpha

The pattern mirrors `synthesist migrate v1-to-v2` exactly. One-shot, idempotent, dry-run first, original timestamps preserved.

```bash
synthesist migrate v2-to-v3 --dry-run    # report what would change
synthesist migrate v2-to-v3              # do it
```

### What the migration tool does

1. Read every `.amc` file in `claims/changes/` and load the full Automerge document.
2. Walk claims in causal order. For each:
   - Map `claim_type`, `props`, `asserted_at`, `asserted_by`, `supersedes` to the v3 JSON-LD shape.
   - Route to `claims/<asserter>/log.jsonl` based on `asserted_by`.
3. **Validate every emitted JSON-LD doc against the v3 imperative validators in the same pass.** Any failure stops the migration with a structured error naming the claim, the failure, and the required fix.
4. Write a `.synthesist-v2-backup.tar.gz` (gitignored) containing the original `claims/` tree.
5. Move the new `claims/` tree into place.

### Dry-run report

The dry-run prints a summary and any validation failures:

```
v2-to-v3 migration dry-run:

would emit:
  trees         2
  specs         6
  tasks         107
  discoveries   4
  sessions      4
  campaigns     4
  phases        16

per asserter:
  user:local:agd:edc-bootstrap          123 claims
  user:local:agd                        13
  user:local:andrewdunn                 3
  user:local:andrewdunn:overnight-deploy 2
  user:local:agd:secondbreakfast        1
  user:local:agd:cms-t5                 1

validation:
  passed                                143
  failed                                0
```

If failures appear, the operator fixes the source data (or the migration tool) and re-runs the dry-run before the real run.

### Team coordination

Migration is a release event, not a per-user operation:

1. All operators commit any in-flight v2 work.
2. One operator runs `migrate v2-to-v3 --dry-run`, verifies, then runs for real.
3. Operator commits the new `claims/<asserter>/` tree alongside `.amc` deletion in one commit and pushes.
4. Everyone else pulls. Their v2.5 binary fails noisily because `claims/changes/` is gone; they upgrade to v3-alpha and continue.

### Backup retention

The `.synthesist-v2-backup.tar.gz` is retained until the first stable v3.0 release (the v3.0-alpha-to-stable transition, expected 3-6 months). Operators are instructed not to delete it within that window. After v3.0 ships stable, the backup can be removed.

## What v3 explicitly does NOT ship

Stating this clearly so the proposal cannot grow under it:

- **Any realtime sync protocol or peer-to-peer transport.** v3 produces sync-friendly files. Transport is git or any filesystem-mirroring tool. Realtime sync is a v3.x value-add.
- **Cross-asserter realtime agent coordination.** Within one operator's machine, multiple agents can coordinate via the local store. Across operators, coordination runs at git-pull cadence (minutes to hours). Cross-asserter realtime is a v3.x feature.
- **Cryptographic signing of claims.** Identity is `<class>:<scope>:<id>` strings with self-asserted aliasing where applicable. Trust derives from git push access. Signing is a v3.x decision.
- **A unified data model across modules.** Synthesist owns the `synthesist:` vocabulary. Future companion modules own their own. Cross-tool queries are SPARQL (alpha) or query functions (gamma) that join across named graphs. The substrate enforces structural validity per module; it does not enforce a global schema.
- **A metapackage binary or any tool orchestration layer.** v3 ships the substrate. Higher-level tools that consume multiple modules are out of scope.
- **Automatic phase transitions, agentic gating, or self-modifying schema.** v3 preserves the v2.5 state machine and adds no new automation.

## Pre-implementation milestones

Two checks completed before this proposal was drafted:

1. **Compaction check** -- done 2026-05-28. Result: kill-switch did not fire. Documented above.

2. **Oxigraph + JSON-LD spike** -- done 2026-05-28. Result: end to end working on real claim-store-scale data. Documented above.

One remaining check before alpha implementation begins:

3. **The first overlay to prove the integration capability.** Pick the concrete overlay query that v3-alpha must support end to end. The candidate is a plan-at-risk detector keyed on `Spec.agree_snapshot`: when any claim in a spec's locked-plan snapshot is superseded, flag the spec. This exercises supersession traversal, named-graph union, and the overlay invocation surface in one feature.

## Acceptance criteria for v3.0-alpha

**Functional**:
- `synthesist migrate v2-to-v3` round-trips an existing v2.5 instance with 100% claim count match and zero validation failures.
- `synthesist status`, `task ready`, `task add`, `task done`, `phase set`, `session start`, `discovery add` all behave identically to v2.5 against the new substrate.
- `synthesist serve` HTML view renders the spec tree and network views unchanged.
- One named overlay is invokable via `synthesist overlay run <name>` and returns hits when the underlying claim shape matches.

**Empirical**:
- Storage growth on a typical workload is within 3x of the projected 641 B per claim average. (Generous tolerance because real workloads have heavier discoveries than the test corpus.)
- Query latency at 10x test-corpus scale (~1500 claims) is under 50 ms for `synthesist status`, `task ready`, and the first overlay.
- Cold rebuild of `_view.oxigraph/` from a fresh log union completes in under 30 seconds for 10x test-corpus scale.

**Operational**:
- Surface manifest mechanism works: `synthesist skill --manifest <path>` regenerates the skill against an alternate manifest.
- `synthesist jig` runs a canonical scenario under a named manifest and writes a result JSON to `_jig/<run>.json`.
- Telemetry writes one line per query to `claims/_telemetry/queries.jsonl` with the shape documented above.

## Open decisions for the proposal owner

These cannot be settled by the design alone:

1. **Backup retention window**: the proposal says 3-6 months; pick a concrete commitment.
2. **Telemetry export endpoint**: opt-in target. Decide now if we want to specify the endpoint format.
3. **Surface manifest experimental policy**: who decides which manifests get tested in the jig phase.
4. **Identity model evolution**: when (not whether) signing comes back on the table. Naming a v3.x decision point now makes the gap explicit instead of latent.
5. **Migration scheduling**: when existing users cut over, what their backup looks like, whether to stagger.

## Risks and how this proposal addresses them

| Risk | Severity | Mitigation |
|---|---|---|
| Alpha 100-dep cloud breaks via transitive CVE or upstream churn | Medium | Alpha is time-boxed (2-3 months). Gate decision forces commitment to gamma (cut deps) or beta (cut graph stack). No staying on the 100-dep stack permanently. |
| Migration is one-shot and irreversible after `.amc` deletion | High | Dry-run includes per-claim validation. Backup tarball retained 3-6 months. Migration is per-team release event, not per-user. |
| LLMs do not adopt the SPARQL surface and graph thesis collapses | Medium | This is what alpha measures. Outcome B in the gate explicitly drops back to a beta-style v3.0 (JSON-LD plus SQLite projection). Substrate savings still land. |
| SHACL ecosystem in Rust remains rough | Confirmed today | Imperative validators stay the runtime gate. SHACL ships as documentation artifact only. No runtime dep on Rust SHACL crates. |
| Substrate change taxes solo users without compensating benefit | Medium | Solo user sees identical CLI, faster local store, smaller `claims/` tree, grep-able audit. No daemon, no peer, no realtime sync. The new surfaces are additive, not required. |
| The jig phase becomes a yak | Medium | First experiments are 2-3 manifests against 2-3 scenarios, time-boxed to 4-6 weeks. If signal is unclear, simpler defaults probably win and we move on. |
| Cross-asserter agent coordination is materially weaker than an agent-army product story would imply | Acknowledged | v3 ships single-asserter-machine coordination only. Cross-asserter realtime requires a future sync layer and is v3.x. Stated explicitly in "What v3 does NOT ship". |

## Implementation phasing inside v3.0-alpha

A rough sequence; not a commitment.

| Week | Work |
|---|---|
| 1 | `nomograph-claim` JSON-LD I/O, log writer, per-asserter routing. Embedded base ontology. |
| 2 | `nomograph-claim` Oxigraph integration, view rebuild from log union. |
| 3 | Synthesist CLI repointed at new substrate. v2.5 imperative validators carried forward. SHACL Turtle emitter. |
| 4 | `synthesist migrate v2-to-v3` tool. Dry-run with per-claim validation. Backup tarball. |
| 5 | Surface manifest mechanism. `synthesist skill --manifest`. Initial manifests committed. |
| 6 | `synthesist query`, `overlay run`, `/sparql` endpoint, MCP method. Telemetry writer. |
| 7 | `synthesist jig` subcommand. Canonical scenarios. Harness wiring. |
| 8 | One overlay implemented end to end on real data. |

Estimated total: 8 weeks for one experienced Rust developer to a shippable v3.0-alpha.

## What ships when

- **v3.0-alpha.0**: storage, migration, basic CLI on new substrate (weeks 1-4 above).
- **v3.0-alpha.1**: surface manifests, jig, telemetry (weeks 5-7).
- **v3.0-alpha.2**: first overlay, integration acceptance (week 8).
- **v3.0-alpha.N**: jig variants tried, telemetry accumulating.
- **Alpha-to-gamma gate**: 2-3 months after alpha.0.
- **v3.0 stable (gamma or beta depending on the gate outcome)**: 6-8 weeks after the gate decision in the gamma case, faster in the beta case.

Total wall-clock to v3.0 stable: 5-6 months from start of alpha work.

## References

- Architecture v1: `docs/architecture-v1.md`
- Proposal 001 (holdout sessions): `docs/proposals/001-holdout-sessions.md`

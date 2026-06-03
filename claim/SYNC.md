# Sync Architecture

`nomograph-claim` stores state as a set of per-asserter append-only
JSON-LD logs under a visible `claims/` directory. Each writer owns one
log; the union of all logs is the source of truth. Sync is plain git:
peers exchange logs by pulling and pushing a shared repository. There is
no server and no realtime relay.

Related reading:

- [`IDENTITY.md`](./IDENTITY.md) for the v0.1 asserter trust model.
- [`synthesist/MIGRATION-v2-to-v3.md`](https://gitlab.com/nomograph/synthesist/-/blob/main/MIGRATION-v2-to-v3.md)
  for how existing v2 projects move onto this substrate.

## What lives under `claims/`

```text
claims/
  <asserter>/log.jsonl   per-writer append-only JSON-LD log (tracked)
  config.toml            schema version, project metadata (tracked)
  _view.gamma            redb gamma index, disposable cache (gitignored)
```

The per-asserter logs and `config.toml` are git-tracked. `_view.gamma`
is a local derived cache and is never committed.

## Why per-asserter logs sync cleanly

Each writer appends only to `claims/<asserter>/log.jsonl`, the log keyed
to its own asserter id. Two collaborators writing at the same time touch
two different files, so a `git pull` brings in the other writer's log
without a textual conflict and without a merge engine. There is no CRDT
reconciliation step and no per-field merge: append-only logs in disjoint
files compose by union.

`git push` is not a commit of intent. It replicates the claims you have
already asserted locally so other peers can pull them.

## Heads-driven index rebuild

The gamma index (`claims/_view.gamma`) is a derived projection of the log
union. It is rebuilt only when the logs change, detected by a cheap heads
signal: a hash over the sorted asserter directory names and their
per-file line counts.

1. **Git fetch and merge.** `git pull` brings in every new and extended
   `claims/<asserter>/log.jsonl` that landed while you were away. These
   are just files on disk; no claims are loaded yet.
2. **Index sync.** The next read invocation computes the current heads
   signal, sees it differs from the signal recorded in the index's
   `meta` table, and rebuilds the gamma index from the full log union.
   When the signal matches, the rebuild is skipped and the cached index
   is reused.

The index is disposable: deleting `_view.gamma` forces a clean rebuild
from the logs on the next read. Final query state depends only on the
set of logs present, never on pull order.

## Two-person git workflow

The canonical multi-user setup is two or more people sharing one git
repository, each with a distinct asserter id:

1. Each person works locally, appending to their own
   `claims/<asserter>/log.jsonl`.
2. At a natural breakpoint they `git pull` to receive peers' logs and
   `git push` to publish their own.
3. The next read on each machine rebuilds the gamma index from the
   combined logs, so everyone converges on the same query state.

Because writers never share a log file, pulls are conflict-free at the
git layer for claim content. The only conflicts git can surface are on
shared non-log files (e.g. `config.toml`), resolved the ordinary way.

## Conflicts in the claim graph

A *claim-graph* conflict is distinct from a git conflict. Two writers
can each supersede the same claim with a different successor. Both
supersession edges land cleanly on pull (different logs), and the index
then shows two rival "current" claims. Surface these with:

```bash
synthesist conflicts
```

which lists every claim with an unresolved supersession fan-out and
prompts the operator to append a fresh claim superseding both rivals.

## What NOT to put in the claim log

The substrate is for structured work claims. It is not an arbitrary
key-value store and not a blob store. Do not append:

- **Secrets.** Logs replicate to every peer with repository access. That
  is the wrong blast radius for credentials; use a secret manager.
- **Large binary blobs.** Images, PDFs, datasets, model weights. The
  index reads every log line on a rebuild, so big blobs slow every
  rebuild for every peer. Reference them by URL or content hash and
  store the bytes elsewhere.
- **Ephemeral conversation state.** A claim is asserted forever. Use a
  scratchpad, not the log, for thinking out loud.

If the thing you are storing is not a typed assertion that a tool will
later read to make a decision, it does not belong in the log.

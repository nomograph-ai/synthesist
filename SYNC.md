# Sync Architecture

`nomograph-claim` stores state as an append-only CRDT log under a
visible `claims/` directory. Two sync channels carry those claims
between peers: **git** and **beacon**. Both are optional and
independent. A user with only one still reaches correct eventual
state.

Related reading:

- [`IDENTITY.md`](./IDENTITY.md) for the v0.1 asserter trust model.
- [`synthesist/MIGRATION.md`](https://gitlab.com/nomograph/synthesist/-/blob/main/MIGRATION.md)
  for how existing v1 projects move onto this substrate.

## Two sync channels by design

### Git: cold storage

Everything under `claims/` that is durable is git-tracked:

```text
claims/
  genesis.amc          # bootstrap document
  changes/<hash>.amc   # content-addressed, append-only changes
  config.toml          # schema version, project metadata
```

`git pull` receives peer changes. Because the log is a CRDT, a merge
is not a rebase: two collaborators appending at once produce two new
files under `changes/`, both land cleanly on pull, and Automerge
reconciles the document on next open. There is no textual conflict
at the git layer.

`git push` is not a commit of intent. It replicates claims you have
already asserted locally.

### Beacon: realtime relay

[Beacon](https://gitlab.com/nomograph/beacon) is a Cloudflare
Workers + Durable Object WebSocket relay. When enabled, the claim
library opens a persistent WS, authenticates via a forge token
(GitLab, today), and ships each local append as a ciphertext frame
to any subscribed peer on the same project channel.

- **Forge-mediated auth.** Beacon does not issue credentials. It
  verifies your forge token against project membership and
  short-circuits any connection that fails.
- **Blind relay.** Claim content is E2EE with ChaCha20-Poly1305
  under a key derived from the project secret. Beacon sees a
  project id and an opaque envelope; it cannot read claims.
- **No durability.** Beacon is a fan-out hose, not a log. Offline
  peers catch up via git, not beacon replay.

## What happens when one channel is down

**Beacon down, git up.** Effectively synthesist v1 semantics on the
v2 substrate. Local appends go to `claims/changes/`; you `git pull`
to receive and `git push` to publish. CRDT merges happen on open.
You lose realtime peer awareness, nothing more.

**Beacon up, git down.** Local appends write to disk and stream to
the relay, so connected peers see your work in real time. When git
access returns, the same files push up with no special handling.

**Both down.** Pure local. The log grows on your disk. On reconnect
to either channel, CRDT reconciles whatever divergence accumulated.

## Offline edit reconciliation

When a previously offline peer comes back online, the order of
reconnection matters for what they see first but not for final
state. Typical flow:

1. **Git fetch and merge.** `git pull` brings every
   `changes/*.amc` file that landed while you were offline into
   your working tree. No claims are loaded yet; these are just
   new files.
2. **Claim library open.** The next CLI invocation opens the log,
   merges the new change files into the live Automerge document,
   and rebuilds the SQLite projection (`view.sqlite`) from the
   now-current heads.
3. **Beacon reconnect.** If the relay is configured, the client
   reopens the WS. It does not replay missed frames; beacon has
   no log to replay from. It begins relaying new appends going
   forward.

Final state is identical regardless of reconnect order. Git alone
is sufficient for correctness; beacon gives liveness during active
collaboration.

## Failure modes

**Beacon auth fails.** The WS returns a named error:

```text
beacon: BeaconAuthFailed
  reason: forge token rejected or expired
  action: refresh your GitLab token and retry
```

Refresh the token via your usual forge flow and retry. The library
backs off and reconnects on the next open.

**Divergent supersession chain.** Two offline peers can supersede
the same claim with different successors. On pull, the CRDT layer
accepts both supersession edges; the view then has two rival
"current" claims. This is surfaced by:

```bash
claim conflicts
```

which lists every claim with an unresolved supersession fan-out and
prompts the operator to append a fresh claim superseding both
rivals. A full interactive resolution subcommand is planned but not
yet shipped; track the design in the keaton
`research/graph-primitive/` thread.

**Concurrent compaction.** Compaction rewrites `snapshot.amc` from
the current log and is local-only. The library takes a
directory-level `claims/.lock` via `fs4` around any compaction;
readers are unaffected, a second compactor blocks until the first
releases.

## When you need beacon vs when git is enough

- **Single user, one or more devices.** Git only. Pull when you
  switch machines.
- **Multiple users, asynchronous workflow.** Git only. Pulls and
  pushes at natural breakpoints are enough.
- **Multiple users, live collaboration on the same spec.** Beacon.
  You want to see each other's appends as they happen.

Beacon is always optional. Disabling it never compromises
correctness.

## What NOT to put in the claim log

The substrate is for structured work claims. It is not an arbitrary
key-value store and not a blob store. Do not append:

- **Secrets.** The log is E2EE in transit and at rest but
  replicates to every peer with project membership. That is the
  wrong blast radius for credentials; use a secret manager.
- **Large binary blobs.** Images, PDFs, datasets, model weights.
  The CRDT engine loads the full document into memory, so big
  blobs make every open slow for every peer. Reference them by
  URL or content hash and store the bytes elsewhere.
- **Ephemeral conversation state.** A claim is asserted forever.
  Use a scratchpad, not the log, for thinking out loud.

If the thing you are storing is not a typed assertion that a tool
will later read to make a decision, it does not belong in the log.

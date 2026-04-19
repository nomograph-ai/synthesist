# Sync Architecture

`nomograph-claim` stores state as an append-only CRDT log under a
visible `claims/` directory. Two sync channels carry those claims
between peers: **git** and **beacon**. Both are optional, independent,
and designed so a user with only one of them still gets correct
eventual state.

This document explains how the two channels interact, what happens
when one is unavailable, how offline edits reconcile, and the failure
modes operators are most likely to hit.

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

`git pull` is how peer changes arrive. Because the log is a CRDT, a
merge is not a rebase. Two collaborators appending simultaneously
produce two new files under `changes/`, and both land cleanly on
`git pull`. The Automerge engine reconciles the document on next
open. There is no textual conflict to resolve at the git layer.

`git push` is how your changes leave. A push is not a commit of
intent; it is a replication of claims you have already asserted
locally.

### Beacon: realtime relay

[Beacon](https://gitlab.com/nomograph/beacon) is a Cloudflare
Workers + Durable Object WebSocket relay. When enabled, the claim
library opens a persistent WS connection, authenticates via a forge
token (GitLab, today), and ships each local append as a ciphertext
frame to any subscribed peer on the same project channel.

Key properties:

- **Forge-mediated auth.** Beacon does not issue credentials. It
  verifies your GitLab token against project membership and
  short-circuits any connection that fails. The relay does not
  know who you are beyond "this token belongs to a member of
  project X".
- **Blind relay.** Claim content is end-to-end encrypted with
  ChaCha20-Poly1305 using a key derived from the project secret.
  Beacon sees an opaque ciphertext, a project id, and a message
  envelope. It cannot read claims.
- **No durability guarantee.** Beacon is a fan-out hose, not a log.
  If a peer is offline at relay time, they catch up via git, not
  beacon replay.

## What happens when one channel is down

### Beacon down, git up

This is effectively synthesist v1 semantics on the v2 substrate.
Local appends go to `claims/changes/` as usual. You `git pull` to
receive peer changes and `git push` to publish your own. CRDT merges
happen on open. You lose realtime peer awareness, meaning someone
else opening the same spec does not see your partial work until a
push and pull round-trip, but nothing is corrupted.

### Beacon up, git down

Local appends still write to `claims/changes/` on disk and also
stream to the relay, so connected peers see your work in real time.
When git access comes back, the same files push up with no special
handling. Nothing in the beacon pipeline depends on git being
reachable.

### Both down

Pure local. You continue to append; the log grows on your disk.
On reconnect to either channel, CRDT reconciles whatever divergence
accumulated. This is the point of a CRDT: peers can diverge
arbitrarily and converge on first contact.

## Offline edit reconciliation

When a previously offline peer comes back online, the order of
operations matters for what they see first, but not for final
state.

Typical reconnect flow:

1. **Git fetch and merge.** `git pull` brings every `changes/*.amc`
   file that landed while you were offline into your working tree.
   At this point, no claims have been loaded; it is just new files.
2. **Claim library open.** The next CLI invocation opens the log,
   sees unloaded change files, and merges them into the live
   Automerge document. The local SQLite projection (`view.sqlite`)
   is rebuilt from the now-current heads.
3. **Beacon reconnect.** If the relay is configured, the client
   reopens the WS. It does not replay missed frames; beacon has no
   log to replay from. It begins relaying new appends going
   forward.

Final state is identical regardless of which channel you reconnect
first. Git alone is sufficient for correctness; beacon alone gives
you liveness during an active collaboration.

## Failure modes

### Beacon auth fails

The WS connection returns a named error:

```text
beacon: BeaconAuthFailed
  reason: forge token rejected or expired
  action: refresh your GitLab token and retry
```

Refresh the token via your usual forge flow, set it in the expected
env var or config, and retry. The library backs off and reconnects
automatically on the next open.

### Git merge with divergent supersession chain

Two peers can, while offline, supersede the same claim with
different successors. When they pull, the CRDT layer accepts both
supersession edges but the spec-level view has two rival "current"
claims. This is surfaced by:

```bash
claim conflicts
```

which lists every claim with an unresolved supersession fan-out and
prompts the operator to append a fresh claim that supersedes both
rivals. A full conflicts subcommand with interactive resolution is
planned but not yet shipped; track the design in the keaton
`research/graph-primitive/` thread.

### Concurrent compaction

Compaction rewrites `snapshot.amc` from the current log and is a
local-only operation. Two processes compacting at once would race.
The library takes a directory-level `claims/.lock` via `fs4`
(filesystem advisory lock) around any compaction. Concurrent readers
are unaffected; a second compactor blocks until the first releases.

## When you need beacon vs when git is enough

Rule of thumb:

- **Single user, one device.** Git only. Nothing to gain from
  beacon.
- **Single user, multiple devices.** Git only. Pull when you switch
  machines.
- **Two or more users, asynchronous workflow.** Git only. Pulls and
  pushes at natural breakpoints are enough.
- **Two or more users, live collaboration on the same spec.**
  Beacon. You want to see each other's appends as they happen, and
  you are not going to push on every keystroke.

Beacon is always optional. Disabling it never compromises
correctness.

## What NOT to put in the claim log

The substrate is for structured work claims. It is not an arbitrary
key-value store and not a blob store. Do not append:

- **Secrets.** Anything an attacker could use is better stored in
  a secret manager. The log is E2EE in transit and at rest but
  replicates to every peer with project membership, which is the
  wrong blast radius for credentials.
- **Large binary blobs.** Images, PDFs, datasets, model weights. The
  CRDT engine loads the full document into memory. Big blobs make
  every open slow for every peer. Reference them by URL or content
  hash and store the bytes elsewhere.
- **Ephemeral conversation state.** A claim is asserted forever. Use
  a scratchpad, not the log, for thinking out loud.

If the thing you are storing is not a typed assertion that a tool
will later read to make a decision, it does not belong in the log.

# Identity and Asserter Trust

Every claim in `nomograph-claim` carries an `asserted_by` field
naming who asserted it. In v0.1, that field is an **advisory
display string**, not an authenticated identity. This document
explains what v0.1 does, why, and how v0.2 will close the gap.

Related reading:

- [`SYNC.md`](./SYNC.md) for how claims replicate across git and
  beacon and where transport-layer auth fits in.
- [`synthesist/MIGRATION.md`](https://gitlab.com/nomograph/synthesist/-/blob/main/MIGRATION.md)
  for how v1 project state gets re-asserted with v2 attribution.

## v0.1 contract

`asserted_by` is a string with an enforced format. The library
rejects writes that do not match one of these shapes:

```text
user:<forge>:<username>
agent:<model>:<session>@<host>
ingest:<tool>:<adapter>
```

Examples:

```text
user:gitlab:andunn
agent:claude-opus-4-7:research-1@laptop
ingest:lattice:gitlab-mr-adapter
```

The format is validated. The content is not. Anyone with write
access to a project's `claims/` log can append a claim with any
well-formed asserter string. Nothing in v0.1 cryptographically binds
the string to the actor.

## Why v0.1 is this way

Two reasons, both deliberate.

**The local-trust model is sufficient for the deployments v0.1
targets.** Single-user repos and small-team estates share a trust
boundary at the repo itself. Anyone who can `git push` into the
project can already do whatever they want with its state, signed
or not. Adding per-claim signing on day one solves a problem these
users do not have and slows down the work that ships.

**Beacon provides transport-layer identity, not per-claim
authorship.** When the relay is enabled, every WebSocket
connection is authenticated against a forge token and short-circuits
if the token is not a project member. This gives you "a project
member asserted this", which is meaningful but coarse. It does not
let a downstream tool inspect one claim out of a bundle and say
"user X specifically authored this, and not a peer of X pretending
to be X". That proof requires per-claim signatures, which v0.1
does not have.

## What downstream tools MUST NOT do

If you are building a tool that reads the claim log,
specifically including [synthesist](https://gitlab.com/nomograph/synthesist),
[lattice](https://gitlab.com/nomograph/lattice), and future tools
like seer, the following rules apply in v0.1:

- **Never make an access-control decision based solely on
  `asserted_by`.** "This claim says it was asserted by `user:gitlab:root`,
  so I will grant admin privileges" is a vulnerability. The string
  is user-submitted.
- **Never expose a user-submitted `asserted_by` string as trusted
  identity.** Render it as audit metadata, clearly labeled. Do not
  render it as the author of a signed commit or the owner of a
  permission-gated resource.
- **Never use `asserted_by` to route notifications to external
  systems without a verification step.** "This claim is from
  `agent:...@host`, so I will ping that host" can be used to make
  your tool emit traffic toward anything a claim author names.

Treat asserter as a display hint and an audit breadcrumb. Nothing
more.

## v0.2 roadmap

Post-Josh-sync, v0.2 adds per-claim signatures:

1. **Sign on append.** When a user appends a claim, the library
   signs the claim hash with the user's SSH key (or a forge-bound
   key pair managed by the library).
2. **Parallel signature file.** The signature lands at
   `claims/signatures/<claim-hash>.sig`, committed alongside the
   claim itself. Signatures replicate through git like any other
   file.
3. **Beacon validation.** The relay verifies the signature before
   fanning out to peers. Unsigned or badly-signed claims are
   rejected at the relay boundary in addition to the library
   boundary.
4. **Authorized asserter check.** Downstream tools can call a new
   `claim verify <id>` primitive that returns a bool plus the
   verified signer. Permission checks that need "this specific
   user said this" gain a real answer.

v0.2 does not remove the v0.1 contract. Claims written under v0.1
remain valid and readable; they simply lack signature files and
`claim verify` returns `unverified` for them. The signed-or-not
distinction is surfaced in query output.

## For contributors writing new tools today

While v0.1 is in force:

- Treat `asserted_by` as a **display hint** for audit rendering
  and nothing else.
- If your tool needs authorization, **require it at the input
  layer**: forge token, env config, or a side-channel credential
  check. Do not derive authorization from claim content.
- If your tool needs attribution for reporting purposes (for
  example, "who claimed this task in the last week"), read
  `asserted_by` freely; it is fine for reporting and audit views
  that are not security-sensitive.
- If you are tempted to write a feature where a decision hinges on
  claim authorship being genuine, **wait for v0.2** or add a
  non-claim-log verification path. We will not backport
  authentication into v0.1 contracts.

When v0.2 ships, the migration for downstream tools is additive:
add a signature-check call at decision points, leave display paths
alone.

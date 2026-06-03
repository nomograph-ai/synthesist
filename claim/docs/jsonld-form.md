# JSON-LD Form Specification (v3 Substrate)

**Status**: Draft, v3-alpha
**Date**: 2026-05-29
**Owner**: nomograph-claim

This document defines the compact JSON-LD form the v3 substrate writes to
disk. It is the contract between the substrate runtime and any tool that
writes or reads claims (synthesist, future modules, the migration tool).

## Goals

1. **Grep-able on disk.** A reader who opens
   `claims/<asserter>/log.jsonl` should see readable JSON with
   short prefixed predicates, not expanded IRIs.
2. **Conformant.** The form is valid JSON-LD; any conformant parser
   round-trips the file to the same triples.
3. **Content-proportional in bytes.** Per-claim overhead is on the
   order of 200 to 400 bytes; the rest is payload.
4. **Module-extensible.** Synthesist adds `synthesist:` predicates;
   future modules add their own. The substrate enforces only the
   universal envelope.

## The base @context

Every claim references the base @context by URI:

```
"@context": "https://nomograph.org/v3/context.jsonld"
```

The body of that context is embedded in `nomograph-claim` as the
constant [`BASE_CONTEXT_BODY`](../src/jsonld.rs). Tools embed a local
copy of the body so parsing works offline without a network fetch.

The base @context declares:

| Prefix | IRI |
|---|---|
| `nomograph` | `https://nomograph.org/v3/` |
| `prov` | `http://www.w3.org/ns/prov#` |
| `xsd` | `http://www.w3.org/2001/XMLSchema#` |

And the following predicate typings:

- `prov:generatedAtTime` is typed as `xsd:dateTime`.
- `prov:wasAttributedTo` is typed as `@id`.
- `prov:wasRevisionOf` is typed as `@id` (universal supersession edge).
- `nomograph:parentAsserter` is typed as `@id` (agent hierarchy audit).

## Per-module contexts

Each module (synthesist, others) ships its own context for predicates
in its vocabulary. The module context layers on top of the base via
JSON-LD's array form. When writing a claim, the tool merges the base
context and the module context:

```jsonld
{
  "@context": [
    {"prov": "http://www.w3.org/ns/prov#", "xsd": "...", ...},
    {"synthesist": "https://nomograph.org/synthesist/", "synthesist:depends_on": {"@type": "@id", "@container": "@set"}, ...}
  ],
  "@id": "synthesist:claim/abc123",
  ...
}
```

For ergonomic on-disk lines, tools may emit the URI form
(`"@context": "https://nomograph.org/v3/context.jsonld"`) and rely on
the embedded body. The substrate accepts both forms.

## Required envelope predicates

Every claim must carry:

- `@id` -- a stable IRI of the form `<module>:claim/<hash>`.
- `@type` -- the claim type IRI in the module's namespace (e.g.,
  `synthesist:Task`).
- `prov:generatedAtTime` -- xsd:dateTime in millisecond precision,
  trailing `Z`. Example: `"2026-05-29T01:00:00.123Z"`.
- `prov:wasAttributedTo` -- an asserter IRI of the form
  `asserter:<class>:<scope>:<id>[:<session>]`. The `asserter:` prefix
  is the substrate's convention.

## Optional envelope predicates

- `<module>:supersedes` (or equivalently `prov:wasRevisionOf`) -- an
  IRI pointing at the claim this one supersedes. Modules may name
  their own supersession predicate; the substrate treats either as
  the supersession edge.
- `nomograph:parentAsserter` -- an asserter IRI naming the parent of
  the asserter that wrote this claim. Used for agent hierarchy audit;
  the canonical example is an LLM agent session whose parent is the
  user that spawned it.

## Module-specific predicates

Predicates in a module's namespace (e.g., `synthesist:status`,
`synthesist:depends_on`, `synthesist:summary`) are defined and validated by the
module, not by the substrate. Predicate typing is declared in the
module's @context; predicate semantics are the module's concern.

The substrate verifies that the universal envelope is present and
well-formed. It does not enforce the module's per-type constraints;
that is the synthesist (or other module) validator's job.

## Hash and content addressing

A claim's hash is a content-addressed identifier computed by the
writer. The convention for v3-alpha is a blake3 hash over the
canonical-JSON form of the claim's required fields (`claim_type`,
`props`, `prov:generatedAtTime`, `prov:wasAttributedTo`,
`<module>:supersedes` if any). The substrate accepts the writer's
hash; it does not recompute.

For brevity in IRIs, the hash is truncated to the first 16 hex
characters in the on-disk form. Collisions at that truncation are
detectable by the substrate (two distinct claims with the same
truncated @id is an error); in practice they do not occur at the
volumes synthesist handles.

## Asserter IRI form

Asserter IRIs in JSON-LD use the `asserter:` IRI prefix. The full
asserter string follows the asserter convention:

```
<class>:<scope>:<id>[:<session>]
```

Where `<class>` is `user`, `agent`, or `ingest`. Examples:

- `asserter:user:local:agd`
- `asserter:user:local:agd:edc-bootstrap`
- `asserter:agent:claude-opus-4-7:sess-abc123`
- `asserter:ingest:gitlab:nomograph-keaton`

The `asserter:` prefix is a substrate convention; it is not part of
the asserter string itself. Tools that round-trip an asserter IRI
strip the prefix to get the asserter string.

## Datatypes

- Timestamps: `xsd:dateTime`, RFC 3339, millisecond precision, `Z`
  suffix (UTC). Example: `"2026-05-29T01:00:00.123Z"`.
- IRIs: bare strings in the JSON-LD doc; typed via the @context
  (`{"@type": "@id"}`).
- Booleans, numbers, strings: native JSON types.
- Lists: JSON arrays. For unordered set semantics, the @context can
  declare `"@container": "@set"` (the synthesist context does this
  for `synthesist:depends_on`).

## On-disk file format

`claims/<asserter>/log.jsonl` is one JSON-LD doc per line, with a
trailing newline on every line. Each line must:

- Parse as valid JSON.
- Carry a single JSON-LD doc (one claim per line, no arrays-of-claims).
- Be UTF-8 encoded.
- Not contain unescaped newlines inside JSON strings.

The substrate's log writer enforces these properties at write time.

## Round-trip guarantee

If a claim is written to disk by the substrate and read back, parsing
the line with any conformant JSON-LD parser produces exactly the same
triples that the substrate's reader would produce. The CI gate for
`nomograph-claim` includes a round-trip test against `oxjsonld` that
proves this for the required envelope.

## What this spec deliberately does NOT cover

- Per-module schemas (claim types, required fields). Modules define
  their own.
- Validation rules beyond structural well-formedness. Modules run
  their own validators.
- Migration from v2 `.amc` format. See `synthesist/src/migrate/` for
  that.
- SHACL shapes. Shapes ship with the module that defines them and
  are documentation-only artifacts (not enforced at the substrate
  layer).

## Companion artifacts

- [`src/jsonld.rs`](../src/jsonld.rs) -- constants and helpers.
- [Proposal 002 (synthesist)](https://gitlab.com/nomograph/synthesist/-/blob/main/docs/proposals/002-graph-substrate.md)
  -- the design rationale.

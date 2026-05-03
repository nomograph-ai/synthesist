# Performance options matrix (t3)

Evaluation of acceleration ideas against the spec constraints for **`nomograph/crdt-storage-performance`** (CRDT semantics, append-only log, disposable caches).

Legend: **✓** compatible if designed carefully, **~** partial / needs proof, **✗** violates non-negotiables or is unsafe as stated.

## View / SQLite path

| Idea | Fit | Notes |
|------|-----|--------|
| **Incremental view update** (delta from prior heads) | ~ | Would avoid full `DROP TABLE` + full insert; must prove correctness when supersessions and duplicates exist (`INSERT OR IGNORE` semantics today). High engineering cost in `nomograph-claim`. |
| **Defer `sync_view` until read** | ~ | Could batch writes but reads must not see stale typed state; workflow today assumes sync-after-append. Would need explicit consistency rules at API boundary. |
| **Batch append + single sync** (CLI or library) | ✓ | Multiple claims per transaction **without** relaxing CRDT rules if still appended as separate Automerge commits or one validated batch API. Requires API/design work. |
| **SQLite pragma tuning** (cache size, mmap) | ✓ | Local optimization only; validate on CI + large fixtures. |

## Automerge / disk path

| Idea | Fit | Notes |
|------|-----|--------|
| **`compact()` / snapshot more often** | ✓ | Shrinks `changes/*.amc` count; speeds **`Store::open`**. Trade-off: compaction itself is heavy; must not run concurrently with append without locks (already serialized via flock). |
| **Parallel `load_incremental`** | ✗ | Change files have order dependency; parallel replay breaks causal ordering unless proven safe. |
| **Reduce fsync frequency** | ✗ | Durability guarantees (atomic writes + `fsync_dir`) exist for crash safety; weakening risks corrupted or invisible claims. |
| **Smaller props / fewer claims** | ✓ | Product/process lever — fewer superseding writes reduces merge volume (not a substrate shortcut). |

## Rejected shortcuts (explicit)

| Shortcut | Why reject |
|----------|------------|
| **Treat `view.sqlite` as source of truth** | Violates derived-cache model; conflicts with CRDT audit story. |
| **Single-writer lock instead of CRDT merge** | Violates multi-writer design goal. |
| **Skip validation on append** | Violates boundary validation contract in workflow adapter. |

## Next step

See **`docs/perf-recommendation.md`** for a ranked plan and validation checklist.

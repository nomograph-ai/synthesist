//! Performance harness for the v2 claim stack (`SynthStore` → Automerge + SQLite view).
//!
//! Run (release artifacts only; copy/setup time is excluded from Criterion timings via
//! `iter_batched` where applicable):
//!   cargo bench -p nomograph-synthesist --bench store
//!
//! Claims directory resolution (first match wins):
//!   1. `SYNTHESIST_BENCH_CLAIMS` — absolute or relative path to the `claims/` directory
//!      (must contain `genesis.amc`).
//!   2. `SYNTHESIST_DIR` — parent directory; uses `<SYNTHESIST_DIR>/claims` when that path exists.
//!   3. `<repo>/claims` via `CARGO_MANIFEST_DIR`.
//!
//! Large estates: copy your production `claims/` elsewhere and point `SYNTHESIST_BENCH_CLAIMS`
//! at it. Directory-copy setup is timed separately by Criterion only when it appears inside the
//! measured closure; batched benches keep copies in `setup`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use criterion::{BatchSize, Criterion, Throughput, black_box, criterion_group, criterion_main};
use nomograph_claim::ClaimType;
use nomograph_synthesist::store::SynthStore;
use serde_json::json;

static APPEND_SEQ: AtomicU64 = AtomicU64::new(0);

fn resolve_claims_template() -> PathBuf {
    if let Ok(raw) = std::env::var("SYNTHESIST_BENCH_CLAIMS") {
        let p = PathBuf::from(raw);
        if p.join("genesis.amc").is_file() {
            return p;
        }
        panic!(
            "SYNTHESIST_BENCH_CLAIMS={} does not contain genesis.amc",
            p.display()
        );
    }
    if let Ok(root) = std::env::var("SYNTHESIST_DIR") {
        let claims = PathBuf::from(root).join(nomograph_synthesist::store::CLAIMS_DIR);
        if claims.join("genesis.amc").is_file() {
            return claims;
        }
    }
    let fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("claims");
    if !fallback.join("genesis.amc").is_file() {
        panic!(
            "No claims fixture: run `synthesist init` or set SYNTHESIST_BENCH_CLAIMS / SYNTHESIST_DIR \
             (expected genesis.amc under {})",
            fallback.display()
        );
    }
    fallback
}

fn copy_claims_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".lock" {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            copy_claims_tree(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn bench_cold_open_materialize(c: &mut Criterion) {
    let template = resolve_claims_template();
    let mut group = c.benchmark_group("cold_open_materialize_sqlite");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(5));
    group.bench_function("open_at_after_stale_heads", |b| {
        b.iter_batched(
            || {
                let tmp = tempfile::tempdir().unwrap();
                let dst = tmp.path().join("claims");
                copy_claims_tree(&template, &dst).unwrap();
                let heads = dst.join("view.heads");
                let _ = fs::remove_file(&heads);
                (tmp, dst)
            },
            |(_tmp, dst)| {
                let store = SynthStore::open_at(&dst).unwrap();
                black_box(store.root());
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_warm_open(c: &mut Criterion) {
    let template = resolve_claims_template();
    let mut group = c.benchmark_group("warm_open");
    group.sample_size(30);
    group.bench_function("open_at_heads_current", |b| {
        b.iter_batched(
            || {
                let tmp = tempfile::tempdir().unwrap();
                let dst = tmp.path().join("claims");
                copy_claims_tree(&template, &dst).unwrap();
                (tmp, dst)
            },
            |(_tmp, dst)| {
                let store = SynthStore::open_at(&dst).unwrap();
                black_box(store.root());
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_append_session(c: &mut Criterion) {
    let template = resolve_claims_template();
    let mut group = c.benchmark_group("append_session_claim");
    group.throughput(Throughput::Elements(1));
    group.bench_function("append_includes_view_sync", |b| {
        b.iter_batched(
            || {
                let tmp = tempfile::tempdir().unwrap();
                let dst = tmp.path().join("claims");
                copy_claims_tree(&template, &dst).unwrap();
                let store = SynthStore::open_at(&dst).unwrap();
                (tmp, store)
            },
            |(_tmp, mut store)| {
                let n = APPEND_SEQ.fetch_add(1, Ordering::Relaxed);
                let id = store
                    .append(
                        ClaimType::Session,
                        json!({ "id": format!("bench-session-{n}") }),
                        None,
                    )
                    .unwrap();
                black_box(id);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_sync_view_rebuild(c: &mut Criterion) {
    let template = resolve_claims_template();
    let mut group = c.benchmark_group("sync_view");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(5));
    group.bench_function("rebuild_after_heads_file_removed", |b| {
        b.iter_batched(
            || {
                let tmp = tempfile::tempdir().unwrap();
                let dst = tmp.path().join("claims");
                copy_claims_tree(&template, &dst).unwrap();
                let store = SynthStore::open_at(&dst).unwrap();
                let heads = store.root().join("view.heads");
                fs::remove_file(&heads).unwrap();
                (tmp, store)
            },
            |(_tmp, mut store)| {
                store.sync_view().unwrap();
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_sync_view_noop(c: &mut Criterion) {
    let template = resolve_claims_template();
    let mut group = c.benchmark_group("sync_view_noop");
    group.bench_function("heads_already_match", |b| {
        b.iter_batched(
            || {
                let tmp = tempfile::tempdir().unwrap();
                let dst = tmp.path().join("claims");
                copy_claims_tree(&template, &dst).unwrap();
                let store = SynthStore::open_at(&dst).unwrap();
                (tmp, store)
            },
            |(_tmp, mut store)| {
                store.sync_view().unwrap();
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_query_claims(c: &mut Criterion) {
    let template = resolve_claims_template();
    let mut group = c.benchmark_group("query_view_sqlite");
    group.throughput(Throughput::Elements(1));
    group.bench_function("select_count_star_claims", |b| {
        b.iter_batched(
            || {
                let tmp = tempfile::tempdir().unwrap();
                let dst = tmp.path().join("claims");
                copy_claims_tree(&template, &dst).unwrap();
                let store = SynthStore::open_at(&dst).unwrap();
                (tmp, store)
            },
            |(_tmp, store)| {
                let rows = store
                    .query("SELECT COUNT(*) AS n FROM claims", &[])
                    .unwrap();
                black_box(rows);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_cold_open_materialize,
    bench_warm_open,
    bench_append_session,
    bench_sync_view_rebuild,
    bench_sync_view_noop,
    bench_query_claims,
);
criterion_main!(benches);

use std::{hint::black_box, time::Duration};

use chunk_pipeline::{
    MAX_LOD_WRITERS,
    benchmark_support::{LodMeshFixture, LodSampleFixture, LodWriterFixture},
};
use criterion::{BenchmarkId, Criterion, Throughput};
use lod_store::{
    actor::{ActorConfig, Request, StoreActor},
    identity::{Dimension, StoreKey},
    minute::Minute,
    root::{Quotas, StoreRoot},
    store::StoreOptions,
};

/// Times one distant terrain sample job's capture, content hash, level-0 sample and surface
/// signature; the store's hash check and the writer's merge are not included.
pub fn benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline/lod_sample_job");
    for sub_chunks in [4_usize, 24] {
        let fixture = LodSampleFixture::new(sub_chunks);
        assert_eq!(fixture.sample().1, sub_chunks);
        group.throughput(Throughput::Elements(sub_chunks as u64));
        group.bench_with_input(
            BenchmarkId::new("column", sub_chunks),
            &fixture,
            |b, fixture| b.iter(|| black_box(fixture.sample())),
        );
    }
    group.finish();
    writers(c);
    meshes(c);
}

/// Times the writer threads merging and encoding 32 × 32 sampled fixture columns into a
/// fresh store, with one writer and with [`MAX_LOD_WRITERS`]: centred on a corner where
/// four writer regions meet, the best split, and inside one writer region, where one
/// writer takes every column.
fn writers(c: &mut Criterion) {
    let fixtures = [
        ("corner", LodWriterFixture::new(-16, 32)),
        ("inside", LodWriterFixture::new(0, 32)),
    ];
    let mut group = c.benchmark_group("pipeline/lod_writers");
    group.sample_size(10);
    for (name, fixture) in &fixtures {
        group.throughput(Throughput::Elements(fixture.len() as u64));
        for writers in [1, MAX_LOD_WRITERS] {
            group.bench_with_input(
                BenchmarkId::new(format!("{name}_columns"), writers),
                &writers,
                |b, &writers| {
                    b.iter_custom(|iterations| {
                        (0..iterations)
                            .map(|_| ingest_into_fresh_store(fixture, writers))
                            .sum::<Duration>()
                    });
                },
            );
        }
    }
    group.finish();
}

/// Times mesh jobs end to end on the level-0 sections of 16 × 16 merged fixture columns:
/// wanting them all from a fresh mesher until every mesh was taken, idle and while
/// [`MAX_LOD_WRITERS`] writers merge another 16 × 16 columns into the same store. Prints
/// p50 and p99 of the time from the want until each section's mesh was taken.
fn meshes(c: &mut Criterion) {
    let meshed = LodWriterFixture::new(0, 16);
    let load = LodWriterFixture::new(64, 16);
    let fixture = LodMeshFixture::new(&meshed);
    let dir = tempfile::tempdir().expect("temp dir");
    let actor = open_store(dir.path());
    meshed.ingest(actor.client(), MAX_LOD_WRITERS);
    let mut group = c.benchmark_group("pipeline/lod_mesh_jobs");
    group.sample_size(10);
    group.throughput(Throughput::Elements(fixture.len() as u64));
    for loaded in [false, true] {
        let name = if loaded { "under_ingestion" } else { "idle" };
        let mut latencies = Vec::new();
        group.bench_function(format!("l0_sections_{name}"), |b| {
            b.iter_custom(|iterations| {
                (0..iterations)
                    .map(|_| {
                        let run =
                            fixture.mesh(actor.client(), loaded.then(|| (&load, actor.client())));
                        latencies.extend(run.latencies);
                        run.elapsed
                    })
                    .sum::<Duration>()
            });
        });
        latencies.sort_unstable();
        if let (Some(p50), Some(p99)) = (
            latencies.get(latencies.len() / 2),
            latencies.get((latencies.len() * 99).div_ceil(100).saturating_sub(1)),
        ) {
            eprintln!(
                "pipeline/lod_mesh_jobs {name}: {} sections, want-to-taken p50 {p50:?}, p99 {p99:?}",
                fixture.len()
            );
        }
    }
    group.finish();
    actor.shutdown(Minute::new(1));
}

/// A store in `dir` with the overworld session begun, behind its write-back thread.
fn open_store(dir: &std::path::Path) -> StoreActor {
    let root = StoreRoot::new(dir);
    let store = root
        .open(
            &StoreKey::new("remote:lod-writers", "", 0),
            Minute::new(1),
            StoreOptions::default(),
        )
        .expect("open store");
    let mut actor = StoreActor::spawn(store, root, Quotas::default(), ActorConfig::default())
        .expect("spawn store thread");
    actor
        .try_submit(Request::Begin {
            dimension: Dimension(0),
            now: Minute::new(1),
        })
        .expect("begin session");
    actor
}

/// [`LodWriterFixture::ingest`] into a store in a new temporary directory.
fn ingest_into_fresh_store(fixture: &LodWriterFixture, writers: usize) -> Duration {
    let dir = tempfile::tempdir().expect("temp dir");
    let actor = open_store(dir.path());
    let elapsed = fixture.ingest(actor.client(), writers);
    actor.shutdown(Minute::new(1));
    elapsed
}

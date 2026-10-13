//! Region file costs on synthetic section records: batched inserts, hot and cold reads,
//! and compaction. Hardware captures, not these, check streaming budgets.

#[cfg(feature = "benchmark-mimalloc")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::{hint::black_box, path::Path};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use lod::{
    codec::SectionCodec,
    geometry::{SectionKey, SectionPos},
    section::{ChildMask, Section},
    storage::{DenseSection, PackScratch},
    voxel::{FULL_COVERAGE, Light, MaterialId, Voxel},
};
use lod_store::{
    region_file::{Access, RegionFile},
    region_id::RegionId,
    region_set::{DEFAULT_OPEN_REGIONS, RegionSet},
};
use tempfile::TempDir;

/// Records per insert batch, about one write-back flush.
const BATCH: usize = 256;
/// Records stored before the read benches, all in one level-0 region.
const STORED: usize = 256;
/// Block materials per synthetic section; terrain surfaces mix about this many.
const MATERIALS: u32 = 8;

fn key(index: usize) -> SectionKey {
    let index = index as i32;
    SectionPos::new(0, index % 32, index / 1024, (index / 32) % 32)
        .expect("bench position")
        .key()
}

/// A terrain-like record: a solid lower half of scattered materials under air.
fn record(codec: &mut SectionCodec, key: SectionKey, seed: u32) -> Vec<u8> {
    let mut dense = DenseSection::filled(Voxel::air(Light::FULL_SKY), seed % 5);
    for (index, voxel) in dense.voxels_mut().iter_mut().enumerate() {
        let y = index / 1024;
        let noise = (index as u32 ^ seed).wrapping_mul(2_654_435_761) >> 20;
        if y < 12 + (noise % 4) as usize {
            let material = 1 + (noise / 7) % MATERIALS;
            *voxel = Voxel::new(
                MaterialId::new(material).expect("block material"),
                Light::new(0, 0),
                FULL_COVERAGE,
                false,
            );
        }
    }
    let section = Section::from_dense(
        key.pos(),
        &dense,
        ChildMask::UNKNOWN,
        &mut PackScratch::default(),
    )
    .expect("unknown children are valid");
    codec.encode(&section)
}

fn records(count: usize, seed: u32) -> Vec<Vec<u8>> {
    let mut codec = SectionCodec::new();
    (0..count)
        .map(|index| record(&mut codec, key(index), seed + index as u32))
        .collect()
}

/// A committed store of [`STORED`] records under `root`.
fn stored_set(root: &Path, records: &[Vec<u8>]) -> RegionSet {
    let mut set = RegionSet::open(root, Access::ReadWrite, DEFAULT_OPEN_REGIONS);
    for bytes in records {
        set.put(bytes).expect("put");
    }
    assert!(set.commit_all());
    set
}

fn insert_batch(criterion: &mut Criterion) {
    let batch = records(BATCH, 0);
    let mut group = criterion.benchmark_group("insert");
    group.throughput(Throughput::Elements(BATCH as u64));
    group.bench_function("batch_commit", |bench| {
        bench.iter_batched(
            || {
                let dir = TempDir::new().expect("temp dir");
                let set = RegionSet::open(dir.path(), Access::ReadWrite, DEFAULT_OPEN_REGIONS);
                (dir, set)
            },
            |(dir, mut set)| {
                for bytes in &batch {
                    set.put(bytes).expect("put");
                }
                assert!(set.commit_all());
                (dir, set)
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn get(criterion: &mut Criterion) {
    let stored = records(STORED, 0);
    let dir = TempDir::new().expect("temp dir");
    drop(stored_set(dir.path(), &stored));
    let mut group = criterion.benchmark_group("get");

    let mut set = RegionSet::open(dir.path(), Access::ReadWrite, DEFAULT_OPEN_REGIONS);
    let mut out = Vec::new();
    let mut next = 0;
    group.bench_function("hot", |bench| {
        bench.iter(|| {
            next = (next + 97) % STORED;
            assert!(set.get_into(key(next), &mut out).expect("get"));
            black_box(out.len())
        });
    });

    // A fresh handle per read: opening the file and loading its committed index.
    group.bench_function("cold_open", |bench| {
        bench.iter_batched(
            || {
                next = (next + 97) % STORED;
                let set = RegionSet::open(dir.path(), Access::ReadOnly, DEFAULT_OPEN_REGIONS);
                (set, key(next))
            },
            |(mut set, key)| {
                assert!(set.get_into(key, &mut out).expect("get"));
                black_box(out.len())
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn compaction(criterion: &mut Criterion) {
    let region = RegionId::of_key(key(0)).0;
    let rounds: Vec<_> = (0..3).map(|round| records(64, round * 1000)).collect();
    let mut group = criterion.benchmark_group("compaction");
    group.throughput(Throughput::Elements(64));
    group.bench_function("two_thirds_dead", |bench| {
        bench.iter_batched(
            || {
                let dir = TempDir::new().expect("temp dir");
                let path = dir.path().join(region.file_name());
                let mut file = RegionFile::open_writable(&path, region).expect("open");
                for round in &rounds {
                    for bytes in round {
                        file.put(bytes).expect("put");
                    }
                    file.commit().expect("commit");
                }
                (dir, file)
            },
            |(dir, mut file)| {
                black_box(file.compact().expect("compact"));
                (dir, file)
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

criterion_group!(benches, insert_batch, get, compaction);
criterion_main!(benches);

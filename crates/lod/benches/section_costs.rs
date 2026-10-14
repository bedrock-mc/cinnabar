//! Per-column and per-section costs of level-0 sampling, mipping and section records on the
//! synthetic fixture terrain; live captures, not these, check frame and streaming budgets.

#[cfg(feature = "benchmark-mimalloc")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::{
    collections::{BTreeMap, HashMap},
    convert::Infallible,
    hint::black_box,
    sync::Arc,
};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use lod::{
    classify::TableClassifier,
    codec::SectionCodec,
    exposure::Neighbourhood,
    fixtures::{biome_column, fixture_classifier, terrain_column},
    geometry::{SectionKey, SectionPos, VoxelBox},
    ingest::{SectionSource, merge_columns},
    mip::{ChildInput, LOCAL_MIP_TOP_LEVEL, mip_chain, remip_from_children, unknown_section},
    sampler::{ColumnSample, ColumnSnapshot, SampledSubChunk, sample_column},
    section::{ChildMask, ChildState, Section},
    storage::{DenseSection, PackScratch, PackedVoxels},
};
use world::{
    ChunkKey, DecodedBiomeColumn, LightStore, LightStoreSnapshot, SubChunk, SubChunkKey,
    SubChunkLight,
};

/// Neighbourhoods of a local chain's mipped sections; the fixture columns have no stored
/// neighbours.
const CHAIN_NEIGHBOURS: [Neighbourhood; LOCAL_MIP_TOP_LEVEL as usize] =
    [Neighbourhood::UNKNOWN; LOCAL_MIP_TOP_LEVEL as usize];

/// Sub-chunk Ys of every fixture column: the terrain, its caves and the air above it.
const SUB_CHUNK_YS: std::ops::Range<i32> = 0..8;

struct Column {
    sub_chunks: BTreeMap<i32, Arc<SubChunk>>,
    biomes: DecodedBiomeColumn,
}

impl Column {
    fn new(chunk_x: i32, chunk_z: i32) -> Self {
        Self {
            sub_chunks: terrain_column(chunk_x, chunk_z, SUB_CHUNK_YS),
            biomes: biome_column(0, &[1, 1, 2, 2, 3, 3, 4, 4]),
        }
    }

    /// Uniform solved light for every sub-chunk: sky 15 above the terrain, dark below.
    fn light(&self, chunk_x: i32, chunk_z: i32) -> LightStoreSnapshot {
        let mut store = LightStore::default();
        for &y in self.sub_chunks.keys() {
            let sky = if y >= 3 { 15 } else { 0 };
            store.insert_resident(
                SubChunkKey::from_chunk(ChunkKey::new(0, chunk_x, chunk_z), y),
                SubChunkLight::uniform(0, sky, 1).expect("light in range"),
            );
        }
        store.snapshot()
    }

    fn sample(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        light: Option<&LightStoreSnapshot>,
        classifier: &mut TableClassifier,
    ) -> ColumnSample {
        let snapshot = ColumnSnapshot {
            chunk: ChunkKey::new(0, chunk_x, chunk_z),
            sub_chunks: &self.sub_chunks,
            biomes: &self.biomes,
            default_biome: 0,
            light,
        };
        sample_column(&snapshot, classifier).expect("fixture columns have keys")
    }
}

/// The level-0 section and its ancestors up to [`LOCAL_MIP_TOP_LEVEL`], parents unknown,
/// with their child masks.
fn fresh_chain(sub: &SampledSubChunk) -> (Vec<DenseSection>, Vec<ChildMask>) {
    let levels = usize::from(LOCAL_MIP_TOP_LEVEL) + 1;
    let mut chain: Vec<_> = (0..levels).map(|_| unknown_section()).collect();
    sub.write_into(&mut chain[0]);
    (chain, vec![ChildMask::UNKNOWN; levels])
}

fn sampling(c: &mut Criterion) {
    let column = Column::new(3, -2);
    let light = column.light(3, -2);
    let mut classifier = fixture_classifier();
    let mut group = c.benchmark_group("sample");
    group.throughput(Throughput::Elements(SUB_CHUNK_YS.len() as u64));
    group.bench_function("column_8_sub_chunks_fallback_light", |b| {
        b.iter(|| black_box(column.sample(3, -2, None, &mut classifier)));
    });
    group.bench_function("column_8_sub_chunks_solved_light", |b| {
        b.iter(|| black_box(column.sample(3, -2, Some(&light), &mut classifier)));
    });
    group.finish();
}

fn mipping(c: &mut Criterion) {
    let classes = fixture_classifier();
    let sample = Column::new(3, -2).sample(3, -2, None, &mut fixture_classifier());
    let surface = &sample.sub_chunks[2];
    let mut group = c.benchmark_group("mip");
    group.bench_function("local_chain_new_sub_chunk", |b| {
        b.iter_batched(
            || fresh_chain(surface),
            |(mut chain, mut masks)| {
                black_box(mip_chain(
                    &mut chain,
                    &mut masks,
                    &CHAIN_NEIGHBOURS,
                    surface.section(),
                    surface.region(),
                    &classes,
                ));
                (chain, masks)
            },
            BatchSize::LargeInput,
        );
    });
    let (mut settled, mut settled_masks) = fresh_chain(surface);
    mip_chain(
        &mut settled,
        &mut settled_masks,
        &CHAIN_NEIGHBOURS,
        surface.section(),
        surface.region(),
        &classes,
    );
    group.bench_function("local_chain_unchanged_sub_chunk", |b| {
        b.iter_batched(
            || (settled.clone(), settled_masks.clone()),
            |(mut chain, mut masks)| {
                surface.write_into(&mut chain[0]);
                black_box(mip_chain(
                    &mut chain,
                    &mut masks,
                    &CHAIN_NEIGHBOURS,
                    surface.section(),
                    surface.region(),
                    &classes,
                ));
                (chain, masks)
            },
            BatchSize::LargeInput,
        );
    });

    let children: Vec<DenseSection> = (0..8)
        .map(|index| {
            let column = Column::new(index % 2, index / 2 % 2);
            let sample = column.sample(index % 2, index / 2 % 2, None, &mut fixture_classifier());
            let mut section = unknown_section();
            for sub in &sample.sub_chunks[..2] {
                sub.write_into(&mut section);
            }
            section
        })
        .collect();
    let inputs: [ChildInput<'_>; 8] = std::array::from_fn(|index| ChildInput::Present {
        section: &children[index],
        state: ChildState::of_level_zero(&children[index]),
    });
    let mut out = unknown_section();
    group.bench_function("remip_from_eight_children", |b| {
        b.iter(|| {
            black_box(remip_from_children(
                inputs,
                &[Neighbourhood::UNKNOWN; 8],
                &classes,
                &mut out,
            ))
        });
    });
    group.finish();
}

fn records(c: &mut Criterion) {
    let classes = fixture_classifier();
    let sample = Column::new(3, -2).sample(3, -2, None, &mut fixture_classifier());
    let (mut chain, mut masks) = fresh_chain(&sample.sub_chunks[2]);
    sample.sub_chunks[3].write_into(&mut chain[0]);
    mip_chain(
        &mut chain,
        &mut masks,
        &CHAIN_NEIGHBOURS,
        sample.sub_chunks[2].section(),
        VoxelBox::FULL,
        &classes,
    );
    let mut scratch = PackScratch::default();
    let surface = Section::from_dense(
        sample.sub_chunks[2].section(),
        &chain[0],
        masks[0],
        &mut scratch,
    )
    .expect("level-0 mask");
    let level1_pos = surface.pos().parent().expect("level 1");
    let mipped =
        Section::from_dense(level1_pos, &chain[1], masks[1], &mut scratch).expect("level-1 mask");

    let mut codec = SectionCodec::new();
    let mut group = c.benchmark_group("record");
    group.throughput(Throughput::Elements(1));
    for (name, section, dense) in [
        ("l0_surface", &surface, &chain[0]),
        ("l1_mipped", &mipped, &chain[1]),
    ] {
        let record = codec.encode(section);
        let mut out = Vec::new();
        group.bench_function(format!("pack_{name}"), |b| {
            b.iter(|| black_box(PackedVoxels::from_dense(dense.voxels(), &mut scratch)));
        });
        group.bench_function(format!("encode_{name}"), |b| {
            b.iter(|| codec.encode_into(black_box(section), &mut out));
        });
        group.bench_function(format!("decode_{name}"), |b| {
            b.iter(|| black_box(codec.decode(black_box(&record)).expect("valid record")));
        });
    }
    group.finish();
}

/// Stored sections in memory, standing in for the store the writer reads.
#[derive(Clone, Default)]
struct Memory(HashMap<SectionKey, Section>);

impl SectionSource for Memory {
    type Error = Infallible;

    fn load(&mut self, pos: SectionPos) -> Result<Option<Section>, Infallible> {
        Ok(self.0.get(&pos.key()).cloned())
    }
}

/// One column's merge into the sections it shares and the encoding of every section it
/// rewrote: the writer's work per column after sampling.
fn ingest(c: &mut Criterion) {
    let classes = fixture_classifier();
    let first = Column::new(3, -2).sample(3, -2, None, &mut fixture_classifier());
    let beside = Column::new(4, -2).sample(4, -2, None, &mut fixture_classifier());
    let mut stored = Memory::default();
    let Ok(merged) = merge_columns([&first], &mut stored, &classes);
    for section in merged.sections {
        stored.0.insert(section.pos().key(), section);
    }
    let mut codec = SectionCodec::new();
    let mut out = Vec::new();
    let mut group = c.benchmark_group("ingest");
    group.throughput(Throughput::Elements(1));
    for (name, base) in [
        ("first_column", Memory::default()),
        ("beside_stored", stored),
    ] {
        let column = if base.0.is_empty() { &first } else { &beside };
        group.bench_function(format!("merge_and_encode_{name}"), |b| {
            b.iter_batched(
                || base.clone(),
                |mut source| {
                    let Ok(merged) = merge_columns([column], &mut source, &classes);
                    for section in &merged.sections {
                        codec.encode_into(section, &mut out);
                    }
                    merged.stats
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, sampling, mipping, records, ingest);
criterion_main!(benches);

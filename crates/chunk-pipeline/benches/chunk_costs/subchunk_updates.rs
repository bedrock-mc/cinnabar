use super::*;
use benchmark_support::MutationBurstFixture;
use protocol::{ActorBlockSyncMessage, BlockUpdateEvent, SyncedBlockUpdateEvent};
use world::{BLOCKS_PER_SUB_CHUNK, BlockUpdate, ChunkStore, RawBlockIds, SubChunk, SubChunkKey};

const PACKETS: usize = client_world::ingestion::MAX_ADMITTED_HEAVY_EVENTS;
const UPDATES_PER_PACKET: usize = 128;

/// Alternates opaque blocks and leaves changed cells in every section without starting a relight.
fn fixture(assets: &Arc<RuntimeAssets>, sections: usize) -> MutationBurstFixture {
    assert!(sections > 0 && PACKETS.is_multiple_of(sections));
    let packets_per_section = PACKETS / sections;
    let section = SubChunk::decode(&[9, 1, 0, 1, (SOLID << 1) as u8], &RawBlockIds { air: AIR });
    let packets = (0..PACKETS)
        .map(|packet| {
            let section_index = packet / packets_per_section;
            (0..UPDATES_PER_PACKET)
                .map(|cell| SyncedBlockUpdateEvent {
                    update: BlockUpdateEvent {
                        dimension: 0,
                        position: [
                            section_index as i32 * 16 + (cell >> 8) as i32,
                            (cell & 15) as i32,
                            ((cell >> 4) & 15) as i32,
                        ],
                        layer: 0,
                        network_id: if packet % 2 == 0
                            || packet % packets_per_section == packets_per_section - 1
                        {
                            SOLID_SWAPPED
                        } else {
                            SOLID
                        },
                    },
                    flags: 0,
                    sync: ActorBlockSyncMessage {
                        actor_unique_id: -1,
                        message: 0,
                    },
                })
                .collect()
        })
        .collect();
    MutationBurstFixture::new(new_stream(assets), packets, section)
}

/// Measures the same bounded burst's preparation CPU and complete worker-pool drain separately.
pub(super) fn benches(c: &mut Criterion) {
    let assets = Arc::new(terrain_assets());
    for sections in [1, 4] {
        let mut witness = fixture(&assets, sections);
        let jobs = witness.drain_worker_cpu();
        eprintln!(
            "synced_update_burst packets={PACKETS} sections={sections} cells_per_packet={UPDATES_PER_PACKET} worker_preparations={jobs}"
        );
        let mut group = c.benchmark_group("pipeline/synced_update_burst");
        group.sample_size(10);
        group.throughput(Throughput::Elements((PACKETS * UPDATES_PER_PACKET) as u64));
        group.bench_function(BenchmarkId::new("worker_cpu", sections), |b| {
            b.iter_batched_ref(
                || fixture(&assets, sections),
                |fixture| {
                    black_box(fixture.drain_worker_cpu());
                },
                BatchSize::PerIteration,
            );
        });
        group.bench_function(BenchmarkId::new("async_drain", sections), |b| {
            b.iter_batched_ref(
                || fixture(&assets, sections),
                |fixture| {
                    black_box(fixture.drain_async(CAMERA));
                },
                BatchSize::PerIteration,
            );
        });
        group.finish();
    }
    single_cell_benches(c);
}

/// Seeds retained counts through production batch mutation before measuring one prepared edit.
fn single_cell_fixture(name: &str) -> (Arc<SubChunk>, BlockUpdate) {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mut store = ChunkStore::new();
    let section = SubChunk::decode(&[9, 1, 0, 1, (SOLID << 1) as u8], &RawBlockIds { air: AIR });
    store.commit_sub_chunk(key, section).unwrap();
    let cells = match name {
        "one_cell" => Vec::new(),
        "last_use" => vec![0],
        "same_width" | "width_growth" => (1..BLOCKS_PER_SUB_CHUNK).step_by(2).collect(),
        _ => unreachable!("known single-cell fixture"),
    };
    let updates = cells
        .into_iter()
        .map(|i| {
            BlockUpdate::new(
                (i >> 8) as u8,
                (i & 15) as u8,
                ((i >> 4) & 15) as u8,
                0,
                SOLID_SWAPPED,
            )
        })
        .collect::<Vec<_>>();
    store.update_sub_chunk_blocks(key, &updates, AIR).unwrap();
    let value = match name {
        "last_use" => SOLID,
        "width_growth" => AIR,
        _ => SOLID_SWAPPED,
    };
    (
        store.sub_chunk(key).unwrap(),
        BlockUpdate::new(0, 0, 0, 0, value),
    )
}

/// Times production single-edit preparation, including snapshot copies, with allocation witnesses.
fn single_cell_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline/synced_update_burst");
    group.sample_size(10);
    group.throughput(Throughput::Elements(1));
    for name in ["one_cell", "same_width", "width_growth", "last_use"] {
        let (source, update) = single_cell_fixture(name);
        let prepare = || {
            ChunkStore::prepare_sub_chunk_blocks(
                SubChunkKey::new(0, 0, 0, 0),
                Some(&source),
                &[update],
                AIR,
            )
            .unwrap()
        };
        #[cfg(not(feature = "runtime-profile"))]
        let (witness, allocations) = test_alloc::measure(prepare);
        #[cfg(feature = "runtime-profile")]
        let witness = prepare();
        assert!(witness.changed());
        let output = witness.replacement().unwrap();
        let cells_changed = (0..BLOCKS_PER_SUB_CHUNK)
            .filter(|&i| {
                let (x, y, z) = ((i >> 8) as u8, (i & 15) as u8, ((i >> 4) & 15) as u8);
                source.runtime_id(0, x, y, z) != output.runtime_id(0, x, y, z)
            })
            .count();
        assert_eq!(cells_changed, 1);
        assert_eq!(output.runtime_id(0, 0, 0, 0), Some(update.runtime_id));
        let before = &source.storages()[0];
        let after = &output.storages()[0];
        let expected_bits = match name {
            "one_cell" | "same_width" => 1,
            "width_growth" => 2,
            "last_use" => 0,
            _ => unreachable!(),
        };
        assert_eq!(after.bits_per_index(), expected_bits);
        let word_differences = before
            .words()
            .iter()
            .zip(after.words())
            .filter(|(a, b)| a != b)
            .count()
            + before.words().len().abs_diff(after.words().len());
        if name == "same_width" {
            assert_eq!(word_differences, 1);
        }
        // Preparation owns a deep snapshot copy even when mutation patches that copy in place.
        if !before.words().is_empty() && !after.words().is_empty() {
            assert_ne!(before.words().as_ptr(), after.words().as_ptr());
        }
        eprintln!(
            "single_cell case={name} cells_changed={cells_changed} source_words={} output_words={} word_differences={word_differences}",
            before.words().len(),
            after.words().len()
        );
        #[cfg(not(feature = "runtime-profile"))]
        eprintln!(
            "single_cell case={name} allocation_requests_including_realloc={} requested_bytes={}",
            allocations.calls, allocations.bytes
        );
        group.bench_function(BenchmarkId::new("single_prepare", name), |b| {
            b.iter(|| black_box(prepare()));
        });
    }
    group.finish();
}

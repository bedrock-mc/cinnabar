use super::*;
use benchmark_support::MutationBurstFixture;
use protocol::{ActorBlockSyncMessage, BlockUpdateEvent, SyncedBlockUpdateEvent};
use world::{RawBlockIds, SubChunk};

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
}

//! Fixture-only access for headless benchmarks; absent from normal client builds.

use client_world::ingestion::vanilla_dimension_range;
use {super::*, render_api::MAX_VIEW_RADIUS_CHUNKS};

/// Recreates the resident-key population of the former radius-16 cohort timing test.
/// No terrain is decoded here: this measures the diagnostic's key scans, not streaming.
pub fn cohort_fixture(mut stream: WorldStream, radius: i32) -> WorldStream {
    assert!((1..=MAX_VIEW_RADIUS_CHUNKS).contains(&radius));
    let range = vanilla_dimension_range(stream.current_dimension()).unwrap();
    for x in -radius..=radius {
        for z in -radius..=radius {
            let column = ChunkKey::new(stream.current_dimension(), x, z);
            stream.loaded_columns.insert(column);
            for offset in 0..range.sub_chunk_count {
                stream.resident.insert(SubChunkKey::from_chunk(
                    column,
                    range.base_sub_chunk_y + offset as i32,
                ));
            }
        }
    }
    stream
}

/// Bytes of block storage the stream keeps for its loaded columns: palettes and packed words.
pub fn terrain_block_bytes(stream: &WorldStream) -> usize {
    stream
        .loaded_columns
        .iter()
        .filter_map(|column| stream.authority.terrain().chunk(*column))
        .flat_map(|chunk| chunk.sub_chunks().map(|(_, section)| section))
        .map(|section| {
            std::mem::size_of::<SubChunk>()
                + section
                    .storages()
                    .iter()
                    .map(|storage| {
                        std::mem::size_of_val(storage)
                            + 4 * (storage.packed_words().len() + storage.palette().values().len())
                    })
                    .sum::<usize>()
        })
        .sum()
}

/// Checks completion without performing the diagnostic population scans being benchmarked.
pub fn work_is_idle(stream: &WorldStream) -> bool {
    stream.pending_decode.is_empty()
        && stream.in_flight_decode_jobs == 0
        && stream.lighting.jobs.pending.is_empty()
        && stream.lighting.jobs.in_flight.is_empty()
        && stream.mesh_jobs.pending.is_empty()
        && stream.mesh_jobs.in_flight.is_empty()
        && stream.mesh_changes.is_empty()
        && stream.staged_mesh_completions.is_empty()
        && stream.requests.requested.is_empty()
        && stream.far_summaries_idle()
}

mod dispatch;
pub use dispatch::DispatchFixture;
pub mod biomes;

mod mutations;
pub use mutations::MutationBurstFixture;

/// A resident grid for measuring retention discovery and retirement without worker timing.
pub struct RetentionFixture {
    stream: WorldStream,
}

impl RetentionFixture {
    /// Populates one air section in every retained column around the origin.
    pub fn new(radius: i32) -> Self {
        let mut stream = WorldStream::new(WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        stream.chunk_radius = Some(radius);
        stream.reevaluate_chunk_retention();
        let reach = radius + world::CHUNK_VIEW_SLACK;
        for x in -reach..=reach {
            for z in -reach..=reach {
                if chunk_in_view(radius, [x, z], [0, 0]) {
                    let column = ChunkKey::new(0, x, z);
                    stream.loaded_columns.insert(column);
                    stream.record_known_air(SubChunkKey::from_chunk(column, 0));
                }
            }
        }
        Self { stream }
    }

    /// Uses non-air resident markers to exercise neighbour invalidation during retirement.
    pub fn with_connectivity(radius: i32) -> Self {
        let mut fixture = Self::new(radius);
        for column in fixture.stream.loaded_columns.iter().copied() {
            let key = SubChunkKey::from_chunk(column, 0);
            fixture.stream.known_air.remove(&key);
            fixture.stream.resident.insert(key);
            fixture
                .stream
                .connectivity
                .insert(key, FaceConnectivity::all());
        }
        fixture
    }

    /// Retires the trailing boundary after a one-column crossing.
    pub fn cross(&mut self) {
        self.stream.local_player_chunk = Some(ChunkKey::new(0, 1, 0));
        assert!(self.stream.reevaluate_chunk_retention());
        std::hint::black_box(self.stream.loaded_column_count());
    }
}

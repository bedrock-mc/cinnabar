//! Fixture-only access for headless benchmarks; absent from normal client builds.

use super::*;
use client_world::ingestion::vanilla_dimension_range;

/// Recreates the resident-key population of the former radius-16 cohort timing test.
/// No terrain is decoded here: this measures the diagnostic's key scans, not streaming.
pub fn cohort_fixture(mut stream: WorldStream, radius: i32) -> WorldStream {
    assert!((1..=PHASE0_MAX_VIEW_RADIUS_CHUNKS).contains(&radius));
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
}

mod dispatch;
pub use dispatch::DispatchFixture;

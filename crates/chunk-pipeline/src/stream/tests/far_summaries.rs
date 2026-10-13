//! Complete columns and retiring columns reach far terrain as surface summaries.

use super::*;

fn stream(enabled: bool) -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.set_far_terrain_enabled(enabled);
    stream
}

fn drain_until(stream: &mut WorldStream, count: usize) -> Vec<FarColumnSummary> {
    let mut summaries = Vec::new();
    test_time::eventually("far-terrain summaries", || {
        stream.dispatch_far_summaries();
        stream.drain_far_summaries(&mut summaries);
        summaries.len() >= count
    });
    summaries
}

/// An inline column, its resend and its eviction are each summarised from complete data.
#[test]
fn complete_resent_and_retiring_columns_are_summarised() {
    let mut stream = stream(true);
    stream
        .submit(1, inline_block_entity_event(1, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let first = drain_until(&mut stream, 1);
    assert_eq!(first[0].column, ChunkKey::new(0, 1, 0));
    let decoded = far_terrain::ColumnSummary::decode(&first[0].summary.bytes).unwrap();
    assert_eq!(decoded.cell(3, 9).height, -4 * 16 + 16);

    stream.submit(2, inline_air_event(1)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    let resent = drain_until(&mut stream, 1);
    let resent = far_terrain::ColumnSummary::decode(&resent[0].summary.bytes).unwrap();
    assert!(resent.is_empty(), "the resend replaced the surface");

    stream.evict_column(ChunkKey::new(0, 1, 0));
    let retired = drain_until(&mut stream, 1);
    assert_eq!(retired[0].column, ChunkKey::new(0, 1, 0));
}

/// A request-mode resend with no sub-chunks empties the column, and its summary says so.
#[test]
fn empty_request_mode_resend_replaces_the_summary() {
    let mut stream = stream(true);
    stream
        .submit(1, inline_block_entity_event(1, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let first = drain_until(&mut stream, 1);
    let first = far_terrain::ColumnSummary::decode(&first[0].summary.bytes).unwrap();
    assert!(!first.is_empty());

    stream
        .submit(
            2,
            request_level_chunk_event(0, 1, 0, LevelChunkMode::LimitedRequests { highest: 0 }, 1),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let resent = drain_until(&mut stream, 1);
    assert_eq!(resent[0].column, ChunkKey::new(0, 1, 0));
    let resent = far_terrain::ColumnSummary::decode(&resent[0].summary.bytes).unwrap();
    assert!(resent.is_empty(), "the empty resend replaced the surface");
}

/// Retiring the stream summarises every complete column into the handed-over inbox.
#[test]
fn retiring_the_stream_summarises_resident_columns() {
    let mut stream = stream(true);
    stream
        .submit(1, inline_block_entity_event(1, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let _ = drain_until(&mut stream, 1);
    let mut inbox = stream.retire_far_terrain();
    drop(stream);
    let mut summaries = Vec::new();
    test_time::eventually("retired summaries", || {
        inbox.drain(&mut summaries);
        inbox.is_idle()
    });
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].column, ChunkKey::new(0, 1, 0));
}

/// With far terrain off, no column work is queued or produced.
#[test]
fn disabled_far_terrain_summarises_nothing() {
    let mut stream = stream(false);
    stream
        .submit(1, inline_block_entity_event(1, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.evict_column(ChunkKey::new(0, 1, 0));
    stream.dispatch_far_summaries();
    let mut summaries = Vec::new();
    stream.drain_far_summaries(&mut summaries);
    assert!(summaries.is_empty());
    assert!(stream.far_terrain.pending.is_empty());
}

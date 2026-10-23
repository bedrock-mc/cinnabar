//! Complete columns and retiring columns reach far terrain as surface summaries.

use super::*;

/// A dimension round trip must not deliver the old world's queued or retired summaries.
#[test]
fn dimension_round_trip_cancels_old_far_summaries() {
    let mut stream = stream(true);
    stream
        .submit(1, inline_block_entity_event(1, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.dispatch_far_summaries();
    for (sequence, dimension) in [(2, 1), (3, 0)] {
        stream
            .submit(
                sequence,
                WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                    dimension,
                    ..Default::default()
                }),
            )
            .unwrap();
    }
    let mut summaries = Vec::new();
    test_time::eventually("previous dimension summary work finishes", || {
        stream.dispatch_far_summaries();
        stream.drain_far_summaries(&mut summaries);
        stream.far_summaries_idle()
    });
    assert!(
        summaries.is_empty(),
        "old-world summaries reached the destination: {summaries:?}"
    );
    stream
        .submit(4, inline_block_entity_event(2, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let current = drain_until(&mut stream, 1);
    assert_eq!(current[0].column, ChunkKey::new(0, 2, 0));
}

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

#[test]
fn resource_reload_resummarizes_resident_columns_without_a_geometry_change() {
    let mut stream = stream(true);
    stream
        .submit(1, inline_block_entity_event(1, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let first = drain_until(&mut stream, 1).pop().unwrap();
    let session = stream.authority().actor_session_id();
    let replacement = Arc::new(RuntimeAssets::diagnostic());
    assert!(replacement.has_same_geometry(stream.runtime_assets()));
    stream.reload_resource_assets(Arc::clone(&replacement));
    let fresh = drain_until(&mut stream, 1).pop().unwrap();
    assert_eq!(fresh.column, first.column);
    assert!(fresh.generation > first.generation);
    assert_eq!(stream.authority().actor_session_id(), session);
    stream.reload_resource_assets(replacement);
    assert!(
        stream.far_summaries_idle(),
        "the same carrier does not rebuild summaries"
    );
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

/// Delivered air owns its visible emptiness before a summary worker or mesh upload runs.
#[test]
fn missing_terrain_empty_delivered_column_hides_cached_terrain_immediately() {
    for mode in [
        LevelChunkMode::Inline { count: 0 },
        LevelChunkMode::LimitedRequests { highest: 0 },
    ] {
        let mut stream = stream(true);
        stream.set_full_detail_radius(Some(1));
        let column = ChunkKey::new(0, 10, 0);
        stream
            .submit(1, inline_block_entity_event(column.x, 7, Vec::new()))
            .unwrap();
        complete_pending_decode_jobs(&mut stream);
        assert!(!stream.near_drawn_columns().1.any(|key| key == column));
        stream
            .submit(2, request_level_chunk_event(0, column.x, column.z, mode, 1))
            .unwrap();
        complete_pending_decode_jobs(&mut stream);

        let mask = far_terrain::NearMask::around_with_empty(
            [0, 0],
            16,
            1,
            stream
                .near_geometry_columns()
                .map(|column| far_terrain::ColumnPos::new(column.x, column.z)),
            stream
                .near_empty_columns()
                .map(|column| far_terrain::ColumnPos::new(column.x, column.z)),
        );
        assert!(mask.contains(far_terrain::ColumnPos::new(column.x, column.z)));
        assert_eq!(
            mask.outer_blocks(),
            far_terrain::NearMask::around([0, 0], 16, 1, []).outer_blocks()
        );

        stream
            .submit(3, inline_block_entity_event(column.x, 7, Vec::new()))
            .unwrap();
        complete_pending_decode_jobs(&mut stream);
        assert!(!stream.near_drawn_columns().1.any(|key| key == column));

        stream.submit(4, inline_air_event(column.x)).unwrap();
        complete_pending_decode_jobs(&mut stream);
        assert!(stream.near_drawn_columns().1.any(|key| key == column));
        stream.evict_column(column);
        assert!(!stream.near_drawn_columns().1.any(|key| key == column));
    }
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

/// Turning far terrain on summarises columns that loaded while it was off.
#[test]
fn enabling_far_terrain_summarises_resident_columns() {
    let mut stream = stream(false);
    stream
        .submit(1, inline_block_entity_event(1, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.set_far_terrain_enabled(true);
    let summaries = drain_until(&mut stream, 1);
    assert_eq!(summaries[0].column, ChunkKey::new(0, 1, 0));
}

/// A deadline-bounded drain blocks for issued work instead of returning empty.
#[test]
fn draining_with_a_deadline_waits_for_issued_summaries() {
    let mut stream = stream(true);
    stream
        .submit(1, inline_block_entity_event(1, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let _ = drain_until(&mut stream, 1);
    let mut inbox = stream.retire_far_terrain();
    let mut summaries = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(60);
    for _ in 0..64 {
        if inbox.is_idle() {
            break;
        }
        inbox.drain_waiting(&mut summaries, deadline);
    }
    assert!(inbox.is_idle());
    assert_eq!(summaries.len(), 1);
}

/// The final owner-thread edit survives eviction and stream drop before summary admission.
#[test]
fn final_edit_before_eviction_survives_stream_retirement() {
    let mut stream = stream(true);
    stream
        .submit(1, inline_block_entity_event(1, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream
        .submit(
            2,
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position: [19, -40, 9],
                layer: 0,
                network_id: 1,
            }]),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.evict_column(ChunkKey::new(0, 1, 0));
    let mut inbox = stream.retire_far_terrain();
    drop(stream);
    let mut out = Vec::new();
    test_time::eventually("final edited retirement summary", || {
        inbox.drain_budgeted(&mut out, &mut far_terrain::SummaryBudget::default());
        inbox.is_idle()
    });
    assert_eq!(out.len(), 1);
    let summary = far_terrain::ColumnSummary::decode(&out[0].summary.bytes).unwrap();
    assert_eq!(summary.cell(3, 9).height, -39);
}

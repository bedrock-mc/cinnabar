//! A resent inline column relights only the sections whose light inputs changed.

use super::{light_scheduler::install_current_light, *};

/// Commits chunk 1's single section as `runtime_id` with solved light and no queued meshes.
fn settled_column(runtime_id: u32) -> (WorldStream, SubChunkKey) {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 1, -4, 0);
    stream
        .submit(1, inline_block_entity_event(1, runtime_id, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    install_current_light(&mut stream, key, 0, 15, false);
    stream.mesh_jobs.pending.clear();
    stream.mesh_jobs.clear_queued();
    (stream, key)
}

/// Swapping blocks for others with the same light inputs remeshes without relighting.
#[test]
fn light_neutral_resend_remeshes_without_relighting() {
    let (mut stream, key) = settled_column(7);
    let before = stream.authority.terrain().sub_chunk(key).unwrap();

    stream
        .submit(2, inline_block_entity_event(1, 8, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    let after = stream.authority.terrain().sub_chunk(key).unwrap();
    assert!(
        !Arc::ptr_eq(&before, &after),
        "the resend replaced the section"
    );
    assert!(stream.light_is_current(key));
    assert!(!stream.lighting.jobs.pending.contains_key(&key));
    assert!(stream.mesh_jobs.pending.contains_key(&key));
}

/// A resend that changes light inputs, here to air, relights the section.
#[test]
fn light_changing_resend_relights() {
    let (mut stream, key) = settled_column(7);

    stream.submit(2, inline_air_event(1)).unwrap();
    complete_pending_decode_jobs(&mut stream);

    assert!(!stream.light_is_current(key));
}

/// A summary compared against a section that is no longer resident cannot skip the relight.
#[test]
fn light_neutral_summary_of_a_replaced_section_relights() {
    let (mut stream, key) = settled_column(7);
    let mut payload = vec![9, 1, (-4_i8) as u8, 1];
    payload.extend(zig_zag_i32(8));

    stream.apply_prepared(PreparedWorldEvent::InlineLevelChunk {
        event: LevelChunkEvent {
            dimension: 0,
            x: 1,
            z: 0,
            mode: LevelChunkMode::Inline { count: 1 },
            payload: payload.clone(),
        },
        decoded: DecodedLevelChunk::decode(-4, 1, &payload, &RAW_IDS),
        // Equal blocks, but not the resident allocation the commit replaces.
        light_neutral: vec![(-4, Arc::new(uniform_sub_chunk(7)))],
        duration: std::time::Duration::ZERO,
    });

    assert!(!stream.light_is_current(key));
}

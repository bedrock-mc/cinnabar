//! Columns past the full-detail radius keep their blocks but skip lighting and meshing.

use super::*;

fn stream() -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [8.0, 0.0, 8.0],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.set_far_terrain_enabled(true);
    stream
}

fn meshing(stream: &WorldStream, x: i32) -> bool {
    stream
        .mesh_jobs
        .pending
        .keys()
        .any(|key| key.chunk() == ChunkKey::new(0, x, 0))
}

/// A far column is stored and summarised but not meshed until the radius reaches it, and a
/// shrinking radius withdraws it again.
#[test]
fn far_columns_mesh_only_inside_the_full_detail_radius() {
    let mut stream = stream();
    stream.set_full_detail_radius(Some(1));
    for (sequence, x) in [(1, 0), (2, 5)] {
        stream
            .submit(sequence, inline_block_entity_event(x, 7, Vec::new()))
            .unwrap();
    }
    complete_pending_decode_jobs(&mut stream);
    assert!(meshing(&stream, 0), "the near column meshes");
    assert!(!meshing(&stream, 5), "the far column waits");
    assert!(
        stream
            .authority
            .terrain()
            .chunk(ChunkKey::new(0, 5, 0))
            .is_some(),
        "its blocks are kept"
    );

    stream.set_full_detail_radius(Some(6));
    assert!(meshing(&stream, 5), "a wider radius promotes it");

    stream.mesh_jobs.pending.clear();
    stream.set_full_detail_radius(Some(1));
    assert!(meshing(&stream, 5), "a narrower one queues its removal");
    assert!(!stream.wants_mesh(ChunkKey::new(0, 5, 0)));
}

/// Without a radius every column meshes, as before far terrain.
#[test]
fn no_radius_meshes_everything() {
    let mut stream = stream();
    stream
        .submit(1, inline_block_entity_event(5, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(meshing(&stream, 5));
    assert_eq!(stream.full_detail_radius(), None);
}

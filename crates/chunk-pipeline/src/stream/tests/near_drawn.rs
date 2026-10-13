//! A column is drawn by near terrain only once its meshes are on screen.

use super::*;

fn drawn(stream: &WorldStream, column: ChunkKey) -> bool {
    stream.near_drawn_columns().1.any(|drawn| drawn == column)
}

/// Completing a column is not enough: every section's mesh must upload, and eviction hands
/// the column back to far terrain.
#[test]
fn columns_are_drawn_once_their_meshes_upload() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(1, inline_block_entity_event(1, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let column = ChunkKey::new(0, 1, 0);
    assert!(stream.loaded_columns.contains(&column));
    assert!(!drawn(&stream, column), "complete but not yet meshed");

    let keys: Vec<SubChunkKey> = stream
        .resident
        .column(column)
        .chain(stream.known_air.column(column))
        .copied()
        .collect();
    assert!(!keys.is_empty());
    let now = Instant::now();
    for key in keys {
        if let Some((revision, since)) = stream
            .revisions
            .dirty(key)
            .map(|dirty| (dirty.revision, dirty.since))
        {
            stream.acknowledge_mesh_upload(key, revision, since, now);
        }
    }
    assert!(drawn(&stream, column));

    stream.evict_column(column);
    assert!(!drawn(&stream, column));
}

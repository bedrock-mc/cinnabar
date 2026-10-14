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
    stream.set_far_terrain_enabled(true);
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

/// Near-drawn columns are loaded and every section they hold has a published mesh.
fn assert_near_drawn_invariant(stream: &WorldStream) {
    for column in stream.near_drawn_columns().1 {
        assert!(
            stream.loaded_columns.contains(&column),
            "{column:?} is not loaded"
        );
        let keys: Vec<SubChunkKey> = stream
            .resident
            .column(column)
            .chain(stream.known_air.column(column))
            .copied()
            .collect();
        assert!(!keys.is_empty(), "{column:?} holds no sections");
        for key in keys {
            assert!(
                stream.applied_mesh_generations.contains_key(&key),
                "{key:?} has no published mesh"
            );
        }
    }
}

/// Loads, resends, partial uploads and evictions in a scrambled order never leave a column
/// near-drawn before its meshes are published.
#[test]
fn near_drawn_columns_always_have_published_meshes() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.set_far_terrain_enabled(true);
    let mut seed = 0x2545_f491_u64;
    let mut next = move |bound: u64| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (seed >> 33) % bound
    };
    let mut sequence = 0;
    let mut ever_drawn = false;
    for _ in 0..200 {
        let x = next(4) as i32;
        match next(4) {
            0 => {
                sequence += 1;
                stream
                    .submit(sequence, inline_block_entity_event(x, 7, Vec::new()))
                    .unwrap();
                complete_pending_decode_jobs(&mut stream);
            }
            1 | 2 => {
                let column = ChunkKey::new(0, x, 0);
                let dirty: Vec<(SubChunkKey, u64, Instant)> = stream
                    .resident
                    .column(column)
                    .chain(stream.known_air.column(column))
                    .filter_map(|key| {
                        stream
                            .revisions
                            .dirty(*key)
                            .map(|dirty| (*key, dirty.revision, dirty.since))
                    })
                    .collect();
                if let Some(&(key, revision, since)) =
                    dirty.get(next(dirty.len().max(1) as u64) as usize)
                {
                    stream.acknowledge_mesh_upload(key, revision, since, Instant::now());
                }
            }
            _ => stream.evict_column(ChunkKey::new(0, x, 0)),
        }
        assert_near_drawn_invariant(&stream);
        ever_drawn |= stream.near_drawn_columns().1.next().is_some();
    }
    assert!(ever_drawn, "uploads hand columns to near terrain");
}

/// With far terrain off nothing is tracked, and turning it on picks up what is on screen.
#[test]
fn near_drawn_columns_are_tracked_only_with_far_terrain_on() {
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
    let keys: Vec<SubChunkKey> = stream.resident.column(column).copied().collect();
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
    assert!(!drawn(&stream, column));
    stream.set_far_terrain_enabled(true);
    assert!(drawn(&stream, column));
    stream.set_far_terrain_enabled(false);
    assert!(!drawn(&stream, column));
}

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
    let required: std::collections::HashMap<_, _> = keys
        .iter()
        .map(|&key| (key, stream.revisions.dirty(key).unwrap().revision))
        .collect();
    assert_eq!(
        stream.terrain_publication_target(32).near.get(&column),
        Some(&required),
        "the publication boundary carries each pending section generation"
    );
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
    assert_eq!(
        stream.terrain_publication_target(32).near.get(&column),
        Some(&required),
        "published sections keep their acknowledged generations"
    );
    let key = *required.keys().next().unwrap();
    let revision = stream.mark_dirty_exact(key, now);
    assert_eq!(
        stream.terrain_publication_target(32).near[&column][&key],
        revision,
        "a pending replacement supersedes the published generation"
    );

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
            action @ (1 | 2) => {
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
                // A column holds dozens of sections, so single uploads alone never finish one
                // between resends and evictions; whole-column uploads let some complete.
                let uploads = if action == 1 {
                    let pick = next(dirty.len().max(1) as u64) as usize;
                    dirty.get(pick..=pick).unwrap_or_default()
                } else {
                    &dirty[..]
                };
                for &(key, revision, since) in uploads {
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

/// Exact section requirements change independently of the column's full-detail age.
#[test]
fn section_membership_stamps_track_insert_remove_replace_and_retire() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    let a = SubChunkKey::new(0, 0, 0, 0);
    let b = SubChunkKey { y: 1, ..a };
    let c = SubChunkKey { y: 2, ..a };
    stream.loaded_columns.insert(a.chunk());
    stream.record_known_air(a);
    let age = stream.resident.column_generation();
    let mut stamp = (
        stream.terrain_publication_source(),
        stream.terrain_publication_revision(),
    );
    let mut changed = |stream: &WorldStream, expected: &[SubChunkKey]| {
        let next = (
            stream.terrain_publication_source(),
            stream.terrain_publication_revision(),
        );
        assert_ne!(next.0, stamp.0);
        assert_ne!(next.1, stamp.1);
        stamp = next;
        let target = stream.terrain_publication_target(32);
        assert_eq!(target.retained, expected.iter().copied().collect());
        assert_eq!(
            target
                .near
                .get(&a.chunk())
                .map(|keys| keys
                    .keys()
                    .copied()
                    .collect::<std::collections::HashSet<_>>())
                .unwrap_or_default(),
            target.retained
        );
    };
    stream.record_known_air(b);
    changed(&stream, &[a, b]);
    stream.resident.remove(&b);
    stream.known_air.remove(&b);
    changed(&stream, &[a]);
    stream.resident.replace_column(a.chunk(), [c]);
    stream.known_air.replace_column(a.chunk(), [c]);
    changed(&stream, &[c]);
    assert!(stream.resident.existed_at(a.chunk(), age));
    assert_eq!(stream.resident.column_generation(), age);
    let before = stream.terrain_publication_source();
    stream.resident.replace_column(a.chunk(), [c, c]);
    stream.known_air.replace_column(a.chunk(), [c]);
    stream.record_known_air(c);
    assert_eq!(stream.terrain_publication_source(), before);
    stream.resident.retire();
    stream.known_air.retire();
    changed(&stream, &[]);
    let empty = stream.terrain_publication_source();
    stream.resident.retire();
    stream.known_air.retire();
    assert_eq!(stream.terrain_publication_source(), empty);
}

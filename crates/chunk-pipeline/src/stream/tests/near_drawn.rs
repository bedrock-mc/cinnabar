//! Delivered air and published near meshes own coverage over cached far terrain.

use super::*;

/// Whether either kind of near coverage hides this column.
fn drawn(stream: &WorldStream, column: ChunkKey) -> bool {
    stream.near_drawn_columns().1.any(|drawn| drawn == column)
}

/// Publishes every dirty section of a column without running mesh workers.
fn upload_column(stream: &mut WorldStream, column: ChunkKey) {
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
            stream.mesh_jobs.pending.remove(&key);
            stream.acknowledge_mesh_upload(key, revision, since, now);
        }
    }
}

/// Geometry waits for uploads, air resends change coverage immediately, and eviction clears both.
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
    upload_column(&mut stream, column);
    assert!(drawn(&stream, column));
    assert_eq!(
        stream.terrain_publication_target(32).near.get(&column),
        Some(&required),
        "published sections keep their acknowledged generations"
    );
    let key = *required.keys().next().unwrap();
    let revision = stream.mark_dirty_exact(key, Instant::now());
    assert_eq!(
        stream.terrain_publication_target(32).near[&column][&key],
        revision
    );
    upload_column(&mut stream, column);
    assert!(drawn(&stream, column));
    assert!(stream.near_geometry_columns().any(|key| key == column));
    assert!(!stream.near_empty_columns().any(|key| key == column));
    let geometry_revision = stream.near_drawn_columns().0;

    stream.submit(2, inline_air_event(column.x)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.near_empty_columns().any(|key| key == column));
    assert!(!stream.near_geometry_columns().any(|key| key == column));
    let air_revision = stream.near_drawn_columns().0;
    assert!(air_revision > geometry_revision);

    stream.submit(3, inline_air_event(column.x)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.near_drawn_columns().0, air_revision);
    upload_column(&mut stream, column);
    assert!(stream.near_empty_columns().any(|key| key == column));
    stream
        .submit(4, inline_block_entity_event(column.x, 7, Vec::new()))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.near_empty_columns().any(|key| key == column));
    assert!(!stream.near_geometry_columns().any(|key| key == column));
    upload_column(&mut stream, column);
    assert!(stream.near_geometry_columns().any(|key| key == column));
    assert!(!stream.near_empty_columns().any(|key| key == column));
    assert!(stream.near_drawn_columns().0 > air_revision);

    stream.evict_column(column);
    assert!(!drawn(&stream, column));
    assert!(stream.near_geometry_columns().next().is_none());
    assert!(stream.near_empty_columns().next().is_none());
}

/// Covered columns are loaded and either empty or backed by published meshes.
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
        if stream
            .authority
            .terrain()
            .chunk(column)
            .is_none_or(|chunk| {
                chunk
                    .sub_chunks()
                    .all(|(_, section)| stream.classifier.is_sub_chunk_air(&section))
            })
        {
            continue;
        }
        assert!(!keys.is_empty(), "{column:?} holds no tracked sections");
        for key in keys {
            assert!(
                stream.applied_mesh_generations.contains_key(&key),
                "{key:?} has no published mesh"
            );
        }
    }
}

/// Loads, resends, partial uploads and evictions retain valid coverage in any order.
#[test]
fn covered_columns_are_empty_or_have_published_meshes() {
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
        assert_eq!(
            *target.retained,
            expected
                .iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
        );
        assert_eq!(
            target
                .near
                .get(&a.chunk())
                .map(|keys| keys
                    .keys()
                    .copied()
                    .collect::<std::collections::HashSet<_>>())
                .unwrap_or_default(),
            *target.retained
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

/// First delivery publishes air evidence at either radius; missing data never proves emptiness.
#[test]
fn first_delivery_publishes_air_evidence_independently_of_mesh_radius() {
    #[derive(Clone, Copy, Debug)]
    enum Delivery {
        Air,
        NonAir,
        Undelivered,
    }
    for x in [0, 10] {
        for delivery in [Delivery::Air, Delivery::NonAir, Delivery::Undelivered] {
            let mut stream = WorldStream::new(WorldBootstrap {
                local_player_unique_id: 1,
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
                block_network_ids_are_hashes: false,
            });
            stream.set_far_terrain_enabled(true);
            stream.set_full_detail_radius(Some(1));
            let column = ChunkKey::new(0, x, 0);
            let event = match delivery {
                Delivery::Air => Some(inline_air_event(x)),
                Delivery::NonAir => Some(inline_block_entity_event(x, 7, Vec::new())),
                Delivery::Undelivered => None,
            };
            if let Some(event) = event {
                stream.submit(1, event).unwrap();
                complete_pending_decode_jobs(&mut stream);
            }
            assert_eq!(stream.wants_mesh(column), x == 0);
            let air = matches!(delivery, Delivery::Air);
            let mask = far_terrain::NearMask::from_columns(
                [0, 0],
                16,
                stream
                    .near_empty_columns()
                    .map(|column| far_terrain::ColumnPos::new(column.x, column.z)),
            );
            assert_eq!(
                mask.contains(far_terrain::ColumnPos::new(x, 0)),
                air,
                "delivered-air mask at {x}, {delivery:?}"
            );
            let sections: BTreeSet<_> = stream.terrain_publication_sections().collect();
            let expected: BTreeSet<_> = sections
                .iter()
                .copied()
                .filter(|key| {
                    stream.terrain_removal_kind(*key) == render_api::TerrainRemovalKind::Empty
                })
                .collect();
            if air {
                assert!(!expected.is_empty());
            }
            assert_eq!(
                stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, sections.len()),
                0
            );
            let mut published = BTreeSet::new();
            for change in stream.take_mesh_changes() {
                let WorldMeshChange::Remove {
                    key, generation, ..
                } = change
                else {
                    panic!("zero geometry credit must only publish removals");
                };
                assert_ne!(generation, 0);
                assert_eq!(
                    stream.terrain_removal_kind(key),
                    render_api::TerrainRemovalKind::Empty
                );
                assert_eq!(
                    stream
                        .terrain_publication_target(16)
                        .near
                        .get(&column)
                        .and_then(|keys| keys.get(&key))
                        .copied(),
                    (air || x == 0).then_some(generation)
                );
                published.insert(key);
            }
            assert_eq!(published, expected, "empty evidence at {x}, {delivery:?}");
            assert_eq!(
                stream
                    .terrain_publication_target(16)
                    .near
                    .contains_key(&column),
                air || (x == 0 && matches!(delivery, Delivery::NonAir))
            );
        }
    }
}

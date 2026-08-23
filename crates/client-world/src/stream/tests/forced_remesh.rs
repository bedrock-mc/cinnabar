use super::*;

#[test]
fn forced_remesh_returns_exact_resident_generation_manifest() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let keys = [
        SubChunkKey::new(0, -1, -4, 2),
        SubChunkKey::new(0, 0, -4, 0),
        SubChunkKey::new(0, 1, -4, -2),
    ];
    for key in keys {
        stream
            .store
            .update_block(key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
            .unwrap();
        stream.resident.insert(key);
    }
    let known_air = SubChunkKey::new(0, 2, -4, 3);
    stream.record_known_air(known_air);
    stream.mark_light_changed_sources(keys.into_iter().chain([known_air]));
    light_scheduler::settle_light(&mut stream, [0.0; 3]);
    let previously_dirty_at = std::time::Instant::now();
    stream.mark_dirty_exact(keys[0], previously_dirty_at);
    let started = previously_dirty_at + Duration::from_millis(1);

    let manifest = stream.remesh_all_resident(started);

    assert_eq!(manifest.started_at, started);
    assert_eq!(manifest.entries.len(), 4);
    assert_eq!(
        manifest
            .entries
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>(),
        keys.into_iter().chain([known_air]).collect()
    );
    assert_eq!(
        manifest
            .entries
            .iter()
            .map(|(_, generation)| *generation)
            .collect::<BTreeSet<_>>()
            .len(),
        manifest.entries.len(),
        "every forced remesh key must receive one unique generation"
    );
    for (key, generation) in manifest.entries.iter().copied() {
        let dirty = stream.revisions.dirty(key).unwrap();
        assert_eq!(dirty.since, started);
        assert_eq!(dirty.revision, generation);
    }

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 3), 3);
    assert!(stream.take_mesh_changes().iter().any(|change| {
        matches!(
            change,
            super::WorldMeshChange::Remove { key, generation, dirty_since, .. }
                if *key == known_air
                    && manifest.entries.contains(&(*key, *generation))
                    && *dirty_since == started
        )
    }));
}

#[test]
fn forced_remesh_of_frozen_published_manifest_skips_unpublished_and_air_keys() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let published = [SubChunkKey::new(0, 0, -4, 0), SubChunkKey::new(0, 1, -4, 0)];
    let unpublished = SubChunkKey::new(0, 2, -4, 0);
    let known_air = SubChunkKey::new(0, 3, -4, 0);
    for key in published.into_iter().chain([unpublished]) {
        stream
            .store
            .update_block(key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
            .unwrap();
        stream.resident.insert(key);
    }
    stream.record_known_air(known_air);
    let frozen = Arc::<[(SubChunkKey, u64)]>::from([(published[0], 40), (published[1], 41)]);
    stream.applied_mesh_generations.insert(published[0], 40);
    stream.applied_mesh_generations.insert(published[1], 41);
    let resident_before = stream.resident.clone();
    let known_air_before = stream.known_air.clone();
    let started = Instant::now();

    let manifest = stream
        .remesh_published_manifest(&frozen, started)
        .expect("the exact frozen published manifest should remesh");

    assert_eq!(
        manifest
            .entries
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>(),
        published.into_iter().collect()
    );
    assert!(manifest.entries.iter().all(|(key, generation)| {
        frozen
            .iter()
            .find(|(published_key, _)| published_key == key)
            .is_some_and(|(_, previous)| previous != generation)
    }));
    assert_eq!(
        stream.pending_mesh.keys().copied().collect::<BTreeSet<_>>(),
        published.into_iter().collect(),
        "unpublished resident and known-air identities must not create no-mesh jobs"
    );
    assert_eq!(stream.resident, resident_before);
    assert_eq!(stream.known_air, known_air_before);
    assert_eq!(
        stream.forced_remesh_manifest_state(&manifest),
        super::ForcedRemeshManifestState::Pending
    );
}

#[test]
fn published_manifest_remesh_rejects_stale_duplicate_or_nonresident_allocations() {
    let new_stream = || {
        let mut stream = WorldStream::new(WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 0, -4, 0);
        stream
            .store
            .update_block(key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
            .unwrap();
        stream.resident.insert(key);
        stream.applied_mesh_generations.insert(key, 7);
        (stream, key)
    };
    let now = Instant::now();

    let (mut stale, stale_key) = new_stream();
    assert!(
        stale
            .remesh_published_manifest(&[(stale_key, 6)], now)
            .is_none()
    );
    assert!(stale.pending_mesh.is_empty());

    let (mut duplicate, duplicate_key) = new_stream();
    assert!(
        duplicate
            .remesh_published_manifest(&[(duplicate_key, 7), (duplicate_key, 7)], now)
            .is_none()
    );
    assert!(duplicate.pending_mesh.is_empty());

    let (mut nonresident, nonresident_key) = new_stream();
    nonresident.resident.remove(&nonresident_key);
    assert!(
        nonresident
            .remesh_published_manifest(&[(nonresident_key, 7)], now)
            .is_none()
    );
    assert!(nonresident.pending_mesh.is_empty());

    let (mut known_air, known_air_key) = new_stream();
    known_air.record_known_air(known_air_key);
    assert!(
        known_air
            .remesh_published_manifest(&[(known_air_key, 7)], now)
            .is_none(),
        "a key that became known air must not create a forced removal job"
    );
    assert!(known_air.pending_mesh.is_empty());
}

#[test]
fn eviction_or_superseding_revision_cannot_complete_forced_manifest() {
    let new_stream = || {
        let mut stream = WorldStream::new(WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 0, -4, 0);
        stream.record_known_air(key);
        (stream, key)
    };

    let started = Instant::now();
    let (mut evicted, evicted_key) = new_stream();
    let evicted_manifest = evicted.remesh_all_resident(started);
    evicted.evict_column(evicted_key.chunk());
    assert_eq!(
        evicted.forced_remesh_manifest_state(&evicted_manifest),
        super::ForcedRemeshManifestState::Invalid
    );

    let (mut superseded, superseded_key) = new_stream();
    let superseded_manifest = superseded.remesh_all_resident(started);
    let superseded_at = started + Duration::from_millis(1);
    superseded.mark_dirty_exact(superseded_key, superseded_at);
    let replacement = superseded.revisions.dirty(superseded_key).unwrap();
    superseded.acknowledge_mesh_upload(
        superseded_key,
        replacement.revision,
        superseded_at,
        superseded_at + Duration::from_millis(1),
    );
    assert_eq!(
        superseded.forced_remesh_manifest_state(&superseded_manifest),
        super::ForcedRemeshManifestState::Invalid,
        "applying a replacement revision must not satisfy the forced generation"
    );
}

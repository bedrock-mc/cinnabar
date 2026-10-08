use super::*;

fn seasonal_stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: RAW_IDS.air,
        block_network_ids_are_hashes: false,
    })
}

#[test]
fn seasonal_roof_add_remove_invalidates_only_leaf_meshes_below_in_the_same_column() {
    let mut stream = seasonal_stream();
    let leaf = SubChunkKey::new(0, 0, 0, 0);
    let plain = SubChunkKey::new(0, 0, 1, 0);
    let neighbor = SubChunkKey::new(0, 1, 0, 0);
    let above = SubChunkKey::new(0, 0, 5, 0);
    for key in [leaf, plain, neighbor, above] {
        stream
            .authority
            .commit_sub_chunk(key, uniform_sub_chunk(1))
            .unwrap();
        stream.resident.insert(key);
        let generation = stream.mark_dirty_exact(key, Instant::now());
        assert!(stream.register_mesh_dependency_mask(
            key,
            generation,
            MeshDependencyMask::default().with_seasonal_foliage(key != plain)
        ));
    }
    stream.mesh_jobs.pending.clear();
    let plain_generation = stream.revisions.dirty(plain).unwrap().revision;
    let neighbour_generation = stream.revisions.dirty(neighbor).unwrap().revision;
    for _mutation in ["add roof", "remove roof"] {
        let previous = stream.revisions.dirty(leaf).unwrap().revision;
        stream.mark_live_mutation_changed(above, Instant::now(), false);
        let current = stream.revisions.dirty(leaf).unwrap().revision;
        assert_ne!(current, previous);
        assert_eq!(
            stream.revisions.dirty(plain).unwrap().revision,
            plain_generation
        );
        assert_eq!(
            stream.revisions.dirty(neighbor).unwrap().revision,
            neighbour_generation
        );
        assert!(stream.register_mesh_dependency_mask(
            leaf,
            current,
            MeshDependencyMask::default().with_seasonal_foliage(true)
        ));
        stream.mesh_jobs.pending.clear();
    }
}

#[test]
fn seasonal_snapshot_captures_higher_palette_sources_without_replacing_the_ao_halo() {
    let mut stream = seasonal_stream();
    let leaf = SubChunkKey::new(0, 0, 0, 0);
    for y in [0, 1, 3] {
        stream
            .authority
            .commit_sub_chunk(
                SubChunkKey::from_chunk(leaf.chunk(), y),
                uniform_sub_chunk(1),
            )
            .unwrap();
    }
    let snapshot = stream.mesh_snapshot(
        leaf,
        stream.authority.terrain().sub_chunk(leaf).unwrap(),
        MeshLightHalo::default(),
    );
    assert_eq!(
        snapshot
            .neighbourhood()
            .seasonal_column()
            .map(|(y, _)| y)
            .collect::<Vec<_>>(),
        [0, 1, 3]
    );
    assert!(snapshot.neighbourhood().sub_chunk([0, 1, 0]).is_some());
    assert_eq!(snapshot.neighbourhood().seasonal_column().count(), 3);
}

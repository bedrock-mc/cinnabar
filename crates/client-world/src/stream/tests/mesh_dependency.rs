use std::time::Instant;

use protocol::WorldBootstrap;
use world::{DecodedLevelChunk, MeshDependencyMask, SubChunkKey};

use super::WorldStream;

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    })
}

#[test]
fn diagonal_change_invalidates_ao_dependents() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, 0, 0);
    let dependent = SubChunkKey::new(0, 1, 1, 0);
    stream.resident.insert(dependent);
    let generation = stream.mark_dirty_exact(dependent, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        dependent,
        generation,
        MeshDependencyMask::new(true, false),
    ));
    stream.pending_mesh.clear();

    stream.mark_changed(source, Instant::now());

    assert_ne!(
        stream.revisions.dirty(dependent).unwrap().revision,
        generation
    );
    assert!(stream.pending_mesh.contains_key(&dependent));
}

#[test]
fn horizontal_corner_change_invalidates_liquid_dependent() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, 0, 0);
    let dependent = SubChunkKey::new(0, 1, 0, 1);
    stream.resident.insert(dependent);
    let generation = stream.mark_dirty_exact(dependent, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        dependent,
        generation,
        MeshDependencyMask::new(false, true),
    ));
    stream.pending_mesh.clear();

    stream.mark_changed(source, Instant::now());

    assert_ne!(
        stream.revisions.dirty(dependent).unwrap().revision,
        generation
    );
    assert!(stream.pending_mesh.contains_key(&dependent));
}

#[test]
fn liquid_dependency_skips_vertical_corner_outside_sample_set() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, 0, 0);
    let outside = SubChunkKey::new(0, 1, 1, 1);
    stream.resident.insert(outside);
    let generation = stream.mark_dirty_exact(outside, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        outside,
        generation,
        MeshDependencyMask::new(false, true),
    ));
    stream.pending_mesh.clear();

    stream.mark_changed(source, Instant::now());

    assert_eq!(
        stream.revisions.dirty(outside).unwrap().revision,
        generation
    );
    assert!(!stream.pending_mesh.contains_key(&outside));
}

#[test]
fn face_only_target_skips_diagonal_but_face_neighbour_still_dirties() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, 0, 0);
    let diagonal = SubChunkKey::new(0, 1, 0, 1);
    let face = SubChunkKey::new(0, 1, 0, 0);
    for target in [diagonal, face] {
        stream.resident.insert(target);
        let generation = stream.mark_dirty_exact(target, Instant::now());
        assert!(stream.register_mesh_dependency_mask(
            target,
            generation,
            MeshDependencyMask::default(),
        ));
    }
    let diagonal_generation = stream.revisions.dirty(diagonal).unwrap().revision;
    let face_generation = stream.revisions.dirty(face).unwrap().revision;
    stream.pending_mesh.clear();

    stream.mark_changed(source, Instant::now());

    assert_eq!(
        stream.revisions.dirty(diagonal).unwrap().revision,
        diagonal_generation
    );
    assert!(!stream.pending_mesh.contains_key(&diagonal));
    assert_ne!(
        stream.revisions.dirty(face).unwrap().revision,
        face_generation
    );
    assert!(stream.pending_mesh.contains_key(&face));
}

#[test]
fn rapid_liquid_changes_coalesce_latest_generation_and_oldest_since() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, 0, 0);
    let dependent = SubChunkKey::new(0, 1, 0, 1);
    stream.resident.insert(dependent);
    let registered_at = Instant::now();
    let registered = stream.mark_dirty_exact(dependent, registered_at);
    assert!(stream.register_mesh_dependency_mask(
        dependent,
        registered,
        MeshDependencyMask::new(false, true),
    ));
    stream.pending_mesh.clear();
    let first_at = Instant::now();

    let before_first = stream.revisions.next_revision;
    stream.mark_changed_sources([source, source], first_at);
    assert_eq!(
        stream.revisions.next_revision - before_first,
        8,
        "duplicate sources must assign one revision per deduplicated dirty target"
    );
    let first = stream.pending_mesh[&dependent];
    let second_at = first_at + std::time::Duration::from_millis(5);
    let before_second = stream.revisions.next_revision;
    stream.mark_changed_sources([source, source], second_at);
    assert_eq!(
        stream.revisions.next_revision - before_second,
        8,
        "each rapid batch must revise every deduplicated dirty target exactly once"
    );
    let second = stream.pending_mesh[&dependent];

    assert_ne!(first.revision, second.revision);
    assert_eq!(
        second.revision,
        stream.revisions.dirty(dependent).unwrap().revision
    );
    assert_eq!(first.since, registered_at);
    assert_eq!(second.since, registered_at);
}

#[test]
fn known_empty_mask_skips_diagonal_change() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, 0, 0);
    let diagonal = SubChunkKey::new(0, 1, 1, 0);
    stream.resident.insert(diagonal);
    let generation = stream.mark_dirty_exact(diagonal, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        diagonal,
        generation,
        MeshDependencyMask::default(),
    ));
    stream.pending_mesh.clear();

    stream.mark_changed(source, Instant::now());

    assert_eq!(
        stream.revisions.dirty(diagonal).unwrap().revision,
        generation
    );
    assert!(!stream.pending_mesh.contains_key(&diagonal));
}

#[test]
fn unknown_new_mask_dirties_diagonal_conservatively() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, 0, 0);
    let diagonal = SubChunkKey::new(0, 1, 1, 0);
    stream.resident.insert(diagonal);

    stream.mark_changed(source, Instant::now());

    assert!(stream.pending_mesh.contains_key(&diagonal));
}

#[test]
fn inline_full_column_change_invalidates_registered_corner_dependency() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, -4, 0);
    let corner = SubChunkKey::new(0, 1, -4, 1);
    let decoded = DecodedLevelChunk::decode(
        source.y,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
    )
    .unwrap();
    stream
        .store
        .commit_level_chunk(source.chunk(), decoded)
        .unwrap();
    stream.resident.insert(source);
    stream.resident.insert(corner);
    let generation = stream.mark_dirty_exact(corner, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        corner,
        generation,
        MeshDependencyMask::new(true, false),
    ));
    stream.pending_mesh.clear();

    stream.submit(1, super::inline_air_event(0)).unwrap();
    super::complete_pending_decode_jobs(&mut stream);

    assert_ne!(stream.revisions.dirty(corner).unwrap().revision, generation);
    assert!(stream.pending_mesh.contains_key(&corner));
    assert_eq!(
        stream.revisions.next_revision - generation,
        stream.pending_mesh.len() as u64,
        "one inline batch must assign exactly one revision per dirty target"
    );
}

#[test]
fn known_air_removal_replaces_stale_mask_and_skips_later_diagonal_change() {
    let mut stream = stream();
    let target = SubChunkKey::new(0, 1, -4, 0);
    let diagonal_source = SubChunkKey::new(0, 0, -4, 1);
    let decoded = DecodedLevelChunk::decode(
        target.y,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
    )
    .unwrap();
    stream
        .store
        .commit_level_chunk(target.chunk(), decoded)
        .unwrap();
    stream.resident.insert(target);
    let stale_generation = stream.mark_dirty_exact(target, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        target,
        stale_generation,
        MeshDependencyMask::new(true, true),
    ));
    stream.pending_mesh.clear();

    stream.submit(1, super::inline_air_event(target.x)).unwrap();
    super::complete_pending_decode_jobs(&mut stream);

    assert!(stream.known_air.contains(&target));
    assert_eq!(
        stream.mesh_dependency_mask(target),
        None,
        "transitioning to known air must clear the stale non-empty mask"
    );
    let empty_generation = stream.revisions.dirty(target).unwrap().revision;
    stream.dispatch_mesh_jobs([24.0, -56.0, 8.0], 1);
    assert_eq!(
        stream.mesh_dependency_mask(target),
        Some((empty_generation, MeshDependencyMask::default())),
        "the exact queued removal generation must register known-empty dependencies"
    );

    stream.mark_changed(diagonal_source, Instant::now());

    assert_eq!(
        stream.revisions.dirty(target).unwrap().revision,
        empty_generation
    );
    assert!(!stream.pending_mesh.contains_key(&target));
}

#[test]
fn mask_generation_replacement() {
    let mut stream = stream();
    let key = SubChunkKey::new(0, 3, 4, 5);
    stream.resident.insert(key);
    let first = stream.mark_dirty_exact(key, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        key,
        first,
        MeshDependencyMask::new(false, false),
    ));
    let second = stream.mark_dirty_exact(key, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        key,
        second,
        MeshDependencyMask::new(true, false),
    ));

    assert_eq!(
        stream.mesh_dependency_mask(key),
        Some((second, MeshDependencyMask::new(true, false)))
    );
}

#[test]
fn stale_mask_rejection() {
    let mut stream = stream();
    let key = SubChunkKey::new(0, 3, 4, 5);
    stream.resident.insert(key);
    let stale = stream.mark_dirty_exact(key, Instant::now());
    let current = stream.mark_dirty_exact(key, Instant::now());

    assert!(
        !stream.register_mesh_dependency_mask(key, stale, MeshDependencyMask::new(true, true),)
    );
    assert_eq!(stream.mesh_dependency_mask(key), None);
    assert!(stream.register_mesh_dependency_mask(
        key,
        current,
        MeshDependencyMask::new(false, true),
    ));
}

#[test]
fn private_snapshot_populates_the_shared_liquid_neighbourhood() {
    let mut stream = stream();
    let center_key = SubChunkKey::new(0, 20, 7, -30);
    for (index, [dx, dy, dz]) in world::MeshNeighbourhood::liquid_sample_offsets().enumerate() {
        let key = SubChunkKey::new(
            center_key.dimension,
            center_key.x + i32::from(dx),
            center_key.y + i32::from(dy),
            center_key.z + i32::from(dz),
        );
        stream
            .store
            .commit_sub_chunk(key, super::uniform_sub_chunk(100 + index as u32))
            .unwrap();
    }
    let center = stream.store.sub_chunk(center_key).unwrap();

    let snapshot = stream.mesh_snapshot(center_key, center, Default::default());
    let neighbourhood = snapshot.neighbourhood();
    let liquid = neighbourhood.liquid_sub_chunks().collect::<Vec<_>>();

    assert_eq!(liquid.len(), 23);
    assert!(liquid.iter().all(|(_, sub_chunk)| sub_chunk.is_some()));
    for (index, (_, sub_chunk)) in liquid.into_iter().enumerate() {
        assert_eq!(
            sub_chunk.unwrap().runtime_id(0, 0, 0, 0),
            Some(100 + index as u32)
        );
    }
    assert!(neighbourhood.sub_chunk([1, -1, 1]).is_none());
}

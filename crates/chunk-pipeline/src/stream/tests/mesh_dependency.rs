use std::time::Instant;

use protocol::WorldBootstrap;
use world::{DecodedLevelChunk, MeshDependencyMask, NeighbourReach, SubChunkKey};

use super::WorldStream;

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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
    stream.mesh_jobs.pending.clear();

    stream.mark_changed(source, Instant::now());

    assert_ne!(
        stream.revisions.dirty(dependent).unwrap().revision,
        generation
    );
    assert!(stream.mesh_jobs.pending.contains_key(&dependent));
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
    stream.mesh_jobs.pending.clear();

    stream.mark_changed(source, Instant::now());

    assert_ne!(
        stream.revisions.dirty(dependent).unwrap().revision,
        generation
    );
    assert!(stream.mesh_jobs.pending.contains_key(&dependent));
}

/// Neighbour data cannot change geometry in an absent slot; explicit source removal still runs.
#[test]
fn absent_neighbours_do_not_queue_empty_removals() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, 0, 0);
    stream.mark_changed(source, Instant::now());
    assert_eq!(stream.mesh_jobs.pending.len(), 1);
    assert!(stream.mesh_jobs.pending.contains_key(&source));
    stream.dispatch_mesh_jobs([0.0; 3], 1);
    assert!(matches!(
        stream.pop_mesh_change(),
        Some(super::WorldMeshChange::Remove { key, .. }) if key == source
    ));
    assert!(stream.mesh_jobs.pending.is_empty());
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
    stream.mesh_jobs.pending.clear();

    stream.mark_changed(source, Instant::now());

    assert_eq!(
        stream.revisions.dirty(outside).unwrap().revision,
        generation
    );
    assert!(!stream.mesh_jobs.pending.contains_key(&outside));
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
    stream.mesh_jobs.pending.clear();

    stream.mark_changed(source, Instant::now());

    assert_eq!(
        stream.revisions.dirty(diagonal).unwrap().revision,
        diagonal_generation
    );
    assert!(!stream.mesh_jobs.pending.contains_key(&diagonal));
    assert_ne!(
        stream.revisions.dirty(face).unwrap().revision,
        face_generation
    );
    assert!(stream.mesh_jobs.pending.contains_key(&face));
}

#[test]
fn adding_and_removing_snow_above_face_only_grass_invalidates_lower_subchunk() {
    let mut stream = stream();
    // Grass at local y=15 samples a snow block at y=0 in the subchunk above.
    let grass = SubChunkKey::new(0, 0, 0, 0);
    let snow = SubChunkKey::new(0, 0, 1, 0);
    stream.resident.insert(grass);
    let now = Instant::now();
    let mut generation = stream.mark_dirty_exact(grass, now);
    for _mutation in ["add snow", "remove snow"] {
        assert!(stream.register_mesh_dependency_mask(
            grass,
            generation,
            MeshDependencyMask::default()
        ));
        stream.mesh_jobs.pending.clear();
        stream.mark_live_mutation_changed(snow, now, false);
        let revised = stream.revisions.dirty(grass).unwrap().revision;
        assert_ne!(revised, generation);
        assert!(stream.mesh_jobs.pending.contains_key(&grass));
        generation = revised;
    }
}

#[test]
fn rapid_liquid_changes_coalesce_latest_generation_and_oldest_since() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, 0, 0);
    let dependent = SubChunkKey::new(0, 1, 0, 1);
    stream.resident.extend(source.mesh_dependents());
    stream.resident.insert(dependent);
    let registered_at = Instant::now();
    let registered = stream.mark_dirty_exact(dependent, registered_at);
    assert!(stream.register_mesh_dependency_mask(
        dependent,
        registered,
        MeshDependencyMask::new(false, true),
    ));
    stream.mesh_jobs.pending.clear();
    let first_at = Instant::now();

    let before_first = stream.revisions.next_revision;
    stream.mark_changed_sources([source, source], first_at);
    assert_eq!(
        stream.revisions.next_revision - before_first,
        8,
        "duplicate sources must assign one revision per deduplicated dirty target"
    );
    let first = stream.mesh_jobs.pending[&dependent];
    let second_at = first_at + std::time::Duration::from_millis(5);
    let before_second = stream.revisions.next_revision;
    stream.mark_changed_sources([source, source], second_at);
    assert_eq!(
        stream.revisions.next_revision - before_second,
        0,
        "pending snapshots must coalesce repeated invalidations"
    );
    let second = stream.mesh_jobs.pending[&dependent];

    assert_eq!(first.revision, second.revision);
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
    stream.mesh_jobs.pending.clear();

    stream.mark_changed(source, Instant::now());

    assert_eq!(
        stream.revisions.dirty(diagonal).unwrap().revision,
        generation
    );
    assert!(!stream.mesh_jobs.pending.contains_key(&diagonal));
}

#[test]
fn unknown_new_mask_dirties_diagonal_conservatively() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, 0, 0);
    let diagonal = SubChunkKey::new(0, 1, 1, 0);
    stream.resident.insert(diagonal);

    stream.mark_changed(source, Instant::now());

    assert!(stream.mesh_jobs.pending.contains_key(&diagonal));
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
        &super::RAW_IDS,
    );
    stream
        .authority
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
    stream.mesh_jobs.pending.clear();

    stream.submit(1, super::inline_air_event(0)).unwrap();
    super::complete_pending_decode_jobs(&mut stream);

    assert_ne!(stream.revisions.dirty(corner).unwrap().revision, generation);
    assert!(stream.mesh_jobs.pending.contains_key(&corner));
    assert_eq!(
        stream.revisions.next_revision - generation,
        stream.mesh_jobs.pending.len() as u64,
        "one inline batch must assign exactly one revision per dirty target"
    );
}

#[test]
fn known_air_removal_replaces_stale_mask_and_skips_neighbour_changes() {
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
        &super::RAW_IDS,
    );
    stream
        .authority
        .commit_level_chunk(target.chunk(), decoded)
        .unwrap();
    stream.resident.insert(target);
    let stale_generation = stream.mark_dirty_exact(target, Instant::now());
    assert!(stream.register_mesh_dependency_mask(
        target,
        stale_generation,
        MeshDependencyMask::new(true, true),
    ));
    stream.mesh_jobs.pending.clear();

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
    assert!(!stream.mesh_jobs.pending.contains_key(&target));
    stream.mark_changed(SubChunkKey::new(0, 0, -4, 0), Instant::now());
    assert!(!stream.mesh_jobs.pending.contains_key(&target));
    stream.mark_changed(target, Instant::now());
    assert!(stream.mesh_jobs.pending.contains_key(&target));
    assert_ne!(
        stream.revisions.dirty(target).unwrap().revision,
        empty_generation
    );
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

/// Queued neighbour work preserves dependency facts, while source and asset replacements expire them.
#[test]
fn dependency_masks_follow_blocks_and_assets_instead_of_mesh_revisions() {
    let mut stream = stream();
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(7))
        .unwrap();
    stream.sync_resident(key);
    let now = test_time::ManualClock::new().now();
    let generation = stream.mark_dirty_exact(key, now);
    let mask = MeshDependencyMask::default();
    assert!(stream.register_mesh_dependency_mask(key, generation, mask));
    stream.mesh_jobs.clear_queued();
    stream.mark_changed(SubChunkKey::new(0, 1, 0, 0), now);
    assert_ne!(stream.revisions.dirty(key).unwrap().revision, generation);
    assert_eq!(stream.current_mesh_dependency_mask(key), Some(mask));

    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(8))
        .unwrap();
    assert_eq!(stream.current_mesh_dependency_mask(key), None);
    let generation = stream.revisions.dirty(key).unwrap().revision;
    assert!(stream.register_mesh_dependency_mask(key, generation, mask));
    let old_assets = std::sync::Arc::downgrade(stream.authority.runtime_assets());
    stream.reload_resource_assets(std::sync::Arc::new(assets::RuntimeAssets::diagnostic()));
    assert_eq!(stream.current_mesh_dependency_mask(key), None);
    assert!(old_assets.upgrade().is_none());
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
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(100 + index as u32))
            .unwrap();
    }
    let center = stream.authority.terrain().sub_chunk(center_key).unwrap();

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

/// A block change remeshes a neighbour only when a changed cell borders it.
#[test]
fn block_changes_remesh_only_the_neighbours_their_cells_border() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, 0, 0);
    let east = SubChunkKey::new(0, 1, 0, 0);
    let diagonal = SubChunkKey::new(0, 1, 1, 0);
    for key in [source, east, diagonal] {
        stream.resident.insert(key);
    }
    let now = Instant::now();
    stream.mark_block_mutation_changed(source, now, false, NeighbourReach::of_cell(8, 8, 8), &[]);
    assert!(stream.mesh_jobs.pending.contains_key(&source));
    assert!(!stream.mesh_jobs.pending.contains_key(&east));
    assert!(!stream.mesh_jobs.pending.contains_key(&diagonal));

    stream.mark_block_mutation_changed(source, now, false, NeighbourReach::of_cell(15, 8, 8), &[]);
    assert!(stream.mesh_jobs.pending.contains_key(&east));
    assert!(!stream.mesh_jobs.pending.contains_key(&diagonal));
}

/// Leaf depth reads a neighbouring leaf's own neighbours, so a change two cells from a face
/// remeshes the sub-chunk across it.
#[test]
fn block_changes_two_cells_from_a_face_remesh_the_neighbour_across_it() {
    let mut stream = stream();
    let source = SubChunkKey::new(0, 0, 0, 0);
    let west = SubChunkKey::new(0, -1, 0, 0);
    let east = SubChunkKey::new(0, 1, 0, 0);
    for key in [source, west, east] {
        stream.resident.insert(key);
    }
    let now = Instant::now();
    stream.mark_block_mutation_changed(source, now, false, NeighbourReach::of_cell(2, 8, 8), &[]);
    assert!(!stream.mesh_jobs.pending.contains_key(&west));

    stream.mark_block_mutation_changed(source, now, false, NeighbourReach::of_cell(1, 8, 8), &[]);
    assert!(stream.mesh_jobs.pending.contains_key(&west));
    assert!(!stream.mesh_jobs.pending.contains_key(&east));

    stream.mark_block_mutation_changed(source, now, false, NeighbourReach::of_cell(14, 8, 8), &[]);
    assert!(stream.mesh_jobs.pending.contains_key(&east));
}

/// Air (0), twin cubes 1 and 2 with identical visuals and light, and a visibly different cube 3.
fn twin_cube_assets() -> assets::RuntimeAssets {
    use assets::{BlockFlags, BlockVisual, NO_ANIMATION, NO_MODEL_TEMPLATE, VisualKind};
    let visual = |faces, flags, kind, contributor_role| BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        support: assets::VisualSupport::Exact,
        faces,
        flags,
        kind,
        contributor_role,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
    };
    let cube = |material| {
        visual(
            [material; 6],
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            VisualKind::Cube,
            assets::ContributorRole::Primary,
        )
    };
    let air = visual(
        [0; 6],
        BlockFlags::AIR,
        VisualKind::Invisible,
        assets::ContributorRole::Air,
    );
    let material = assets::Material {
        texture: assets::TextureRef::DIAGNOSTIC,
        flags: 0,
        animation: NO_ANIMATION,
        ..assets::Material::unvaried()
    };
    let mips = [16_u32, 8, 4, 2, 1]
        .map(|size| assets::TextureMip {
            size,
            rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
        })
        .into();
    let compiled = assets::CompiledAssets {
        visuals: vec![air, cube(1), cube(1), cube(2)].into_boxed_slice(),
        light_properties: vec![assets::LightProperties::default(); 4].into_boxed_slice(),
        hashed: Box::new([]),
        materials: vec![material; 3].into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![assets::TexturePage::new(assets::TextureArray {
            layers: 1,
            mips,
        })]
        .into_boxed_slice(),
        biomes: assets::CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [0xA5; 32],
            block_registry_sha256: [0x5A; 32],
            light_registry_sha256: [0x33; 32],
            biome_registry_sha256: [0x3C; 32],
        },
    };
    assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap()
}

/// A server state change between visually identical states keeps every mesh; a visible one
/// still remeshes the section and the neighbour its cell borders.
#[test]
fn mesh_neutral_block_changes_keep_their_meshes() {
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        std::sync::Arc::new(twin_cube_assets()),
        [0.0; 3],
        None,
    );
    let source = SubChunkKey::new(0, 0, 0, 0);
    let east = SubChunkKey::new(0, 1, 0, 0);
    for (key, id) in [(source, 1), (east, 3)] {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(id))
            .unwrap();
        stream.resident.insert(key);
    }
    let update = |stream: &mut WorldStream, id| {
        let mutation = world::ChunkStore::prepare_sub_chunk_blocks(
            source,
            stream.authority.terrain().sub_chunk(source).as_deref(),
            &[world::BlockUpdate::new(15, 0, 0, 0, id)],
            0,
        )
        .unwrap();
        assert!(stream.commit_block_mutations(vec![mutation]));
        assert_eq!(
            stream
                .authority
                .terrain()
                .sub_chunk(source)
                .unwrap()
                .runtime_id(0, 15, 0, 0),
            Some(id)
        );
    };

    update(&mut stream, 2);
    assert!(stream.mesh_jobs.pending.is_empty());
    assert!(stream.revisions.dirty(source).is_none());
    assert!(stream.revisions.dirty(east).is_none());

    update(&mut stream, 3);
    assert!(stream.mesh_jobs.pending.contains_key(&source));
    assert!(stream.mesh_jobs.pending.contains_key(&east));
}

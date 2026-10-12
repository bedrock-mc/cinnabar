use super::*;
use ::meshing::occlusion::OcclusionBudget;

/// A same-section opaque wall, with its uploaded mesh explicitly acknowledged.
fn wall_stream() -> (WorldStream, SubChunkKey) {
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.5, 4.5, 4.5],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(cave_test_assets()),
        [0.5, 4.5, 4.5],
        None,
    );
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, cave_test_slab(2))
        .unwrap();
    stream.resident.insert(key);
    stream.applied_mesh_generations.insert(key, 1);
    (stream, key)
}

/// Tests the body behind the wall without any cave-graph rejection.
fn hidden(stream: &WorldStream) -> bool {
    stream.terrain_hides_box(
        &mut OcclusionBudget::default(),
        [0.5, 4.5, 4.5],
        [12.0, 3.0, 3.0],
        [13.0, 6.0, 6.0],
        |_, _| true,
    )
}

#[test]
fn only_current_drawn_opaque_terrain_hides_an_entity() {
    let (mut stream, key) = wall_stream();
    assert!(hidden(&stream));
    assert!(!stream.terrain_hides_box(
        &mut OcclusionBudget::default(),
        [0.5, 4.5, 4.5],
        [12.0, 3.0, 3.0],
        [13.0, 6.0, 6.0],
        |_, _| false
    ));
    stream.applied_mesh_generations.remove(&key);
    assert!(!hidden(&stream));
    stream.applied_mesh_generations.insert(key, 1);
    stream.mark_dirty_exact(key, Instant::now());
    assert!(!hidden(&stream));
}

#[test]
fn clear_sight_and_cutout_terrain_draw() {
    let (mut stream, key) = wall_stream();
    // Leaves do not certify a solid occluder, even after their mesh is uploaded.
    stream
        .authority
        .commit_sub_chunk(key, cave_test_slab(1))
        .unwrap();
    assert!(!hidden(&stream));
    stream
        .authority
        .commit_sub_chunk(key, cave_test_slab(0))
        .unwrap();
    assert!(!hidden(&stream));
}

#[test]
fn occupied_cells_with_no_exposed_face_cannot_certify_depth() {
    let (mut stream, key) = wall_stream();
    // The camera and actor are inside the solid section. Its outer faces cannot hide
    // the actor, and its internal voxel faces were removed by terrain meshing.
    stream
        .authority
        .commit_sub_chunk(key, uniform_sub_chunk(2))
        .unwrap();
    assert!(!hidden(&stream));
}

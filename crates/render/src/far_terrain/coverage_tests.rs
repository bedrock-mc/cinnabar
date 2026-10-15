use super::*;
use far_terrain::{COLUMN_SIDE, ColumnPos};

/// Builds visible test geometry at the requested tile keys without running tile workers.
fn meshes(keys: impl IntoIterator<Item = TileKey>) -> Vec<Arc<TileMesh>> {
    keys.into_iter()
        .enumerate()
        .map(|(index, key)| {
            Arc::new(TileMesh {
                key,
                revision: index as u64 + 1,
                quads: vec![[1 << 14 | 1 << 21, 64]].into_boxed_slice(),
                colours: vec![100; TILE_CELLS * TILE_CELLS * 4].into_boxed_slice(),
                min_y: 0,
                max_y: 64,
            })
        })
        .collect()
}

/// Creates a no-op device with an explicit layer ceiling for boundary transitions.
fn device(slots: u32) -> (RenderDevice, RenderQueue) {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_texture_array_layers: slots,
            ..Default::default()
        },
        ..Default::default()
    });
    (RenderDevice::from(device), RenderQueue::new(queue))
}

/// Covers a row of coarse tiles with its sixteen finest tiles per coarse square.
fn fine_keys(coarse_count: i32) -> impl Iterator<Item = TileKey> {
    (0..coarse_count).flat_map(|coarse| {
        (0..4).flat_map(move |z| {
            (0..4).map(move |x| TileKey {
                level: 0,
                x: coarse * 4 + x,
                z,
            })
        })
    })
}

/// Requires one actual draw, and no duplicate fallback draw, over every previously covered column.
fn assert_covered(gpu: &FarTerrainGpu, old: &[Arc<TileMesh>]) {
    for column in old.iter().flat_map(|mesh| mesh.key.columns()) {
        let block = [column.x * COLUMN_SIDE as i32, column.z * COLUMN_SIDE as i32];
        let covering = gpu
            .resident
            .values()
            .filter(|resident| {
                gpu.draws.iter().any(|draw| draw.slot == resident.slot)
                    && resident.mesh.key
                        == TileKey::containing(resident.mesh.key.level, block[0], block[1])
            })
            .count();
        assert_eq!(covering, 1, "coverage over {column:?}");
    }
}

/// Checks the acknowledgement for the fixture's CPU-selected mesh over a handoff column.
fn ready(scene: &FarTerrainScene, column: ColumnPos) -> bool {
    let mesh = scene
        .tiles
        .iter()
        .find(|mesh| mesh.key.columns().any(|pos| pos == column))
        .expect("CPU-ready handoff mesh");
    scene.residency.ready(column, mesh)
}

/// Completes initialization while leaving transition batches individually observable.
fn populate(
    gpu: &mut FarTerrainGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    meshes: &[Arc<TileMesh>],
) {
    for _ in 0..=meshes.len() {
        if gpu.apply_tiles(device, queue, meshes) {
            return;
        }
    }
    panic!("initial coverage did not settle");
}

#[test]
fn coarse_to_fine_keeps_covering_draws_across_all_upload_batches() {
    let (device, queue) = device(256);
    let mut gpu = FarTerrainGpu::new(&device);
    let old = meshes((0..2).map(|x| TileKey { level: 2, x, z: 0 }));
    populate(&mut gpu, &device, &queue, &old);
    let replacement = meshes(fine_keys(2));
    for batch in 0..4 {
        let complete = gpu.apply_tiles(&device, &queue, &replacement);
        assert_eq!(complete, batch == 3);
        assert_covered(&gpu, &old);
        if batch == 0 {
            assert!(old.iter().all(|mesh| gpu.resident[&mesh.key].visible));
            assert!(
                replacement[..UPLOAD_TILES_PER_FRAME]
                    .iter()
                    .all(|mesh| !gpu.resident[&mesh.key].visible)
            );
        }
    }
    assert!(old.iter().all(|mesh| !gpu.resident.contains_key(&mesh.key)));
    assert_eq!(gpu.draws.len(), replacement.len());
}

#[test]
fn fine_to_coarse_retires_each_cover_only_after_its_replacement_uploads() {
    let (device, queue) = device(256);
    let mut gpu = FarTerrainGpu::new(&device);
    let count = UPLOAD_TILES_PER_FRAME as i32 + 1;
    let old = meshes((0..count).flat_map(|coarse| {
        (0..2).flat_map(move |z| {
            (0..2).map(move |x| TileKey {
                level: 0,
                x: coarse * 2 + x,
                z,
            })
        })
    }));
    populate(&mut gpu, &device, &queue, &old);
    let replacement = meshes((0..count).map(|x| TileKey { level: 1, x, z: 0 }));
    assert!(!gpu.apply_tiles(&device, &queue, &replacement));
    assert_covered(&gpu, &old);
    assert_eq!(gpu.draws.len(), UPLOAD_TILES_PER_FRAME + 4);
    assert!(gpu.apply_tiles(&device, &queue, &replacement));
    assert_covered(&gpu, &old);
    assert_eq!(gpu.draws.len(), replacement.len());
}

#[test]
fn final_replacement_can_reuse_its_covering_slot_at_the_layer_limit() {
    let (device, queue) = device(16);
    let mut gpu = FarTerrainGpu::new(&device);
    let old = meshes([TileKey {
        level: 2,
        x: 0,
        z: 0,
    }]);
    populate(&mut gpu, &device, &queue, &old);
    let replacement = meshes(fine_keys(1));
    assert!(!gpu.apply_tiles(&device, &queue, &replacement));
    assert_covered(&gpu, &old);
    assert!(gpu.apply_tiles(&device, &queue, &replacement));
    assert_covered(&gpu, &old);
    assert_eq!(gpu.resident.len(), 16);
    assert!(!gpu.resident.contains_key(&old[0].key));
}

#[test]
fn a_replacement_that_cannot_fit_keeps_the_old_draw_and_remains_pending() {
    let (device, queue) = device(1);
    let mut gpu = FarTerrainGpu::new(&device);
    let old = meshes([TileKey {
        level: 2,
        x: 0,
        z: 0,
    }]);
    populate(&mut gpu, &device, &queue, &old);
    let replacement = meshes(fine_keys(1));
    for _ in 0..3 {
        assert!(!gpu.apply_tiles(&device, &queue, &replacement));
        assert_covered(&gpu, &old);
        assert_eq!(gpu.resident.len(), 1);
    }
}

#[test]
fn a_failed_quad_reservation_preserves_the_previous_revision_and_range() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_storage_buffer_binding_size: 2 * QUAD_BYTES,
            ..Default::default()
        },
        ..Default::default()
    });
    let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
    let mut gpu = FarTerrainGpu::new(&device);
    let old = meshes([TileKey {
        level: 0,
        x: 0,
        z: 0,
    }]);
    populate(&mut gpu, &device, &queue, &old);
    let range = gpu.resident[&old[0].key].quads.clone();
    let mut replacement = meshes([old[0].key]);
    let mesh = Arc::get_mut(&mut replacement[0]).unwrap();
    mesh.revision += 1;
    mesh.quads = vec![[1, 64]; 3].into_boxed_slice();
    assert!(!gpu.apply_tiles(&device, &queue, &replacement));
    assert!(Arc::ptr_eq(&gpu.resident[&old[0].key].mesh, &old[0]));
    assert_eq!(gpu.resident[&old[0].key].quads, range);
    assert_covered(&gpu, &old);
}

#[test]
fn same_key_handoff_acknowledgements_require_the_current_revision() {
    use bevy::{ecs::system::RunSystemOnce, prelude::World};
    let (device, queue) = device(256);
    let mut world = World::new();
    world.insert_resource(FarTerrainGpu::new(&device));
    world.insert_resource(device);
    world.insert_resource(queue);
    let old = meshes((0..=UPLOAD_TILES_PER_FRAME).map(|x| TileKey {
        level: 0,
        x: x as i32,
        z: 0,
    }));
    world.insert_resource(FarTerrainScene {
        tiles: old.into(),
        ..Default::default()
    });
    for _ in 0..2 {
        world.run_system_once(prepare_far_terrain).unwrap();
    }
    let mut replacement = meshes((0..=UPLOAD_TILES_PER_FRAME).map(|x| TileKey {
        level: 0,
        x: x as i32,
        z: 0,
    }));
    for mesh in &mut replacement {
        Arc::get_mut(mesh).unwrap().revision += 100;
    }
    let column = replacement.last().unwrap().key.columns().next().unwrap();
    world.resource_mut::<FarTerrainScene>().tiles = replacement.into();
    world.run_system_once(prepare_far_terrain).unwrap();
    assert!(!ready(world.resource::<FarTerrainScene>(), column));
    world.run_system_once(prepare_far_terrain).unwrap();
    assert!(ready(world.resource::<FarTerrainScene>(), column));
}

#[test]
fn near_handoffs_wait_for_drawable_replacements_and_update_the_mask_together() {
    use bevy::{ecs::system::RunSystemOnce, prelude::World};
    let (device, queue) = device(256);
    let mut world = World::new();
    world.insert_resource(FarTerrainGpu::new(&device));
    world.insert_resource(device);
    world.insert_resource(queue);
    let replacement = meshes(fine_keys(1));
    let mut handoffs: Vec<_> = replacement
        .iter()
        .map(|mesh| mesh.key.columns().next().unwrap())
        .collect();
    let mut scene = FarTerrainScene {
        tiles: replacement.clone().into(),
        ..Default::default()
    };
    scene.near = Arc::new(NearMask::from_columns([0, 0], 32, handoffs.iter().copied()));
    world.insert_resource(scene);
    for batch in 0..2 {
        world.run_system_once(prepare_far_terrain).unwrap();
        let mut scene = world.resource_mut::<FarTerrainScene>();
        let released: Vec<_> = handoffs
            .iter()
            .copied()
            .filter(|column| ready(&scene, *column))
            .collect();
        assert_eq!(released.len(), UPLOAD_TILES_PER_FRAME);
        handoffs.retain(|column| !released.contains(column));
        scene.near = Arc::new(NearMask::from_columns([0, 0], 32, handoffs.iter().copied()));
        for column in &handoffs {
            assert!(scene.near.contains(*column));
            assert!(!ready(&scene, *column));
        }
        for column in released {
            assert!(!scene.near.contains(column));
            assert!(ready(&scene, column));
        }
        assert_eq!(handoffs.is_empty(), batch == 1);
    }
}

#[test]
fn empty_cpu_tiles_wait_until_the_old_gpu_cover_retires() {
    use bevy::{ecs::system::RunSystemOnce, prelude::World};
    let (device, queue) = device(256);
    let mut world = World::new();
    world.insert_resource(FarTerrainGpu::new(&device));
    world.insert_resource(device);
    world.insert_resource(queue);
    world.insert_resource(FarTerrainScene {
        tiles: meshes([TileKey {
            level: 2,
            x: 0,
            z: 0,
        }])
        .into(),
        ..Default::default()
    });
    world.run_system_once(prepare_far_terrain).unwrap();
    let mut replacement = meshes(fine_keys(1));
    let mut empty = replacement.pop().unwrap();
    Arc::get_mut(&mut empty).unwrap().quads = Box::default();
    let column = empty.key.columns().next().unwrap();
    world.resource_mut::<FarTerrainScene>().tiles = replacement.into();
    world.run_system_once(prepare_far_terrain).unwrap();
    assert!(
        !world
            .resource::<FarTerrainScene>()
            .residency
            .ready(column, &empty)
    );
    world.run_system_once(prepare_far_terrain).unwrap();
    assert!(
        world
            .resource::<FarTerrainScene>()
            .residency
            .ready(column, &empty)
    );
}

#[test]
fn a_reversed_lod_change_reuses_the_cover_and_discards_hidden_partial_uploads() {
    let (device, queue) = device(256);
    let mut gpu = FarTerrainGpu::new(&device);
    let old = meshes([TileKey {
        level: 2,
        x: 0,
        z: 0,
    }]);
    populate(&mut gpu, &device, &queue, &old);
    let replacement = meshes(fine_keys(1));
    assert!(!gpu.apply_tiles(&device, &queue, &replacement));
    assert_covered(&gpu, &old);
    let (_, uploads) = render_scene::runtime_profile_uploads::measure(|| {
        assert!(gpu.apply_tiles(&device, &queue, &old));
    });
    assert_eq!(uploads, (0, 0));
    assert_eq!(gpu.resident.len(), old.len());
    assert_covered(&gpu, &old);
}

#[test]
fn a_new_world_does_not_inherit_fallbacks_or_upload_acknowledgements() {
    use bevy::{ecs::system::RunSystemOnce, prelude::World};
    let (device, queue) = device(256);
    let mut world = World::new();
    world.insert_resource(FarTerrainGpu::new(&device));
    world.insert_resource(device);
    world.insert_resource(queue);
    let old = meshes([TileKey {
        level: 2,
        x: 0,
        z: 0,
    }]);
    world.insert_resource(FarTerrainScene {
        tiles: old.clone().into(),
        ..Default::default()
    });
    world.run_system_once(prepare_far_terrain).unwrap();
    let replacement = meshes(fine_keys(1));
    let next = FarTerrainScene {
        tiles: replacement.into(),
        ..Default::default()
    };
    assert!(!ready(&next, ColumnPos::new(0, 0)));
    world.insert_resource(next);
    world.run_system_once(prepare_far_terrain).unwrap();
    let gpu = world.resource::<FarTerrainGpu>();
    assert!(!gpu.resident.contains_key(&old[0].key));
    assert_eq!(gpu.draws.len(), UPLOAD_TILES_PER_FRAME);
}

use super::*;
use far_terrain::{COLUMN_SIDE, ColumnPos};

/// Builds visible test geometry at the requested tile keys without running tile workers.
fn meshes(keys: impl IntoIterator<Item = TileKey>) -> Vec<Arc<TileMesh>> {
    keys.into_iter()
        .enumerate()
        .map(|(index, key)| {
            Arc::new(TileMesh {
                coverage: far_terrain::TileCoverage::Complete,
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
    capacity_device(slots, None)
}

/// Constrains layers or quad storage independently enough to exercise either allocation path.
fn capacity_device(slots: u32, quads: Option<u32>) -> (RenderDevice, RenderQueue) {
    let mut limits = wgpu::Limits {
        max_texture_array_layers: slots,
        ..Default::default()
    };
    if let Some(quads) = quads {
        limits.max_storage_buffer_binding_size = u64::from(quads) * QUAD_BYTES;
    }
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
        required_limits: limits,
        ..Default::default()
    });
    (RenderDevice::from(device), RenderQueue::new(queue))
}

/// Gives every tile the same allocation size while keeping visible geometry over its area.
fn sized_meshes(keys: impl IntoIterator<Item = TileKey>, quads: usize) -> Vec<Arc<TileMesh>> {
    let mut tiles = meshes(keys);
    for mesh in &mut tiles {
        let mesh = Arc::get_mut(mesh).unwrap();
        mesh.quads = vec![mesh.quads[0]; quads].into_boxed_slice();
    }
    tiles
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
fn interleaved_replacements_reclaim_unrelated_completed_cover_at_capacity() {
    for (layers, capacity, quads) in [(16, None, 1), (64, Some(64), 4)] {
        let (device, queue) = capacity_device(layers, capacity);
        let mut gpu = FarTerrainGpu::new(&device);
        let unchanged = sized_meshes((1..=8).map(|x| TileKey { level: 2, x, z: 0 }), quads);
        let mut old = unchanged.clone();
        old.extend(sized_meshes(
            (0..2).map(|z| TileKey { level: 1, x: 0, z }),
            quads,
        ));
        populate(&mut gpu, &device, &queue, &old);
        // The first parent finishes on upload six; upload seven needs its unrelated slot.
        let mut desired = sized_meshes(
            (0..2).flat_map(|x| (0..4).map(move |z| TileKey { level: 0, x, z })),
            quads,
        );
        desired.extend(unchanged.iter().cloned());
        let (complete, (writes, bytes)) = render_scene::runtime_profile_uploads::measure(|| {
            gpu.apply_tiles(&device, &queue, &desired)
        });
        assert!(complete);
        assert_eq!(writes, 3 * UPLOAD_TILES_PER_FRAME as u64);
        assert!(bytes <= UPLOAD_BYTES_PER_FRAME);
        assert_covered(&gpu, &old);
        assert_eq!(gpu.resident.len(), desired.len());
        for mesh in &desired {
            assert!(Arc::ptr_eq(&gpu.resident[&mesh.key].mesh, mesh));
            assert!(gpu.resident[&mesh.key].visible);
        }
    }
}

#[test]
fn failed_fine_tiles_retry_after_protected_coarse_tiles_shrink() {
    use bevy::{ecs::system::RunSystemOnce, prelude::World};
    // Four quads fit every fine tile after the shrink; an oversized tile never fits.
    let capacity = 64;
    for fine_quads in [4, capacity as usize + 1] {
        let (device, queue) = capacity_device(16, Some(capacity));
        let coarse_count = UPLOAD_TILES_PER_FRAME;
        let old = sized_meshes(
            (10..10 + coarse_count as i32).map(|x| TileKey { level: 2, x, z: 0 }),
            capacity as usize / coarse_count,
        );
        let mut world = World::new();
        world.insert_resource(FarTerrainGpu::new(&device));
        world.insert_resource(device);
        world.insert_resource(queue);
        world.insert_resource(FarTerrainScene {
            tiles: old.clone().into(),
            ..Default::default()
        });
        world.run_system_once(prepare_far_terrain).unwrap();
        world
            .resource_mut::<FarTerrainGpu>()
            .protect_coverage(old.iter().map(|mesh| mesh.key.columns().next().unwrap()));
        let smaller_quads = capacity as usize / coarse_count / 2;
        let mut smaller = sized_meshes(old.iter().map(|mesh| mesh.key), smaller_quads);
        for mesh in &mut smaller {
            Arc::get_mut(mesh).unwrap().revision += 100;
        }
        let mut desired = sized_meshes(
            (0..coarse_count as i32).map(|x| TileKey { level: 0, x, z: 0 }),
            fine_quads,
        );
        desired.extend(smaller);
        let desired: Arc<[Arc<TileMesh>]> = desired.into();
        world.resource_mut::<FarTerrainScene>().tiles = desired.clone();

        let (_, (writes, bytes)) = render_scene::runtime_profile_uploads::measure(|| {
            world.run_system_once(prepare_far_terrain).unwrap();
        });
        let gpu = world.resource::<FarTerrainGpu>();
        assert_eq!(writes, 3 * UPLOAD_TILES_PER_FRAME as u64);
        assert!(bytes <= UPLOAD_BYTES_PER_FRAME);
        assert!(
            desired[..coarse_count]
                .iter()
                .all(|mesh| !gpu.current(mesh))
        );
        assert!(desired[coarse_count..].iter().all(|mesh| gpu.current(mesh)));
        assert_covered(gpu, &old);
        assert!(
            gpu.applied_tiles.is_none(),
            "the early misses must retry the space freed by the later replacements"
        );

        let (_, (writes, bytes)) = render_scene::runtime_profile_uploads::measure(|| {
            world.run_system_once(prepare_far_terrain).unwrap();
        });
        let admitted =
            ((capacity as usize - coarse_count * smaller_quads) / fine_quads).min(coarse_count);
        let gpu = world.resource::<FarTerrainGpu>();
        assert_eq!(writes, 3 * admitted as u64);
        assert!(bytes <= UPLOAD_BYTES_PER_FRAME);
        assert_eq!(gpu.resident.len(), coarse_count + admitted);
        assert_covered(gpu, &old);
        assert_covered(gpu, &desired[..admitted]);
        let scene = world.resource::<FarTerrainScene>();
        for (index, mesh) in desired[..coarse_count].iter().enumerate() {
            assert_eq!(gpu.current(mesh), index < admitted);
            assert_eq!(
                scene
                    .residency
                    .ready(mesh.key.columns().next().unwrap(), mesh),
                index < admitted,
                "receipts include only drawable fine tiles"
            );
        }
        assert!(Arc::ptr_eq(gpu.applied_tiles.as_ref().unwrap(), &desired));
        let publication = gpu.publication_revision();
        let (_, writes) = render_scene::runtime_profile_uploads::measure(|| {
            world.run_system_once(prepare_far_terrain).unwrap();
        });
        assert_eq!(writes, (0, 0));
        assert_eq!(
            world.resource::<FarTerrainGpu>().publication_revision(),
            publication,
            "an unchanged scene stops retrying tiles that cannot fit"
        );
    }
}

#[test]
fn insufficient_replacement_capacity_keeps_partial_children_hidden_and_old_coverage_drawn() {
    for (layers, capacity, quads) in [(11, None, 1), (64, Some(44), 4)] {
        let (device, queue) = capacity_device(layers, capacity);
        let mut gpu = FarTerrainGpu::new(&device);
        let mut old = sized_meshes((10..18).map(|x| TileKey { level: 0, x, z: 0 }), quads);
        let mut desired = old.clone();
        old.extend(sized_meshes(
            [TileKey {
                level: 1,
                x: 0,
                z: 0,
            }],
            quads,
        ));
        populate(&mut gpu, &device, &queue, &old);
        desired.extend(sized_meshes(
            (0..2).flat_map(|x| (0..2).map(move |z| TileKey { level: 0, x, z })),
            quads,
        ));
        for _ in 0..3 {
            assert!(!gpu.apply_tiles(&device, &queue, &desired));
            assert_covered(&gpu, &old);
            assert_eq!(gpu.resident.len(), 11);
            assert!(old.iter().all(|mesh| gpu.resident[&mesh.key].visible));
            assert!(desired[8..].iter().all(|mesh| {
                gpu.resident
                    .get(&mesh.key)
                    .is_none_or(|resident| !resident.visible)
            }));
        }
    }
}

#[test]
fn far_drawer_stays_pinned_while_suppressed_near_resources_retire() {
    let (device, queue) = device(1);
    let mut gpu = FarTerrainGpu::new(&device);
    let old = meshes([TileKey {
        level: 2,
        x: 0,
        z: 0,
    }]);
    populate(&mut gpu, &device, &queue, &old);
    let column = old[0].key.columns().next().unwrap();
    gpu.protect_coverage(std::iter::once(column));
    // A view change cannot retire the outgoing column's last drawer during backpressure.
    gpu.apply_tiles(&device, &queue, &[]);
    assert_covered(&gpu, &old);
    let mut desired = old.clone();
    desired.extend(meshes([TileKey {
        level: 0,
        x: 0,
        z: 0,
    }]));
    gpu.apply_tiles(&device, &queue, &desired);
    assert!(Arc::ptr_eq(&gpu.resident[&old[0].key].mesh, &old[0]));
    assert_covered(&gpu, &old);
    gpu.protect_coverage(std::iter::empty());
    assert!(gpu.apply_tiles(&device, &queue, &[]));
    assert!(
        gpu.resident.is_empty(),
        "released coverage follows the new empty view"
    );
}

#[test]
fn a_proven_empty_selection_can_replace_pinned_geometry_without_a_new_draw() {
    let (device, queue) = device(1);
    let mut gpu = FarTerrainGpu::new(&device);
    let old = meshes([TileKey {
        level: 2,
        x: 0,
        z: 0,
    }]);
    populate(&mut gpu, &device, &queue, &old);
    let column = old[0].key.columns().next().unwrap();
    gpu.protect_coverage(std::iter::once(column));
    gpu.set_empty_coverage(Some(&old[0].key.columns().collect()));
    assert!(gpu.apply_tiles(&device, &queue, &[]));
    assert!(
        gpu.draws.is_empty(),
        "empty coverage is intentional absence of geometry"
    );
    assert!(gpu.covers_column(column));
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
fn near_handoff_receipts_are_bounded_to_drawable_replacements() {
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
    let scene = FarTerrainScene {
        tiles: replacement.clone().into(),
        ..Default::default()
    };
    world.insert_resource(scene);
    for batch in 0..2 {
        world.run_system_once(prepare_far_terrain).unwrap();
        let scene = world.resource::<FarTerrainScene>();
        let released: Vec<_> = handoffs
            .iter()
            .copied()
            .filter(|column| ready(scene, *column))
            .collect();
        assert_eq!(released.len(), UPLOAD_TILES_PER_FRAME);
        handoffs.retain(|column| !released.contains(column));
        for column in &handoffs {
            assert!(!ready(scene, *column));
        }
        for column in released {
            assert!(ready(scene, column));
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

/// The renderer consumes decoded column evidence, including partial empty and solid tiles.
#[test]
fn partial_summary_builds_never_prove_unknown_columns_or_replace_their_fallback() {
    use far_terrain::{ColumnSummary, SurfaceCell, TileGrid, mesh_tile};
    let (device, queue) = device(256);
    let key = TileKey {
        level: 0,
        x: 0,
        z: 0,
    };
    let known = key.columns().next().unwrap();
    let unknown = ColumnPos::new(known.x + 1, known.z);
    let mut solid = ColumnSummary::default();
    solid.cells_mut().fill(SurfaceCell {
        height: 64,
        rgb: [80; 3],
        water_depth: 0,
    });
    let partial = Arc::new(mesh_tile(&TileGrid::build(key, &[(known, solid.clone())])));
    let mut gpu = FarTerrainGpu::new(&device);
    assert!(gpu.apply_tiles(&device, &queue, std::slice::from_ref(&partial)));
    assert!(gpu.covers_column(known));
    assert!(!gpu.covers_column(unknown));
    let residency = render_scene::FarTerrainResidency::default();
    residency.publish(std::iter::once((key, partial.revision)));
    assert!(residency.ready(known, &partial));
    assert!(!residency.ready(unknown, &partial));

    let empty = mesh_tile(&TileGrid::build(key, &[(known, ColumnSummary::default())]));
    let empty_columns: HashSet<_> = empty.covered_columns().collect();
    gpu.invalidate_publication();
    gpu.set_empty_coverage(Some(&empty_columns));
    assert!(gpu.covers_column(known));
    assert!(!gpu.covers_column(unknown));
    let mut near = far_terrain::TerrainPublication::default();
    let section = world::SubChunkKey::new(0, unknown.x, 0, unknown.z);
    let pending = world::SubChunkKey { y: 1, ..section };
    let target = far_terrain::TerrainTarget {
        centre: section.chunk(),
        horizon: 32,
        near: std::sync::Arc::new(HashMap::from([(
            section.chunk(),
            HashMap::from([(section, 1), (pending, 1)]),
        )])),
        retained: std::sync::Arc::new(HashSet::from([section, pending])),
        empty_far: std::sync::Arc::new(empty_columns),
    };
    near.publish(&target, &HashMap::from([(section, 1)]), |column| {
        gpu.covers_column(ColumnPos::new(column.x, column.z))
    });
    assert!(
        near.draws_near(section.chunk()),
        "unknown terrain keeps the partially uploaded near drawer"
    );

    gpu.invalidate_publication();
    let coarse = TileKey {
        level: 1,
        x: 0,
        z: 0,
    };
    let original = Arc::new(mesh_tile(&TileGrid::build(
        coarse,
        &coarse
            .columns()
            .map(|pos| (pos, solid.clone()))
            .collect::<Vec<_>>(),
    )));
    populate(&mut gpu, &device, &queue, std::slice::from_ref(&original));
    let replacements: Vec<_> = [(0, 0), (1, 0), (0, 1), (1, 1)]
        .into_iter()
        .map(|(x, z)| {
            let key = TileKey { level: 0, x, z };
            Arc::new(mesh_tile(&TileGrid::build(
                key,
                &key.columns()
                    .filter(|pos| *pos != unknown)
                    .map(|pos| (pos, solid.clone()))
                    .collect::<Vec<_>>(),
            )))
        })
        .collect();
    assert!(gpu.apply_tiles(&device, &queue, &replacements));
    assert!(
        gpu.resident[&coarse].visible,
        "an uploaded partial replacement does not retire known coverage"
    );
    assert!(gpu.covers_column(unknown));
    let complete: Vec<_> = replacements
        .iter()
        .map(|mesh| {
            Arc::new(mesh_tile(&TileGrid::build(
                mesh.key,
                &mesh
                    .key
                    .columns()
                    .map(|pos| (pos, solid.clone()))
                    .collect::<Vec<_>>(),
            )))
        })
        .collect();
    assert!(gpu.apply_tiles(&device, &queue, &complete));
    assert!(!gpu.resident.contains_key(&coarse));
    assert!(gpu.covers_column(unknown));
}

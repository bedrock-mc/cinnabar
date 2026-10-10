use super::*;
use bevy::render::renderer::WgpuWrapper;

fn device() -> (RenderDevice, RenderQueue) {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    (
        RenderDevice::from(device),
        RenderQueue(Arc::new(WgpuWrapper::new(queue))),
    )
}

fn allocation(x: i32, index: u32) -> ArenaAllocation {
    let model_words = (PACKED_MODEL_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32;
    let lighting_words = (PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32;
    let draw_words = (PACKED_MODEL_DRAW_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32;
    let capacity = model_words * 2 + lighting_words + draw_words;
    let geometry = index * capacity;
    let model_start = geometry + model_words;
    let lighting_start = model_start + model_words;
    let draw_start = lighting_start + lighting_words;
    let gpu = GpuChunkAllocation {
        cube_layout: Default::default(),
        key: SubChunkKey::new(0, x, 0, 0),
        generation: 1,
        tint_identity: Default::default(),
        quad_range: index..index + 1,
        cube_lighting_range: Some(geometry..geometry + lighting_words),
        model_range: Some(model_start..lighting_start),
        model_lighting_range: Some(lighting_start..draw_start),
        model_draw_range: Some(draw_start..geometry + capacity),
        transparent_model_draw_range: None,
        liquid_range: None,
        liquid_lighting_range: None,
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_range: None,
        order_independent_liquid: false,
        metadata_index: index,
    };
    assert!(
        model_direct_draw_command(&gpu).is_some(),
        "fixture model streams must satisfy packed alignment"
    );
    ArenaAllocation {
        generation: gpu.generation,
        tint_identity: gpu.tint_identity,
        cube_range: Some(gpu.quad_range.clone()),
        cube_lighting_range: gpu.cube_lighting_range.clone(),
        model_range: gpu.model_range.clone(),
        model_lighting_range: gpu.model_lighting_range.clone(),
        model_draw_range: gpu.model_draw_range.clone(),
        transparent_model_draw_range: None,
        liquid_range: None,
        liquid_lighting_range: None,
        quad_capacity: 1,
        geometry_stream_range: Some(geometry..geometry + capacity),
        geometry_stream_capacity: capacity,
        biome_range: 0..1,
        biome_capacity: 1,
        gpu,
    }
}

fn region() -> Region {
    Region::Shadow(CascadeBounds {
        light_from_world: Mat4::IDENTITY,
        min: Vec3::splat(-32.0),
        max: Vec3::splat(128.0),
    })
}

#[test]
fn unchanged_geometry_and_cascade_reuse_storage_and_skip_command_uploads() {
    let (device, queue) = device();
    let mut arena = ChunkGpuArena::new(&device);
    arena
        .allocations
        .insert(Entity::from_raw_u32(0).unwrap(), allocation(0, 0));
    let mut scene = ResidentScene::default();
    let mut batch = GeometryBatch::default();
    assert!(scene.refresh(&arena, |_, _| true));
    assert!(batch.refresh(&scene, region()));
    assert_eq!(batch.cube_count, 1);
    assert_eq!(batch.commands.len(), 2);
    assert!(batch.upload_needed());
    batch.upload(&device, &queue);
    let buffer = batch.indirect.as_ref().unwrap().id();
    let addresses = (
        scene.entries.as_ptr(),
        batch.commands.as_ptr(),
        batch.selected.as_ptr(),
    );
    let revision = scene.revision;

    assert!(!scene.refresh(&arena, |_, _| true));
    assert!(!batch.refresh(&scene, region()));
    assert!(!batch.upload_needed());
    batch.upload(&device, &queue);
    assert_eq!(scene.revision, revision);
    assert_eq!(batch.indirect.as_ref().unwrap().id(), buffer);
    assert_eq!(
        addresses,
        (
            scene.entries.as_ptr(),
            batch.commands.as_ptr(),
            batch.selected.as_ptr()
        )
    );
}

#[test]
fn residency_removals_generations_and_publication_eligibility_invalidate_cached_commands() {
    let (device, _) = device();
    let entity = Entity::from_raw_u32(0).unwrap();
    let mut arena = ChunkGpuArena::new(&device);
    arena.allocations.insert(entity, allocation(0, 0));
    let mut scene = ResidentScene::default();
    let mut batch = GeometryBatch::default();
    scene.refresh(&arena, |_, _| true);
    batch.refresh(&scene, region());
    assert_eq!(batch.commands.len(), 2);

    assert!(scene.refresh(&arena, |_, _| false));
    batch.refresh(&scene, region());
    assert!(batch.commands.is_empty());
    assert!(scene.refresh(&arena, |_, _| true));
    batch.refresh(&scene, region());
    assert_eq!(batch.commands.len(), 2);

    arena
        .allocations
        .get_mut(&entity)
        .unwrap()
        .gpu
        .metadata_index = 7;
    arena.allocations.get_mut(&entity).unwrap().gpu.generation = 2;
    assert!(scene.refresh(&arena, |_, _| true));
    batch.refresh(&scene, region());
    assert!(
        batch
            .commands
            .iter()
            .all(|command| command.base_vertex == 28)
    );

    arena.allocations.remove(&entity);
    assert!(scene.refresh(&arena, |_, _| true));
    batch.refresh(&scene, region());
    assert!(batch.commands.is_empty());
}

#[test]
fn shadow_volume_includes_offscreen_casters_and_excludes_distant_residents() {
    let (device, _) = device();
    let mut arena = ChunkGpuArena::new(&device);
    for (index, x) in [0, 4, 40].into_iter().enumerate() {
        arena.allocations.insert(
            Entity::from_raw_u32(index as u32).unwrap(),
            allocation(x, index as u32),
        );
    }
    let mut scene = ResidentScene::default();
    scene.refresh(&arena, |_, _| true);
    let mut batch = GeometryBatch::default();
    let camera = Mat4::orthographic_rh(-8.0, 8.0, -8.0, 8.0, 0.1, 128.0)
        * Mat4::look_at_rh(
            Vec3::new(8.0, 8.0, -64.0),
            Vec3::new(8.0, 8.0, 0.0),
            Vec3::Y,
        );
    batch.refresh(&scene, Region::Camera(camera));
    assert_eq!(batch.selected.len(), 1);
    batch.refresh(&scene, region());
    assert_eq!(batch.selected.len(), 2);
    assert_eq!(batch.cube_count, 2);
    assert_eq!(batch.commands.len(), 4);
}

#[test]
fn matrix_updates_with_unchanged_selected_geometry_do_not_upload_again() {
    let (device, queue) = device();
    let mut arena = ChunkGpuArena::new(&device);
    arena
        .allocations
        .insert(Entity::from_raw_u32(0).unwrap(), allocation(0, 0));
    let mut scene = ResidentScene::default();
    let mut batch = GeometryBatch::default();
    scene.refresh(&arena, |_, _| true);
    batch.refresh(&scene, region());
    batch.upload(&device, &queue);
    let Region::Shadow(mut moved) = region() else {
        unreachable!();
    };
    moved.min.x += 0.01;
    assert!(batch.refresh(&scene, Region::Shadow(moved)));
    assert!(!batch.upload_needed());
}

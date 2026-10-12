use super::*;
use bevy::ecs::system::RunSystemOnce;

#[test]
fn animation_clock_updates_do_not_rebuild_or_reupload_texture_assets() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(RenderQueue::new(queue));
    world.init_resource::<ChunkTextureAssets>();
    world.init_resource::<ChunkTextureReload>();
    world.init_resource::<ChunkGpuTextureAssets>();
    world.init_resource::<ChunkTextureUploadStats>();
    world.run_system_once(init_chunk_gpu_arena).unwrap();
    world
        .run_system_once(init_chunk_gpu_animation_clock)
        .unwrap();
    world.run_system_once(prepare_chunk_texture_assets).unwrap();
    let identities = |world: &World| {
        let prepared = world
            .resource::<ChunkGpuTextureAssets>()
            .prepared
            .as_ref()
            .expect("diagnostic texture assets were uploaded");
        (
            prepared.identity,
            prepared.material_buffer.id(),
            prepared.animation_buffer.id(),
            prepared.animation_frame_buffer.id(),
            prepared.model_template_buffer.id(),
            prepared._textures.each_ref().map(|texture| texture.id()),
        )
    };
    let prepared = identities(&world);
    let uploaded = *world.resource::<ChunkTextureUploadStats>();
    assert_eq!(uploaded.upload_count, 1);
    assert!(uploaded.texture_bytes_including_mips > 0);
    let clock_buffer = world.resource::<ChunkGpuAnimationClock>().buffer.id();
    for frame in 0..120 {
        let clock = ChunkAnimationClock::from_elapsed_seconds(f64::from(frame) / 60.0);
        assert!((0.0..1.0).contains(&clock.partial_tick()));
        world.insert_resource(clock);
        world
            .run_system_once(prepare_chunk_animation_clock)
            .unwrap();
        world.run_system_once(prepare_chunk_texture_assets).unwrap();
        assert_eq!(identities(&world), prepared);
        assert_eq!(*world.resource::<ChunkTextureUploadStats>(), uploaded);
        assert_eq!(
            world.resource::<ChunkGpuAnimationClock>().buffer.id(),
            clock_buffer
        );
    }
}

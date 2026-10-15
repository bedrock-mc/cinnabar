use super::*;
use bevy::ecs::system::RunSystemOnce;

#[test]
fn obsolete_atlas_completion_cannot_stage_or_publish_replacement_geometry() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(RenderQueue::new(queue));
    world.init_resource::<ChunkTextureAssets>();
    world.init_resource::<ChunkGpuTextureAssets>();
    world.init_resource::<ChunkTextureUploadStats>();
    world.run_system_once(init_chunk_gpu_arena).unwrap();
    let reload = ChunkTextureReload::default();
    world.insert_resource(reload.clone());

    let candidate =
        ChunkTextureAssets::with_revision(Arc::new(assets::RuntimeAssets::diagnostic()), 1);
    reload.request(candidate.clone());
    let obsolete = reload.preparation().unwrap();
    let (prepared, uploaded) = build_chunk_texture_assets(
        &candidate,
        world.resource::<RenderDevice>(),
        world.resource::<RenderQueue>(),
    )
    .unwrap();
    reload.cancel_except(world.resource::<ChunkTextureAssets>().identity());
    reload.request_geometry(candidate.clone(), Arc::from([]));

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    world.resource_mut::<ChunkGpuTextureAssets>().pending = Some(PendingTextures {
        request: obsolete,
        result: std::sync::Mutex::new(receiver),
    });
    assert!(sender.send(Some((prepared, uploaded, None))).is_ok());
    world.run_system_once(prepare_chunk_texture_assets).unwrap();
    assert!(reload.status(candidate.identity()).is_none());
    assert!(world.resource::<ChunkGpuTextureAssets>().staged.is_none());

    test_time::eventually("the replacement atlas and geometry", || {
        world.run_system_once(prepare_chunk_texture_assets).unwrap();
        reload.status(candidate.identity()) == Some(Ok(()))
    });
    let staged = world
        .resource::<ChunkGpuTextureAssets>()
        .staged
        .as_ref()
        .unwrap();
    assert!(reload.is_current(&staged.request));
    assert!(staged.replacement.2.is_some());

    world.insert_resource(candidate.clone());
    world.run_system_once(prepare_chunk_texture_assets).unwrap();
    assert!(reload.preparation().is_none());
    assert_eq!(
        world
            .resource::<ChunkGpuTextureAssets>()
            .prepared
            .as_ref()
            .unwrap()
            .identity,
        candidate.identity()
    );
}

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
    reload.cancel_except(world.resource::<ChunkTextureAssets>().binding_identity());
    reload.request_geometry(reload.geometry_epoch(), candidate.clone(), Arc::from([]));

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    world.resource_mut::<ChunkGpuTextureAssets>().pending = Some(PendingTextures {
        request: obsolete,
        result: std::sync::Mutex::new(receiver),
    });
    assert!(sender.send(Some((prepared, uploaded, None))).is_ok());
    world.run_system_once(prepare_chunk_texture_assets).unwrap();
    assert!(reload.status(candidate.binding_identity()).is_none());
    assert!(world.resource::<ChunkGpuTextureAssets>().staged.is_none());

    test_time::eventually("the replacement atlas and geometry", || {
        world.run_system_once(prepare_chunk_texture_assets).unwrap();
        reload.status(candidate.binding_identity()) == Some(Ok(()))
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
        candidate.binding_identity()
    );
}

#[test]
fn acknowledged_snapshot_publishes_without_clearing_a_newer_request() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let candidate =
        ChunkTextureAssets::with_revision(Arc::new(assets::RuntimeAssets::diagnostic()), 1);
    let reload = ChunkTextureReload::default();
    reload.request_geometry(reload.geometry_epoch(), candidate.clone(), Arc::from([]));
    let acknowledged = reload.preparation().unwrap();
    let (prepared, uploaded) = build_chunk_texture_assets(&candidate, &device, &queue).unwrap();
    let material_buffer = prepared.material_buffer.id();
    let geometry = PreparedResourceGeometry::build(
        &[],
        candidate.clone(),
        device.clone(),
        queue.clone(),
        None,
    )
    .unwrap();
    assert!(reload.finish(&acknowledged, true));
    let newer = ChunkTextureAssets::with_revision(Arc::clone(candidate.assets()), 2);
    reload.request_geometry(reload.geometry_epoch(), newer.clone(), Arc::from([]));
    let pending = reload.preparation().unwrap();

    let mut world = World::new();
    world.insert_resource(device);
    world.insert_resource(queue);
    world.insert_resource(candidate.clone());
    world.insert_resource(reload.clone());
    world.insert_resource(ChunkGpuTextureAssets {
        staged: Some(StagedTextures {
            request: acknowledged,
            replacement: (prepared, uploaded, Some(geometry)),
        }),
        ..Default::default()
    });
    world.init_resource::<ChunkTextureUploadStats>();
    world.run_system_once(init_chunk_gpu_arena).unwrap();
    world.run_system_once(prepare_chunk_texture_assets).unwrap();
    assert_eq!(
        world
            .resource::<ChunkGpuTextureAssets>()
            .prepared
            .as_ref()
            .unwrap()
            .material_buffer
            .id(),
        material_buffer,
        "the CPU's acknowledged snapshot must publish its staged GPU resources"
    );
    assert!(reload.is_current(&pending));
    assert!(reload.status(newer.binding_identity()).is_none());
    test_time::eventually("the newer preparation worker", || {
        world.run_system_once(prepare_chunk_texture_assets).unwrap();
        reload.status(newer.binding_identity()) == Some(Ok(()))
    });
}

/// An unfinished old worker must never hold up preparation in a replacement world.
#[test]
fn stale_open_gpu_worker_never_blocks_the_new_generation() {
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
    reload.request_geometry(reload.geometry_epoch(), candidate.clone(), Arc::from([]));
    let obsolete = reload.preparation().unwrap();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    world.resource_mut::<ChunkGpuTextureAssets>().pending = Some(PendingTextures {
        request: obsolete,
        result: std::sync::Mutex::new(receiver),
    });
    reload.discard_geometry();
    reload.request_geometry(reload.geometry_epoch(), candidate.clone(), Arc::from([]));
    world.run_system_once(prepare_chunk_texture_assets).unwrap();
    assert!(
        sender.send(None).is_err(),
        "the stale receiver must be dropped before polling"
    );
    test_time::eventually("the new generation GPU snapshot", || {
        world.run_system_once(prepare_chunk_texture_assets).unwrap();
        reload.status(candidate.binding_identity()) == Some(Ok(()))
    });
}

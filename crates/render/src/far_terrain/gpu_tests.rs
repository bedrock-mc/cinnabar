use super::*;
use render_scene::runtime_profile_uploads::measure;

/// Creates a validation GPU without requiring a hardware adapter.
fn device() -> (RenderDevice, RenderQueue) {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    (RenderDevice::from(device), RenderQueue::new(queue))
}

/// Supplies distinct immutable tiles with predictable upload sizes.
fn tiles(count: usize) -> Vec<Arc<TileMesh>> {
    (0..count)
        .map(|index| {
            Arc::new(TileMesh {
                coverage: far_terrain::TileCoverage::Complete,
                key: TileKey {
                    level: 0,
                    x: index as i32,
                    z: 0,
                },
                revision: index as u64 + 1,
                quads: vec![[index as u32, 64]; 1].into_boxed_slice(),
                colours: vec![index as u8; TILE_CELLS * TILE_CELLS * 4].into_boxed_slice(),
                min_y: 0,
                max_y: 64,
            })
        })
        .collect()
}

/// Completes bounded preparation before testing a capacity crossing.
fn populate(
    gpu: &mut FarTerrainGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    tiles: &[Arc<TileMesh>],
) {
    for _ in 0..=tiles.len() {
        gpu.apply_tiles(device, queue, tiles);
        if gpu.resident.len() == tiles.len() {
            return;
        }
    }
    panic!("far tile preparation did not converge");
}

#[test]
fn quad_growth_does_not_reupload_resident_tiles() {
    let (device, queue) = device();
    let mut gpu = FarTerrainGpu::new(&device);
    let tiles = tiles(256);
    populate(&mut gpu, &device, &queue, &tiles);
    let records = gpu.records.id();
    let colours = gpu.colours.id();
    let quads = gpu.quads.id();
    let (_, uploads) = measure(|| gpu.allocate_quads(&device, &queue, 1).unwrap());
    eprintln!("quad growth: writes={}, bytes={}", uploads.0, uploads.1);
    assert_ne!(gpu.quads.id(), quads);
    assert_eq!(gpu.records.id(), records);
    assert_eq!(gpu.colours.id(), colours);
    assert_eq!(uploads, (0, 0));
}

#[test]
fn slot_growth_does_not_reupload_resident_tiles() {
    let (device, queue) = device();
    let mut gpu = FarTerrainGpu::new(&device);
    let tiles = tiles(128);
    populate(&mut gpu, &device, &queue, &tiles);
    let quads = gpu.quads.id();
    let records = gpu.records.id();
    let colours = gpu.colours.id();
    let (_, uploads) = measure(|| gpu.allocate_slot(&device, &queue).unwrap());
    eprintln!("slot growth: writes={}, bytes={}", uploads.0, uploads.1);
    assert_eq!(gpu.quads.id(), quads);
    assert_ne!(gpu.records.id(), records);
    assert_ne!(gpu.colours.id(), colours);
    assert_eq!(uploads, (0, 0));
}

#[test]
fn a_burst_reserves_once_and_continues_on_unchanged_frames() {
    use bevy::{ecs::system::RunSystemOnce, prelude::World};
    let (device, queue) = device();
    let mut world = World::new();
    world.insert_resource(FarTerrainGpu::new(&device));
    world.insert_resource(device);
    world.insert_resource(queue);
    let tiles: Arc<[Arc<TileMesh>]> = tiles(UPLOAD_TILES_PER_FRAME * 3 + 1).into();
    world.insert_resource(FarTerrainScene {
        tiles: tiles.clone(),
        ..Default::default()
    });
    world.run_system_once(prepare_far_terrain).unwrap();
    let gpu = world.resource::<FarTerrainGpu>();
    assert_eq!(gpu.resident.len(), UPLOAD_TILES_PER_FRAME);
    assert!(gpu.slots as usize >= tiles.len());
    assert!(gpu.quad_capacity as usize >= tiles.len());
    assert!(gpu.applied_tiles.is_none());
    let resources = (gpu.quads.id(), gpu.records.id(), gpu.colours.id());
    for _ in 0..3 {
        world.run_system_once(prepare_far_terrain).unwrap();
    }
    let gpu = world.resource::<FarTerrainGpu>();
    assert_eq!(gpu.resident.len(), tiles.len());
    assert!(Arc::ptr_eq(gpu.applied_tiles.as_ref().unwrap(), &tiles));
    assert_eq!(
        (gpu.quads.id(), gpu.records.id(), gpu.colours.id()),
        resources
    );
    let (_, writes) = measure(|| world.run_system_once(prepare_far_terrain).unwrap());
    assert_eq!(writes, (0, 0));
}

/// An empty scene (far terrain off, or a shader pack drawing the world) gives back the grown
/// quads, records and colour layers.
#[test]
fn an_empty_scene_releases_grown_storage() {
    use bevy::{ecs::system::RunSystemOnce, prelude::World};
    let (device, queue) = device();
    let mut world = World::new();
    world.insert_resource(FarTerrainGpu::new(&device));
    world.insert_resource(device);
    world.insert_resource(queue);
    let tiles: Arc<[Arc<TileMesh>]> = tiles(UPLOAD_TILES_PER_FRAME).into();
    world.insert_resource(FarTerrainScene {
        tiles,
        ..Default::default()
    });
    world.run_system_once(prepare_far_terrain).unwrap();
    let gpu = world.resource::<FarTerrainGpu>();
    assert!(gpu.slots > 1 && gpu.quad_capacity > 1);
    world.insert_resource(FarTerrainScene::default());
    world.run_system_once(prepare_far_terrain).unwrap();
    let gpu = world.resource::<FarTerrainGpu>();
    assert!(gpu.resident.is_empty() && gpu.draws.is_empty());
    assert_eq!((gpu.slots, gpu.quad_capacity), (1, 1));
    assert_eq!(
        (
            gpu.quads.size(),
            gpu.records.size(),
            gpu.colours.depth_or_array_layers()
        ),
        (QUAD_BYTES, TILE_RECORD_BYTES, 1)
    );
}

#[test]
fn upload_bytes_are_bounded_and_an_oversized_first_tile_can_progress() {
    let (device, queue) = device();
    let mut gpu = FarTerrainGpu::new(&device);
    let mut tiles = tiles(3);
    for mesh in &mut tiles {
        Arc::get_mut(mesh).unwrap().quads =
            vec![[0; 2]; (UPLOAD_BYTES_PER_FRAME / QUAD_BYTES / 2) as usize].into_boxed_slice();
    }
    let (complete, (_, bytes)) = measure(|| gpu.apply_tiles(&device, &queue, &tiles));
    assert!(!complete);
    assert_eq!(gpu.resident.len(), 1);
    assert!(bytes <= UPLOAD_BYTES_PER_FRAME);
    Arc::get_mut(&mut tiles[1]).unwrap().quads =
        vec![[0; 2]; (UPLOAD_BYTES_PER_FRAME / QUAD_BYTES + 1) as usize].into_boxed_slice();
    let (complete, (_, bytes)) = measure(|| gpu.apply_tiles(&device, &queue, &tiles));
    assert!(!complete);
    assert_eq!(gpu.resident.len(), 2);
    assert!(bytes > UPLOAD_BYTES_PER_FRAME);
    assert!(gpu.apply_tiles(&device, &queue, &tiles));
}

#[test]
fn deferred_revisions_keep_the_uploaded_geometry_and_bounds() {
    let (device, queue) = device();
    let mut gpu = FarTerrainGpu::new(&device);
    let original = tiles(UPLOAD_TILES_PER_FRAME + 1);
    populate(&mut gpu, &device, &queue, &original);
    let mut changed = tiles(original.len());
    for mesh in &mut changed {
        let mesh = Arc::get_mut(mesh).unwrap();
        mesh.revision += 100;
        mesh.min_y = -64;
        mesh.max_y = 128;
    }
    assert!(!gpu.apply_tiles(&device, &queue, &changed));
    assert_eq!(gpu.draws.len(), original.len());
    let last = original.last().unwrap();
    assert!(Arc::ptr_eq(&gpu.resident[&last.key].mesh, last));
    assert_eq!(gpu.draws.last().unwrap().bounds, tile_bounds(last));
    assert!(gpu.apply_tiles(&device, &queue, &changed));
    assert!(Arc::ptr_eq(
        &gpu.resident[&last.key].mesh,
        changed.last().unwrap()
    ));
}

#[test]
fn a_new_scene_discards_obsolete_pending_tiles() {
    let (device, queue) = device();
    let mut gpu = FarTerrainGpu::new(&device);
    let original = tiles(UPLOAD_TILES_PER_FRAME + 1);
    assert!(!gpu.apply_tiles(&device, &queue, &original));
    assert!(gpu.apply_tiles(&device, &queue, &original[..1]));
    assert_eq!(gpu.resident.len(), 1);
    assert!(gpu.resident.contains_key(&original[0].key));
    assert_eq!(gpu.draws.len(), 1);
}

#[test]
fn returning_to_an_earlier_scene_restores_partially_replaced_tiles() {
    use bevy::{ecs::system::RunSystemOnce, prelude::World};
    let (device, queue) = device();
    let mut gpu = FarTerrainGpu::new(&device);
    let original: Arc<[Arc<TileMesh>]> = tiles(UPLOAD_TILES_PER_FRAME + 1).into();
    populate(&mut gpu, &device, &queue, &original);
    gpu.applied_tiles = Some(original.clone());
    let mut changed = tiles(original.len());
    for mesh in &mut changed {
        Arc::get_mut(mesh).unwrap().revision += 100;
    }
    let mut world = World::new();
    world.insert_resource(gpu);
    world.insert_resource(device);
    world.insert_resource(queue);
    world.insert_resource(FarTerrainScene {
        tiles: changed.into(),
        ..Default::default()
    });
    world.run_system_once(prepare_far_terrain).unwrap();
    assert!(world.resource::<FarTerrainGpu>().applied_tiles.is_none());
    world.resource_mut::<FarTerrainScene>().tiles = original.clone();
    for _ in 0..2 {
        world.run_system_once(prepare_far_terrain).unwrap();
    }
    let gpu = world.resource::<FarTerrainGpu>();
    for mesh in &*original {
        assert!(Arc::ptr_eq(&gpu.resident[&mesh.key].mesh, mesh));
    }
}

#[test]
fn finer_tiles_evict_coarser_residents_at_the_device_limit() {
    let (device, queue) = device();
    let mut gpu = FarTerrainGpu::new(&device);
    let limit = device.limits().max_texture_array_layers as usize;
    let mut coarse = tiles(limit);
    for mesh in &mut coarse {
        Arc::get_mut(mesh).unwrap().key.level = 2;
    }
    populate(&mut gpu, &device, &queue, &coarse);
    let mut desired = coarse.clone();
    desired.push(tiles(1).pop().unwrap());
    assert!(gpu.apply_tiles(&device, &queue, &desired));
    assert_eq!(gpu.resident.len(), limit);
    assert!(gpu.resident.contains_key(&desired.last().unwrap().key));
    assert!(!gpu.resident.contains_key(&coarse.last().unwrap().key));
}

/// Requests a headless adapter for checking real copies rather than no-op resource identities.
fn copy_device() -> Option<(RenderDevice, RenderQueue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: headless GPU adapter for far-terrain copy readback");
        return None;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&Default::default()))
        .expect("far-terrain copy device");
    Some((RenderDevice::from(device), RenderQueue::new(queue)))
}

#[test]
fn growth_preserves_gpu_bytes_and_orders_replacement_writes_after_copies() {
    let Some((device, queue)) = copy_device() else {
        return;
    };
    let mut gpu = FarTerrainGpu::new(&device);
    let mut desired = tiles(3);
    desired.remove(0);
    assert!(gpu.apply_tiles(&device, &queue, &desired[..1]));
    // The first tile's writes are still queued when both resources grow.
    assert!(gpu.apply_tiles(&device, &queue, &desired));
    assert_gpu_bytes(&gpu, &device, &queue);

    let first = &desired[0];
    desired[0] = Arc::new(TileMesh {
        coverage: far_terrain::TileCoverage::Complete,
        key: first.key,
        revision: first.revision + 100,
        quads: vec![[99, 88]; 3].into_boxed_slice(),
        colours: vec![77; first.colours.len()].into_boxed_slice(),
        min_y: 0,
        max_y: 88,
    });
    desired.push(tiles(4).pop().unwrap());
    // Slot and quad growth now precede a replacement upload in the same batch.
    assert!(gpu.apply_tiles(&device, &queue, &desired));
    assert_gpu_bytes(&gpu, &device, &queue);
}

/// Reads every resident payload back through copies and compares it with the uploaded revision.
fn assert_gpu_bytes(gpu: &FarTerrainGpu, device: &RenderDevice, queue: &RenderQueue) {
    let records_offset = gpu.quads.size();
    let colours_offset = (records_offset + gpu.records.size())
        .next_multiple_of(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT));
    let colour_bytes = (TILE_CELLS * TILE_CELLS * 4) as u64;
    let readback = device.create_buffer(&BufferDescriptor {
        label: Some("far terrain copy readback"),
        size: colours_offset + u64::from(gpu.slots) * colour_bytes,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(&gpu.quads, 0, &readback, 0, gpu.quads.size());
    encoder.copy_buffer_to_buffer(
        &gpu.records,
        0,
        &readback,
        records_offset,
        gpu.records.size(),
    );
    encoder.copy_texture_to_buffer(
        gpu.colours.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: TexelCopyBufferLayout {
                offset: colours_offset,
                bytes_per_row: Some(TILE_CELLS as u32 * 4),
                rows_per_image: Some(TILE_CELLS as u32),
            },
        },
        gpu.colours.size(),
    );
    queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receive.recv().unwrap().unwrap();
    let data = readback.slice(..).get_mapped_range().unwrap();
    for resident in gpu.resident.values() {
        let mesh = &resident.mesh;
        let offset = resident.quads.start as usize * QUAD_BYTES as usize;
        assert_eq!(
            &data[offset..offset + mesh.quads.len() * QUAD_BYTES as usize],
            bytemuck::cast_slice::<_, u8>(&mesh.quads)
        );
        let offset = (records_offset + u64::from(resident.slot) * TILE_RECORD_BYTES) as usize;
        let [x, z] = mesh.key.origin();
        let record = [x as u32, z as u32, u32::from(mesh.key.level), 0];
        assert_eq!(
            &data[offset..offset + TILE_RECORD_BYTES as usize],
            bytemuck::cast_slice::<_, u8>(&record)
        );
        let offset = (colours_offset + u64::from(resident.slot) * colour_bytes) as usize;
        assert_eq!(&data[offset..offset + mesh.colours.len()], &*mesh.colours);
    }
    drop(data);
    readback.unmap();
}

/// A view that drew before far terrain was released drops the bind group holding the old storage.
#[test]
fn releasing_a_drawn_view_drops_its_bind_group() {
    let (device, _) = device();
    let gpu = FarTerrainGpu::new(&device);
    let layout = device.create_bind_group_layout("far terrain test layout", &[]);
    let key = BindGroupKey {
        view_uniforms: gpu.mask.id(),
        quads: gpu.quads.id(),
        records: gpu.records.id(),
        colours: gpu.colours_view.id(),
        mask: gpu.mask.id(),
    };
    let mut view = FarTerrainView {
        pipeline: CachedRenderPipelineId::INVALID,
        visible: Vec::new(),
        bind_group: Some((
            key,
            device.create_bind_group("far terrain test", &layout, &[]),
        )),
    };
    view.release();
    assert!(view.bind_group.is_none() && view.visible.is_empty());
}

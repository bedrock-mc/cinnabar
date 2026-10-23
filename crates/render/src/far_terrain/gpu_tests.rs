use super::*;

impl FarTerrainGpu {
    /// Reads each resident tile's GPU ranges and the uploaded mask in a stable key order.
    pub(crate) fn publication_snapshot(
        &self,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> Vec<Vec<u8>> {
        let read = crate::render_test_support::read_buffer;
        let mut residents = self.resident.iter().collect::<Vec<_>>();
        residents.sort_unstable_by_key(|(key, _)| **key);
        let mut bytes = Vec::new();
        for (_, resident) in residents {
            bytes.push(read(
                device,
                queue,
                &self.quads,
                u64::from(resident.quads.start) * QUAD_BYTES,
                resident.quads.len() as u64 * QUAD_BYTES,
            ));
            bytes.push(read(
                device,
                queue,
                &self.records,
                u64::from(resident.slot) * TILE_RECORD_BYTES,
                TILE_RECORD_BYTES,
            ));
            bytes.push(crate::render_test_support::read_rgba8_layer(
                device,
                queue,
                &self.colours,
                resident.slot,
            ));
        }
        bytes.push(read(device, queue, &self.mask, 0, self.mask_words * 4));
        bytes
    }
}

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

/// Failed preallocation preserves draws and eviction, and waits for a release before retrying.
#[test]
fn failed_preallocation_keeps_draws_and_backs_off_until_tiles_are_released() {
    for site in ["far terrain tile slots", "far terrain quads"] {
        let (device, queue) = device();
        let failures = crate::gpu_allocation::FailureFlag::new(&device);
        let mut gpu = FarTerrainGpu::new(&device);
        let mut coarse = tiles(4);
        for mesh in &mut coarse {
            Arc::get_mut(mesh).unwrap().key.level = 2;
        }
        populate(&mut gpu, &device, &queue, &coarse);
        let quads = gpu.quads.id();
        let slots = (gpu.records.id(), gpu.colours.id());
        let fine = tiles(1).pop().unwrap();
        let mut desired = coarse.clone();
        desired.push(Arc::clone(&fine));
        let attempts = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = attempts.clone();
        let slot_capacity = gpu.slots;
        let quad_capacity = gpu.quad_capacity;
        let _failure = crate::gpu_allocation::inject(move |label, bytes| {
            let oversized = label == site
                && match label {
                    "far terrain tile slots" => {
                        bytes > u64::from(slot_capacity) * (TILE_RECORD_BYTES + TILE_LAYER_BYTES)
                    }
                    "far terrain quads" => bytes > u64::from(quad_capacity) * QUAD_BYTES,
                    _ => false,
                };
            if oversized {
                observed.set(observed.get() + 1);
            }
            oversized
        });
        gpu.apply_tiles(&device, &queue, &desired);
        assert!(!failures.failed(), "optional growth invalidated terrain");
        assert!(
            gpu.current(&fine),
            "retained slots must still admit finer tiles"
        );
        assert_eq!(gpu.draws.len(), coarse.len());
        assert_eq!(gpu.quads.id(), quads);
        if site == "far terrain tile slots" {
            assert_eq!((gpu.records.id(), gpu.colours.id()), slots);
        }
        let failed_attempts = attempts.get();
        assert!(failed_attempts > 0);
        for _ in 0..3 {
            gpu.apply_tiles(&device, &queue, &desired);
        }
        assert_eq!(
            attempts.get(),
            failed_attempts,
            "growth retried without a release"
        );
        gpu.apply_tiles(&device, &queue, &[fine]);
        gpu.apply_tiles(&device, &queue, &desired);
        assert!(
            attempts.get() > failed_attempts,
            "a release must permit another attempt"
        );
        assert!(!failures.failed());
    }
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

/// Quad, slot and mask growth that runs out of memory keeps the old storage and retries.
#[test]
fn growth_without_memory_keeps_storage_and_retries() {
    let (device, queue) = device();
    let mut gpu = FarTerrainGpu::new(&device);
    let tiles = tiles(4);
    populate(&mut gpu, &device, &queue, &tiles);
    let (quads, records, colours) = (gpu.quads.id(), gpu.records.id(), gpu.colours.id());
    let (quad_capacity, slots) = (gpu.quad_capacity, gpu.slots);
    {
        let _failure = crate::gpu_allocation::inject(|label, _| label.starts_with("far terrain"));
        assert!(!gpu.grow_quads(&device, &queue, quad_capacity + 1, Requirement::Required));
        gpu.grow_slots(&device, &queue, slots + 1);
        let mask = NearMask::around([0, 0], 32, 8, []);
        assert!(gpu.apply_mask(&device, &queue, &mask).is_err());
    }
    assert_eq!(
        (gpu.quads.id(), gpu.records.id(), gpu.colours.id()),
        (quads, records, colours)
    );
    assert_eq!((gpu.quad_capacity, gpu.slots), (quad_capacity, slots));
    assert_eq!(gpu.mask_words, MASK_HEADER_WORDS as u64);
    assert!(gpu.grow_quads(&device, &queue, quad_capacity + 1, Requirement::Required));
    assert!(gpu.quad_capacity > quad_capacity);
    gpu.invalidate_publication();
    gpu.grow_slots(&device, &queue, slots + 1);
    assert!(gpu.slots > slots);
}

/// Supplies a complete tile large enough to cross a selected quad-buffer capacity.
fn tile_with_quads(x: i32, quads: usize) -> Arc<TileMesh> {
    Arc::new(TileMesh {
        coverage: far_terrain::TileCoverage::Complete,
        key: TileKey { level: 0, x, z: 0 },
        revision: 1,
        quads: vec![[0, 64]; quads].into_boxed_slice(),
        colours: vec![0; TILE_CELLS * TILE_CELLS * 4].into_boxed_slice(),
        min_y: 0,
        max_y: 64,
    })
}

/// Making room for a finer tile must not retire residents or hand out a range the quad
/// buffer cannot hold when it cannot grow; uploading past it would be a validation error.
#[test]
fn eviction_without_memory_to_grow_keeps_every_resident() {
    let (device, queue) = device();
    let mut gpu = FarTerrainGpu::new(&device);
    let old = tiles(4);
    populate(&mut gpu, &device, &queue, &old);
    let quad_capacity = gpu.quad_capacity;
    let mut scene = vec![tile_with_quads(-1, quad_capacity as usize * 2)];
    scene.extend(old.iter().cloned());
    {
        let _failure = crate::gpu_allocation::inject(|label, _| label.starts_with("far terrain"));
        gpu.apply_tiles(&device, &queue, &scene);
    }
    assert_eq!(gpu.quad_capacity, quad_capacity);
    assert!(old.iter().all(|mesh| gpu.resident.contains_key(&mesh.key)));
    assert!(!gpu.resident.contains_key(&scene[0].key));
    for _ in 0..=scene.len() {
        gpu.apply_tiles(&device, &queue, &scene);
    }
    assert!(gpu.resident.contains_key(&scene[0].key));
}

/// A failed fresh allocation invalidates terrain and leaves valid storage for a full retry.
#[test]
fn a_failed_tile_allocation_invalidates_terrain_for_a_full_retry() {
    let (device, queue) = device();
    let failures = crate::gpu_allocation::FailureFlag::new(&device);
    let mut gpu = FarTerrainGpu::new(&device);
    let scene = [tile_with_quads(0, 4)];
    {
        let _failure = crate::gpu_allocation::inject(|label, _| label.starts_with("far terrain"));
        gpu.apply_tiles(&device, &queue, &scene);
    }
    assert!(failures.failed());
    assert!(gpu.resident.is_empty());
    assert!(failures.take());
    gpu.invalidate_publication();
    assert!(gpu.apply_tiles(&device, &queue, &scene));
    assert!(gpu.resident.contains_key(&scene[0].key));
}

#[test]
fn failed_tile_uploads_return_slots_and_quads_during_backoff() {
    for previous_quads in [None, Some(4), Some(1)] {
        let (device, queue) = device();
        let failures = crate::gpu_allocation::FailureFlag::new(&device);
        let mut gpu = FarTerrainGpu::new(&device);
        if let Some(count) = previous_quads {
            populate(&mut gpu, &device, &queue, &[tile_with_quads(0, count)]);
        }
        let mut mesh = tile_with_quads(0, if previous_quads == Some(1) { 4 } else { 1 });
        Arc::get_mut(&mut mesh).unwrap().revision += 1;
        let scene = [mesh];
        let _failure = crate::gpu_allocation::inject(|label, _| label == "far terrain tile upload");
        assert!(!gpu.apply_tiles(&device, &queue, &scene));
        assert!(failures.failed());
        let storage = (gpu.quads.id(), gpu.records.id(), gpu.colours.id());
        for _ in 0..6 {
            assert!(gpu.resident.is_empty());
            assert_eq!(gpu.ranges.end(), 0, "failed upload still owns quads");
            let free: HashSet<_> = gpu.free_slots.iter().copied().collect();
            assert_eq!(free.len(), gpu.slots as usize);
            assert_eq!(free.len(), gpu.free_slots.len(), "slot returned twice");
            assert!(!gpu.apply_tiles(&device, &queue, &scene));
            assert_eq!(
                (gpu.quads.id(), gpu.records.id(), gpu.colours.id()),
                storage
            );
        }
    }
}

/// A mask the device has no memory for is not acknowledged, so publication retries it.
#[test]
fn a_mask_without_memory_is_not_acknowledged() {
    let (device, queue) = device();
    let mut gpu = FarTerrainGpu::new(&device);
    let mask = Arc::new(NearMask::around([0, 0], 32, 8, []));
    {
        let _failure = crate::gpu_allocation::inject(|label, _| label.starts_with("far terrain"));
        assert!(!gpu.publish_mask(&device, &queue, Arc::clone(&mask)));
    }
    assert!(gpu.applied_mask.is_none());
    assert!(gpu.publish_mask(&device, &queue, Arc::clone(&mask)));
    assert!(gpu.applied_mask.is_some());
}

/// A fine retained fallback, listed after the selected tiles, still outranks coarse ones.
#[test]
fn finer_tiles_rank_first_wherever_they_are_listed() {
    let key = |level, x| TileKey { level, x, z: 0 };
    let order: Vec<usize> = upload_order([key(0, 0), key(2, 1), key(0, 5)].into_iter())
        .into_iter()
        .map(|(index, _)| index)
        .collect();
    assert_eq!(order, [0, 2, 1]);
}

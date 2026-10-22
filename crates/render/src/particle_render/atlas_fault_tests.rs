use super::*;
use bevy::{ecs::system::RunSystemOnce, render::render_resource::PollType};
use particles::AtlasPatch;

/// Publishes a new atlas base and runs one preparation.
fn publish(world: &mut World, side: u32, value: u8) {
    world.insert_resource(ParticleGpuFrame {
        base: Some(vec![value; (side * side * 4) as usize].into()),
        atlas_side: side,
        ..Default::default()
    });
    world.run_system_once(prepare_particle_resources).unwrap();
}

// A failed replacement drops the stale atlas, keeps running, and retries once it retires.
#[test]
fn particle_atlas_out_of_memory_releases_the_stale_atlas_and_retries_after_it_retires() {
    let Some(mut app) = crate::upload_allocation_tests::app(ParticleRenderPlugin) else {
        return;
    };
    let world = app.sub_app_mut(RenderApp).world_mut();
    let side = 2048;
    publish(world, side, 1);
    assert!(world.resource::<ParticleGpu>().texture.is_some());
    {
        let _fault = crate::gpu_allocation::inject(|label, _| label == "particle atlas");
        publish(world, side, 2);
        let gpu = world.resource::<ParticleGpu>();
        assert!(gpu.texture.is_none() && gpu.texture_view.is_none());
        assert!(
            gpu.bind_group.is_none(),
            "the old atlas has the old placements"
        );
        assert!(gpu.atlas.retry.failed());
    }
    // The frame poll runs completion callbacks once the released atlas retires.
    world
        .resource::<RenderDevice>()
        .poll(PollType::wait_indefinitely())
        .unwrap();
    world.run_system_once(prepare_particle_resources).unwrap();
    let gpu = world.resource::<ParticleGpu>();
    assert_eq!(
        gpu.texture.as_ref().map(|texture| texture.width()),
        Some(side)
    );
    assert!(!gpu.atlas.retry.failed());
}

// Without a previous atlas to release, an atlas that cannot fit is reduced, tiles included.
#[test]
fn a_particle_atlas_that_cannot_fit_is_reduced_instead_of_dropped() {
    let Some(mut app) = crate::upload_allocation_tests::app(ParticleRenderPlugin) else {
        return;
    };
    let world = app.sub_app_mut(RenderApp).world_mut();
    let side = 2048;
    let full = u64::from(side * side * 4);
    let _fault = crate::gpu_allocation::inject(move |label, bytes| {
        label == "particle atlas" && bytes == full
    });
    publish(world, side, 3);
    assert_eq!(
        world
            .resource::<ParticleGpu>()
            .texture
            .as_ref()
            .map(|texture| texture.width()),
        Some(side / 2)
    );
    {
        let mut frame = world.resource_mut::<ParticleGpuFrame>();
        frame.patches = Arc::from([AtlasPatch {
            seq: 1,
            x: side - particles::TILE_SLOT,
            y: side - particles::TILE_SLOT,
            width: particles::TILE_SLOT,
            height: particles::TILE_SLOT,
            rgba8: vec![9; (particles::TILE_SLOT * particles::TILE_SLOT * 4) as usize].into(),
        }]);
        frame.patch_seq = 1;
    }
    // An unreduced tile at the full-size origin would fail wgpu's copy validation here.
    world.run_system_once(prepare_particle_resources).unwrap();
    assert_eq!(world.resource::<ParticleGpu>().uploaded_seq, 1);
}

// Real out-of-memory leaves an invalid texture, so a failed creation must not be written or viewed.
#[test]
fn a_failed_particle_atlas_creation_is_never_written_and_recovers_after_backoff() {
    let Some(mut app) = crate::upload_allocation_tests::app(ParticleRenderPlugin) else {
        return;
    };
    let world = app.sub_app_mut(RenderApp).world_mut();
    let side = 2048;
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    {
        let seen = std::rc::Rc::clone(&seen);
        let _fault = crate::gpu_allocation::inject(move |label, _| {
            seen.borrow_mut().push(label.to_owned());
            label == "particle atlas"
        });
        publish(world, side, 4);
    }
    let seen = seen.borrow();
    assert!(seen.iter().any(|label| label == "particle atlas"));
    assert!(
        seen.iter().all(|label| label == "particle atlas"),
        "{seen:?}"
    );
    assert!(world.resource::<ParticleGpu>().texture.is_none());
    // Nothing was released, so recovery waits for the backoff rather than a retirement.
    for _ in 1..crate::gpu_allocation::MIN_RETRY_FRAMES {
        world.run_system_once(prepare_particle_resources).unwrap();
        assert!(world.resource::<ParticleGpu>().texture.is_none());
    }
    world.run_system_once(prepare_particle_resources).unwrap();
    assert_eq!(
        world
            .resource::<ParticleGpu>()
            .texture
            .as_ref()
            .map(|texture| texture.width()),
        Some(side)
    );
}

/// Copies the uploaded atlas back to the CPU.
fn read_atlas(world: &World) -> Vec<u8> {
    let device = world.resource::<RenderDevice>();
    let queue = world.resource::<RenderQueue>();
    let texture = world.resource::<ParticleGpu>().texture.clone().unwrap();
    let (width, height) = (texture.width(), texture.height());
    let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&BufferDescriptor {
        label: Some("particle atlas readback"),
        size: u64::from(row * height),
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device.poll(PollType::wait_indefinitely()).unwrap();
    let mapped = buffer.slice(..).get_mapped_range();
    mapped
        .chunks(row as usize)
        .flat_map(|line| &line[..(width * 4) as usize])
        .copied()
        .collect()
}

/// One dynamic tile of a single value at `x`, on the first dynamic row of a 256 atlas.
fn tile(seq: u64, x: u32, value: u8) -> AtlasPatch {
    let side = particles::TILE_SLOT;
    AtlasPatch {
        seq,
        x,
        y: 256 - side,
        width: side,
        height: side,
        rgba8: vec![value; (side * side * 4) as usize].into(),
    }
}

// A swallowed out-of-memory error during an incremental tile write re-uploads the whole atlas.
#[test]
fn a_swallowed_out_of_memory_tile_write_is_followed_by_a_full_reupload() {
    let Some(mut app) = crate::upload_allocation_tests::app(ParticleRenderPlugin) else {
        return;
    };
    let world = app.sub_app_mut(RenderApp).world_mut();
    let side = 256;
    publish(world, side, 1);
    let tiles = [tile(1, 0, 7), tile(2, particles::TILE_SLOT, 9)];
    {
        let mut frame = world.resource_mut::<ParticleGpuFrame>();
        frame.patches = Arc::from(&tiles[..1]);
        frame.patch_seq = 1;
    }
    world.run_system_once(prepare_particle_resources).unwrap();
    {
        let mut frame = world.resource_mut::<ParticleGpuFrame>();
        frame.patches = Arc::from(&tiles[..]);
        frame.patch_seq = 2;
    }
    // The second tile's write was acknowledged, but its staging ran out of memory.
    world.resource_mut::<ParticleGpu>().uploaded_seq = 2;
    crate::gpu_allocation::record_memory_loss();
    world.run_system_once(prepare_particle_resources).unwrap();
    assert_eq!(world.resource::<ParticleGpu>().uploaded_seq, 2);
    let mut expected = vec![1; (side * side * 4) as usize];
    for tile in &tiles {
        for row in 0..tile.height {
            let at = (((tile.y + row) * side + tile.x) * 4) as usize;
            expected[at..at + (tile.width * 4) as usize].fill(tile.rgba8[0]);
        }
    }
    assert!(
        read_atlas(world) == expected,
        "the atlas must match its CPU content"
    );
}

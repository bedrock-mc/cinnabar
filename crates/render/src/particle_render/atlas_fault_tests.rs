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

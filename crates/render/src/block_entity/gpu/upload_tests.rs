use super::vertices::changed_span;
use super::*;
use bevy::ecs::system::RunSystemOnce;

use render_model::BlockEntityVertex;
use std::sync::Arc;

#[test]
fn steady_uploads_portal_parameters_skip_unused_and_equal_frames() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(RenderQueue::new(queue));
    world.init_resource::<render_scene::BlockEntityFrameResource>();
    world.init_resource::<render_scene::BlockSelectionFrameResource>();
    world.run_system_once(init_gpu).unwrap();
    let mut system = IntoSystem::into_system(prepare_resources);
    system.initialize(&mut world);
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<BlockEntityGpu>().portal_uploads, 0);
    let buffer = world.resource::<BlockEntityGpu>().portal_uniform.id();
    let allocated = test_alloc::thread_allocations();
    system.run((), &mut world).unwrap();
    assert_eq!(test_alloc::thread_allocations() - allocated, 0);

    let portal: Arc<[BlockEntityVertex]> = Arc::from([BlockEntityVertex::default(); 3]);
    {
        let mut frame = world.resource_mut::<render_scene::BlockEntityFrameResource>();
        frame.portal = portal.clone();
        frame.revision += 1;
    }
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<BlockEntityGpu>().portal_uploads, 1);
    let allocated = test_alloc::thread_allocations();
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<BlockEntityGpu>().portal_uploads, 1);
    assert_eq!(test_alloc::thread_allocations() - allocated, 0);
    world
        .resource_mut::<render_scene::BlockEntityFrameResource>()
        .portal_time_seconds = 1.0;
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<BlockEntityGpu>().portal_uploads, 2);

    {
        let mut frame = world.resource_mut::<render_scene::BlockEntityFrameResource>();
        frame.portal = Arc::from([]);
        frame.portal_time_seconds = 2.0;
        frame.revision += 1;
    }
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<BlockEntityGpu>().portal_uploads, 2);
    assert_eq!(world.resource::<BlockEntityGpu>().portal.count, 0);
    {
        let mut frame = world.resource_mut::<render_scene::BlockEntityFrameResource>();
        frame.portal = portal;
        frame.revision += 1;
    }
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<BlockEntityGpu>().portal_uploads, 3);
    assert_eq!(world.resource::<BlockEntityGpu>().portal.count, 3);
    assert_eq!(
        world.resource::<BlockEntityGpu>().portal_uniform.id(),
        buffer
    );
}

#[test]
fn changed_spans_cover_every_differing_byte_and_skip_identical_lists() {
    let vertex = |x: f32| BlockEntityVertex {
        position: [x, 0.0, 0.0],
        ..BlockEntityVertex::default()
    };
    // Spans cross several comparison blocks so block and byte refinement both run.
    let count = COMPARE_BLOCK_BYTES / VERTEX_BYTES as usize * 3 + 5;
    let previous: Vec<_> = (0..count).map(|index| vertex(index as f32)).collect();
    assert_eq!(changed_span(&previous, &previous.clone()), None);
    assert_eq!(changed_span(&[], &[]), None);

    let mut moved = previous.clone();
    moved[7].position[1] = 1.0;
    moved[count - 9].uv[0] = 0.5;
    assert_eq!(changed_span(&previous, &moved), Some(7..count - 8));

    let mut signed = previous.clone();
    signed[0] = vertex(-0.0);
    assert_eq!(changed_span(&previous, &signed), Some(0..1));
    let mut last = previous.clone();
    last[count - 1].position[2] = f32::from_bits(0x7fc0_0001);
    assert_eq!(changed_span(&previous, &last), Some(count - 1..count));

    assert_eq!(
        changed_span(&previous, &previous[..count - 2]),
        Some(count - 2..count - 2)
    );
    let mut grown = previous.clone();
    grown.push(vertex(-1.0));
    assert_eq!(changed_span(&previous, &grown), Some(count..count + 1));
    assert_eq!(changed_span(&[], &previous), Some(0..count));
}

/// Real block-model preparation reuses mapped staging across changing publications.
#[cfg(target_os = "macos")]
#[test]
fn recurring_vertices_do_not_allocate_native_staging_buffers() {
    let Some(mut app) = crate::upload_allocation_tests::app(BlockEntityRenderPlugin) else {
        return;
    };
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.init_resource::<render_scene::BlockEntityFrameResource>();
    world.init_resource::<render_scene::BlockSelectionFrameResource>();
    let mut vertices = vec![BlockEntityVertex::default(); 6];
    world
        .resource_mut::<render_scene::BlockEntityFrameResource>()
        .solid = Arc::from(vertices.clone());
    world.run_system_once(prepare_resources).unwrap();
    let before = crate::upload_allocation_tests::buffers(world);
    let staged = world
        .resource::<crate::upload_staging::BufferUploadStaging>()
        .stats()
        .staged_writes;
    for generation in 1..=3 {
        vertices[0].position[0] = generation as f32;
        {
            let mut frame = world.resource_mut::<render_scene::BlockEntityFrameResource>();
            frame.solid = Arc::from(vertices.clone());
            frame.revision += 1;
        }
        world.run_system_once(prepare_resources).unwrap();
        assert_eq!(crate::upload_allocation_tests::buffers(world), before);
    }
    let stats = world
        .resource::<crate::upload_staging::BufferUploadStaging>()
        .stats();
    assert!(stats.staged_writes >= staged + 3);
    assert_eq!(stats.fallback_writes, 0);
}

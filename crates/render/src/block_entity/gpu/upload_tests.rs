use super::*;
use bevy::{ecs::system::RunSystemOnce, render::renderer::WgpuWrapper};
use std::sync::Arc;

#[test]
fn steady_uploads_portal_parameters_skip_unused_and_equal_frames() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))));
    world.init_resource::<BlockEntityFrame>();
    world.init_resource::<BlockSelectionFrame>();
    world.run_system_once(init_gpu).unwrap();
    let mut system = IntoSystem::into_system(prepare_resources);
    system.initialize(&mut world);
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<BlockEntityGpu>().portal_uploads, 0);
    let buffer = world.resource::<BlockEntityGpu>().portal_uniform.id();
    let allocated = crate::alloc_count::thread_allocations();
    system.run((), &mut world).unwrap();
    assert_eq!(crate::alloc_count::thread_allocations() - allocated, 0);

    let portal: Arc<[BlockEntityVertex]> = Arc::from([BlockEntityVertex::default(); 3]);
    {
        let mut frame = world.resource_mut::<BlockEntityFrame>();
        frame.portal = portal.clone();
        frame.revision += 1;
    }
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<BlockEntityGpu>().portal_uploads, 1);
    let allocated = crate::alloc_count::thread_allocations();
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<BlockEntityGpu>().portal_uploads, 1);
    assert_eq!(crate::alloc_count::thread_allocations() - allocated, 0);
    world.resource_mut::<BlockEntityFrame>().portal_time_seconds = 1.0;
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<BlockEntityGpu>().portal_uploads, 2);

    {
        let mut frame = world.resource_mut::<BlockEntityFrame>();
        frame.portal = Arc::from([]);
        frame.portal_time_seconds = 2.0;
        frame.revision += 1;
    }
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<BlockEntityGpu>().portal_uploads, 2);
    assert_eq!(world.resource::<BlockEntityGpu>().portal.count, 0);
    {
        let mut frame = world.resource_mut::<BlockEntityFrame>();
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

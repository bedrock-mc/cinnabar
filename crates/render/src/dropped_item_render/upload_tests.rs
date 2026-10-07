use super::*;
use bevy::{ecs::system::RunSystemOnce, render::renderer::WgpuWrapper};
use std::sync::Arc;

/// Initializes item resources on the CPU-only backend for deterministic preparation tests.
fn item_world() -> World {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))));
    world.init_resource::<DroppedItemScene>();
    world.init_resource::<TerrainItemMeshGenerations>();
    world.run_system_once(init_gpu).unwrap();
    world
}

#[test]
fn hidden_item_catalog_is_prepared_before_visibility() {
    let mut world = item_world();
    let mut system = IntoSystem::into_system(prepare_items);
    system.initialize(&mut world);
    system.run((), &mut world).unwrap();
    let model = DroppedItemModel::Sprite(render_model::DroppedItemSprite {
        width: 1,
        height: 1,
        rgba8: Arc::from([255; 4]),
    });
    let hidden = crate::dropped_item::TerrainItemInstance {
        instance: crate::dropped_item::DroppedItemInstance {
            model: 0,
            world_from_item: crate::dropped_item::dropped_item_transform([0.0; 3], 0.0, 1.0),
            block_level: 15,
            sky_level: 15,
            overlay_rgba8: 0,
        },
        visible: false,
        transitions: Arc::from([]),
    };
    {
        let mut scene = world.resource_mut::<DroppedItemScene>();
        scene.publish(1, Arc::from([model]), &[], &[], 1.0);
        scene.publish_terrain_instances(1, &[hidden]);
    }
    system.run((), &mut world).unwrap();
    let (mesh, atlas) = {
        let gpu = world.resource::<ItemGpu>();
        assert_eq!(gpu.models_revision, 1);
        assert!(!gpu.ranges[0].is_empty());
        assert!(gpu.draws.is_empty());
        assert_eq!(gpu.upload_calls, 0);
        (
            gpu.mesh_buffer.as_ref().unwrap().id(),
            gpu._atlas.as_ref().unwrap().id(),
        )
    };
    {
        let mut scene = world.resource_mut::<DroppedItemScene>();
        Arc::make_mut(&mut scene.terrain_instances)[0].visible = true;
    }
    system.run((), &mut world).unwrap();
    let gpu = world.resource::<ItemGpu>();
    assert_eq!(gpu.draws.len(), 1);
    assert_eq!(gpu.mesh_buffer.as_ref().unwrap().id(), mesh);
    assert_eq!(gpu._atlas.as_ref().unwrap().id(), atlas);
}

#[test]
fn steady_uploads_empty_item_scene_allocates_and_uploads_nothing() {
    let mut world = item_world();
    let mut system = IntoSystem::into_system(prepare_items);
    system.initialize(&mut world);
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<ItemGpu>().upload_calls, 0);
    let buffers = {
        let gpu = world.resource::<ItemGpu>();
        [
            gpu.instance_buffer.id(),
            gpu.dynamic_buffer.id(),
            gpu.environment.id(),
        ]
    };
    {
        let mut gpu = world.resource_mut::<ItemGpu>();
        gpu.draws.push((0..3, 0));
        gpu.dynamic_count = 3;
    }
    let allocated = crate::alloc_count::thread_allocations();
    system.run((), &mut world).unwrap();
    assert_eq!(crate::alloc_count::thread_allocations() - allocated, 0);
    let gpu = world.resource::<ItemGpu>();
    assert!(gpu.draws.is_empty());
    assert_eq!(gpu.dynamic_count, 0);
    assert_eq!(gpu.upload_calls, 0);
    assert_eq!(
        [
            gpu.instance_buffer.id(),
            gpu.dynamic_buffer.id(),
            gpu.environment.id()
        ],
        buffers
    );

    let vertex = ItemMeshVertex {
        position: [0.0; 3],
        uv: [0.0; 2],
        normal: [0.0, 1.0, 0.0],
        layer: 0,
        color: render_model::OPAQUE_WHITE,
    };
    world.resource_mut::<DroppedItemScene>().dynamic = Arc::from([vertex; 3]);
    system.run((), &mut world).unwrap();
    let gpu = world.resource::<ItemGpu>();
    assert_eq!(gpu.dynamic_count, 3);
    assert_eq!(gpu.identity_instance, 0);
    assert_eq!(gpu.upload_calls, 3);
    world.resource_mut::<DroppedItemScene>().clear();
    let allocated = crate::alloc_count::thread_allocations();
    system.run((), &mut world).unwrap();
    assert_eq!(crate::alloc_count::thread_allocations() - allocated, 0);
    assert_eq!(world.resource::<ItemGpu>().upload_calls, 3);
    assert_eq!(world.resource::<ItemGpu>().dynamic_count, 0);
}

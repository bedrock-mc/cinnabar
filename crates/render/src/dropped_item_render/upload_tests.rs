use super::*;
use bevy::ecs::system::RunSystemOnce;

use std::sync::Arc;
use {render_model::ItemMeshVertex, render_scene::DroppedItemScene};

/// Initializes item resources on the CPU-only backend for deterministic preparation tests.
fn item_world() -> World {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(RenderQueue::new(queue));
    world.init_resource::<DroppedItemScene>();
    world.init_resource::<TerrainItemMeshGenerations>();
    world.run_system_once(init_gpu).unwrap();
    world
}

#[test]
fn potted_block_artwork_keeps_its_resolution_and_alpha() {
    let side = 64_u32;
    let mut pixels = vec![90_u8; (side * side * 4) as usize];
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    let transparent = (2 * side as usize + 3) * 4;
    pixels[transparent + 3] = 0;
    let model = DroppedItemModel::Block(render_model::DroppedItemBlock {
        materials: Arc::from([(
            render_model::DroppedItemSprite {
                width: side,
                height: side,
                rgba8: Arc::from(pixels.clone()),
            },
            render_model::OPAQUE_WHITE,
        )]),
        quads: Arc::from([assets::ModelQuad {
            positions: [[36, 64, 36], [220, 64, 220], [220, 256, 220], [36, 256, 36]],
            uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
            material: 0,
            flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
        }]),
        rotation: 0,
    });
    let mut world = item_world();
    world
        .resource_mut::<DroppedItemScene>()
        .publish(1, Arc::from([model.clone()]), &[], &[], 1.0);
    world.run_system_once(prepare_items).unwrap();
    let gpu = world.resource::<ItemGpu>();
    assert_eq!(gpu._atlas.as_ref().unwrap().width(), side);
    assert_eq!(
        gpu.ranges[0].vertices.len(),
        6,
        "high-resolution pot art must remain drawable"
    );
    let mut atlas = vec![255; (side * side * 4) as usize];
    let mesh = build_model(&mut atlas, &model, side).unwrap();
    assert_eq!(&atlas[(side * side * 4) as usize..], &pixels);
    assert!(mesh.iter().any(|vertex| vertex.uv == [1.0, 1.0]));
    assert_eq!(
        atlas_side(&[DroppedItemModel::Sprite(render_model::DroppedItemSprite {
            width: 16,
            height: 16,
            rgba8: Arc::from([255; 16 * 16 * 4])
        })]),
        MAX_ITEM_SPRITE_SIDE
    );
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
    let hidden = render_model::TerrainItemInstance {
        instance: render_model::DroppedItemInstance {
            model: 0,
            world_from_item: render_model::dropped_item_transform([0.0; 3], 0.0, 1.0),
            block_level: 15,
            sky_level: 15,
            overlay_rgba8: 0,
            glint: false,
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
        assert!(!gpu.ranges[0].vertices.is_empty());
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
        gpu.draws.push(ItemDraw {
            vertices: 0..3,
            instance: 0,
            cull_back: false,
            bounds: None,
        });
        gpu.dynamic_count = 3;
    }
    let allocated = test_alloc::thread_allocations();
    system.run((), &mut world).unwrap();
    assert_eq!(test_alloc::thread_allocations() - allocated, 0);
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
    let allocated = test_alloc::thread_allocations();
    system.run((), &mut world).unwrap();
    assert_eq!(test_alloc::thread_allocations() - allocated, 0);
    assert_eq!(world.resource::<ItemGpu>().upload_calls, 3);
    assert_eq!(world.resource::<ItemGpu>().dynamic_count, 0);
}

#[test]
fn reclaimed_slots_leave_live_model_indices_and_atlas_layers_intact() {
    let mut world = item_world();
    let sprite = DroppedItemModel::Sprite(render_model::DroppedItemSprite {
        width: 1,
        height: 1,
        rgba8: Arc::from([255; 4]),
    });
    world.resource_mut::<DroppedItemScene>().publish(
        1,
        Arc::from([sprite.clone(), DroppedItemModel::Vacant, sprite]),
        &[],
        &[],
        1.0,
    );
    world.run_system_once(prepare_items).unwrap();
    let gpu = world.resource::<ItemGpu>();
    assert_eq!(gpu.ranges.len(), 3);
    assert!(!gpu.ranges[0].vertices.is_empty());
    assert!(gpu.ranges[1].vertices.is_empty());
    assert!(!gpu.ranges[2].vertices.is_empty());
    assert_eq!(gpu._atlas.as_ref().unwrap().depth_or_array_layers(), 3);
}

#[test]
fn cull_modes_follow_models_and_preserve_instance_order_and_mirrored_transforms() {
    let mut world = item_world();
    let sprite = DroppedItemModel::NativeSprite(render_model::DroppedItemSprite {
        width: 1,
        height: 1,
        rgba8: Arc::from([255; 4]),
    });
    let plane = DroppedItemModel::Block(render_model::DroppedItemBlock {
        materials: Arc::from([(
            render_model::DroppedItemSprite {
                width: 1,
                height: 1,
                rgba8: Arc::from([255; 4]),
            },
            render_model::OPAQUE_WHITE,
        )]),
        quads: Arc::from([assets::ModelQuad {
            positions: [[0, 0, 128], [256, 0, 128], [256, 256, 128], [0, 256, 128]],
            uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
            material: 0,
            flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
        }]),
        rotation: 0,
    });
    let make = |model, scale| render_model::DroppedItemInstance {
        model,
        world_from_item: render_model::dropped_item_transform([0.0; 3], 0.0, scale),
        block_level: 15,
        sky_level: 15,
        overlay_rgba8: 0,
        glint: false,
    };
    world.resource_mut::<DroppedItemScene>().publish(
        1,
        Arc::from([sprite, plane]),
        &[make(0, 1.0), make(1, 1.0), make(0, -1.0), make(0, 1.0)],
        &[],
        1.0,
    );
    world.run_system_once(prepare_items).unwrap();
    let draws = &world.resource::<ItemGpu>().draws;
    assert_eq!(
        draws
            .iter()
            .map(|d| (d.instance, d.cull_back))
            .collect::<Vec<_>>(),
        [(0, true), (1, false), (2, false), (3, true)]
    );
    assert_eq!(draws[0].vertices, draws[2].vertices);
    assert_eq!(draws[0].vertices, draws[3].vertices);
}

#[test]
fn one_prepared_item_selects_its_cull_mode_independently_for_each_view() {
    let mesh = cube_mesh([0; 6], [render_model::OPAQUE_WHITE; 6], 1, 1).unwrap();
    let draw = ItemDraw {
        vertices: 0..mesh.len() as u32,
        instance: 0,
        cull_back: true,
        bounds: DroppedItemBounds::from_mesh(&mesh).and_then(|bounds| {
            bounds.transformed(render_model::dropped_item::dropped_item_transform(
                [0.0; 3], 0.0, 1.0,
            ))
        }),
    };
    for (z, expected) in [
        (2.5, true),
        (0.5 + render_api::CAMERA_NEAR_PLANE_BLOCKS * 0.8, false),
        (0.25, false),
        (2.5, true),
    ] {
        let eye = glam::Vec3::Z * z;
        let clip = glam::camera::rh::proj::directx::perspective_infinite_reverse(
            0.65,
            1.0,
            render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ) * glam::camera::rh::view::look_at_mat4(eye, glam::Vec3::ZERO, glam::Vec3::Y);
        let view = DroppedItemCullView::new(clip, eye).unwrap();
        assert_eq!(draw.culls_in_view(Some(&view)), expected);
    }
    assert!(!draw.culls_in_view(None));
}

#[test]
fn glinting_copies_carry_the_item_glint_scale_across_their_sprite() {
    let sprite = render_model::DroppedItemSprite {
        width: 16,
        height: 16,
        rgba8: Arc::from(vec![200_u8; 16 * 16 * 4]),
    };
    let mut world = item_world();
    let glint = render_model::EquipmentRaster {
        width: 2,
        height: 2,
        rgba8: Arc::from(vec![120_u8; 16]),
    };
    {
        let mut scene = world.resource_mut::<DroppedItemScene>();
        scene.publish(
            1,
            Arc::from([DroppedItemModel::NativeSprite(sprite)]),
            &[],
            &[],
            1.0,
        );
        scene.set_glint(Some(glint.clone()), [-0.25, 0.5, 0.75]);
    }
    world.run_system_once(prepare_items).unwrap();
    let gpu = world.resource::<ItemGpu>();
    // A 16-pixel sprite fills half of a 32-pixel layer, so 0.5 across it is 1 across the layer.
    assert_eq!(gpu.glint_scales, [[1.0, 1.0]]);
    assert!(
        gpu.glint
            .as_ref()
            .and_then(|(_, _, pixels)| pixels.as_ref())
            .is_some_and(|pixels| Arc::ptr_eq(pixels, &glint.rgba8)),
        "the scene's item glint raster is uploaded"
    );
    let instance = |glint| render_model::DroppedItemInstance {
        model: 0,
        world_from_item: render_model::dropped_item_transform([0.0; 3], 0.0, 1.0),
        block_level: 0,
        sky_level: 15,
        overlay_rgba8: 0,
        glint,
    };
    assert_eq!(
        pack_instance(&instance(true), &gpu.glint_scales).glint,
        [1.0, 1.0, 0.0, 0.0]
    );
    assert_eq!(
        pack_instance(&instance(false), &gpu.glint_scales).glint,
        [0.0; 4]
    );
}

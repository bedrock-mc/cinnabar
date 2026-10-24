use super::*;
use bevy::ecs::system::RunSystemOnce;
use render_model::dropped_item::cube_mesh;

use std::sync::Arc;
use {render_model::ItemMeshVertex, render_scene::DroppedItemScene};

/// Initializes item resources on the CPU-only backend for deterministic preparation tests.
pub(super) fn item_world() -> World {
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
        DroppedItemModel::Sprite(render_model::DroppedItemSprite {
            width: 16,
            height: 16,
            rgba8: Arc::from([255; 16 * 16 * 4])
        })
        .atlas_side(assets::texture::policy::device_policy()),
        Some(render_model::MIN_ITEM_ATLAS_SIDE)
    );
}

#[test]
fn hd_dropped_sprites_keep_their_source_pixels_and_physical_extent() {
    let width = 128;
    let height = 64;
    let pixels = (0..width * height)
        .flat_map(|i| [if i % 2 == 0 { 255 } else { 0 }, 73, 91, 255])
        .collect::<Vec<_>>();
    let model = DroppedItemModel::NativeSprite(render_model::DroppedItemSprite {
        width,
        height,
        rgba8: Arc::from(pixels.clone()),
    });
    assert_eq!(
        model.atlas_side(assets::texture::policy::device_policy()),
        Some(width)
    );
    let mut atlas = vec![255; (width * width * 4) as usize];
    assert!(build_model(&mut atlas, &model, width).is_some());
    let row = (width * 4) as usize;
    let layer = (width * width * 4) as usize;
    assert_eq!(&atlas[layer..layer + row * height as usize], &pixels);
    let mut world = item_world();
    world
        .resource_mut::<DroppedItemScene>()
        .publish(1, Arc::from([model]), &[], &[], 1.0);
    world.run_system_once(prepare_items).unwrap();
    assert_eq!(
        world.resource::<ItemGpu>()._atlas.as_ref().unwrap().width(),
        width
    );
}

#[test]
fn unaffordable_wide_sprite_does_not_hide_unrelated_small_items() {
    let large = render_model::DroppedItemSprite {
        width: 3072,
        height: 16,
        rgba8: vec![255; 3072 * 16 * 4].into(),
    };
    let small = DroppedItemModel::Sprite(render_model::DroppedItemSprite {
        width: 16,
        height: 16,
        rgba8: vec![73; 16 * 16 * 4].into(),
    });
    for oversized in [
        DroppedItemModel::Sprite(large.clone()),
        DroppedItemModel::NativeSprite(large),
    ] {
        for (oversized_index, models) in [
            [oversized.clone(), small.clone()],
            [small.clone(), oversized.clone()],
        ]
        .into_iter()
        .enumerate()
        {
            let small_index = 1 - oversized_index;
            let mut world = item_world();
            world
                .resource_mut::<DroppedItemScene>()
                .publish(1, Arc::from(models), &[], &[], 1.0);
            world.run_system_once(prepare_items).unwrap();
            let gpu = world.resource::<ItemGpu>();
            assert_eq!(gpu._atlas.as_ref().unwrap().width(), MIN_ITEM_ATLAS_SIDE);
            assert!(!gpu.ranges[small_index].vertices.is_empty());
            assert!(gpu.ranges[oversized_index].vertices.is_empty());
        }
    }
}

#[test]
fn checkerboard_items_cannot_overflow_the_aggregate_geometry_upload() {
    let side = 256;
    let pixels: Arc<[u8]> = (0..side * side)
        .flat_map(|index| {
            [
                255,
                73,
                91,
                if (index % side + index / side) % 2 == 0 {
                    255
                } else {
                    0
                },
            ]
        })
        .collect::<Vec<_>>()
        .into();
    let mut models = (0..9)
        .map(|_| {
            DroppedItemModel::Sprite(render_model::DroppedItemSprite {
                width: side,
                height: side,
                rgba8: Arc::clone(&pixels),
            })
        })
        .collect::<Vec<_>>();
    models.push(DroppedItemModel::Sprite(render_model::DroppedItemSprite {
        width: 16,
        height: 16,
        rgba8: vec![255; 16 * 16 * 4].into(),
    }));
    let mut world = item_world();
    let limit = world
        .resource::<RenderDevice>()
        .limits()
        .max_buffer_size
        .min(assets::texture::policy::MAX_SUBSCRIBER_BYTES as u64);
    world
        .resource_mut::<DroppedItemScene>()
        .publish(1, models.into(), &[], &[], 1.0);
    world.run_system_once(prepare_items).unwrap();
    let gpu = world.resource::<ItemGpu>();
    assert!(gpu.mesh_buffer.as_ref().unwrap().size() <= limit);
    let admitted = gpu.ranges[..9]
        .iter()
        .filter(|range| !range.vertices.is_empty())
        .count();
    assert!(admitted > 0 && admitted < 9);
    assert!(
        !gpu.ranges[9].vertices.is_empty(),
        "rejected meshes leave storage for smaller items"
    );
}

#[test]
fn rejected_hd_geometry_cannot_enlarge_the_atlas_or_hide_small_items() {
    let side = 2048;
    let pixels: Arc<[u8]> = (0..side * side)
        .flat_map(|index| {
            [
                73,
                91,
                255,
                if (index % side + index / side) % 2 == 0 {
                    255
                } else {
                    0
                },
            ]
        })
        .collect::<Vec<_>>()
        .into();
    for native in [false, true] {
        for hd_first in [false, true] {
            let sprite = render_model::DroppedItemSprite {
                width: side,
                height: side,
                rgba8: Arc::clone(&pixels),
            };
            let rejected = if native {
                DroppedItemModel::NativeSprite(sprite)
            } else {
                DroppedItemModel::Sprite(sprite)
            };
            let mut models = (0..4)
                .map(|_| {
                    DroppedItemModel::Sprite(render_model::DroppedItemSprite {
                        width: 16,
                        height: 16,
                        rgba8: vec![255; 16 * 16 * 4].into(),
                    })
                })
                .collect::<Vec<_>>();
            let rejected_index = if hd_first { 0 } else { models.len() };
            models.insert(rejected_index, rejected);
            let mut world = item_world();
            world
                .resource_mut::<DroppedItemScene>()
                .publish(1, models.into(), &[], &[], 1.0);
            world.run_system_once(prepare_items).unwrap();
            let gpu = world.resource::<ItemGpu>();
            assert_eq!(gpu._atlas.as_ref().unwrap().width(), MIN_ITEM_ATLAS_SIDE);
            assert!(gpu.ranges[rejected_index].vertices.is_empty());
            for (index, range) in gpu.ranges.iter().enumerate() {
                if index != rejected_index {
                    assert!(!range.vertices.is_empty());
                }
            }
            let buffer = gpu.mesh_buffer.as_ref().unwrap().id();
            let texture = gpu._atlas.as_ref().unwrap().id();
            world.run_system_once(prepare_items).unwrap();
            let gpu = world.resource::<ItemGpu>();
            assert_eq!(gpu.mesh_buffer.as_ref().unwrap().id(), buffer);
            assert_eq!(gpu._atlas.as_ref().unwrap().id(), texture);
        }
    }
}

#[test]
fn enlarging_an_admitted_atlas_keeps_earlier_uvs_glint_and_texture_layers() {
    let models = [16, 128].map(|side| {
        DroppedItemModel::Sprite(render_model::DroppedItemSprite {
            width: side,
            height: side,
            rgba8: vec![side as u8; (side * side * 4) as usize].into(),
        })
    });
    let original = PreparedModels::new(
        &models[..1],
        &[],
        render_model::dropped_item::ItemModelBudget::new(
            assets::texture::policy::device_policy(),
            render_model::MAX_ITEM_LAYERS,
            assets::texture::policy::MAX_SUBSCRIBER_BYTES as u64,
        ),
    );
    let prepared = PreparedModels::new(
        &models,
        &[],
        render_model::dropped_item::ItemModelBudget::new(
            assets::texture::policy::device_policy(),
            render_model::MAX_ITEM_LAYERS,
            assets::texture::policy::MAX_SUBSCRIBER_BYTES as u64,
        ),
    );
    assert_eq!(prepared.side, 128);
    let first = &prepared.ranges[0].vertices;
    for (before, after) in original
        .vertices
        .iter()
        .zip(&prepared.vertices[first.start as usize..first.end as usize])
    {
        for axis in 0..2 {
            assert_eq!(
                before.uv[axis] * before.glint_scale[axis],
                after.uv[axis] * after.glint_scale[axis],
                "atlas growth preserves texture-local foil coordinates"
            );
        }
    }
    for (index, model) in models.iter().enumerate() {
        let mut atlas = vec![255; (prepared.side * prepared.side * 4) as usize * (index + 1)];
        let expected = build_model(&mut atlas, model, prepared.side).unwrap();
        let range = &prepared.ranges[index].vertices;
        assert_eq!(
            &prepared.vertices[range.start as usize..range.end as usize],
            &expected
        );
        let DroppedItemModel::Sprite(sprite) = model else {
            unreachable!()
        };
        let layer_start = (index + 1) * (prepared.side * prepared.side * 4) as usize;
        assert_eq!(
            &prepared.atlas[layer_start..layer_start + 4],
            &sprite.rgba8[..4]
        );
    }
}

#[test]
fn buffer_rejection_cannot_enlarge_already_admitted_texture_layers() {
    let models = [(16, 16), (2048, 16)].map(|(width, height)| {
        DroppedItemModel::Sprite(render_model::DroppedItemSprite {
            width,
            height,
            rgba8: vec![255; (width * height * 4) as usize].into(),
        })
    });
    let prepared = PreparedModels::new(
        &models,
        &[],
        render_model::dropped_item::ItemModelBudget::new(
            assets::texture::policy::device_policy(),
            render_model::MAX_ITEM_LAYERS,
            (4096 * ITEM_MESH_VERTEX_BYTES) as u64,
        ),
    );
    assert_eq!(prepared.side, MIN_ITEM_ATLAS_SIDE);
    assert!(!prepared.ranges[0].vertices.is_empty());
    assert!(prepared.ranges[1].vertices.is_empty());
    assert_eq!(
        prepared.atlas.len(),
        2 * (MIN_ITEM_ATLAS_SIDE * MIN_ITEM_ATLAS_SIDE * 4) as usize
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
        glint_scale: [0.0; 2],
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
    let mut atlas = Vec::new();
    let vertices = build_model(
        &mut atlas,
        &world.resource::<DroppedItemScene>().models[0],
        32,
    )
    .unwrap();
    assert!(vertices.iter().all(|vertex| vertex.glint_scale == [1.0; 2]));
    assert!(
        gpu.glint
            .as_ref()
            .and_then(|glint| glint.pixels.as_ref())
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
        glint::pack_instance(&instance(true)).glint,
        [1.0, 0.0, 0.0, 0.0]
    );
    assert_eq!(glint::pack_instance(&instance(false)).glint, [0.0; 4]);
}

#[test]
fn reshaped_dropped_glint_reuploads_shared_pixels_and_retains_unchanged_shapes() {
    let mut world = item_world();
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let mut gpu = world.remove_resource::<ItemGpu>().unwrap();
    let mut scene = DroppedItemScene::default();
    let pixels: Arc<[u8]> = vec![255; 4 * 2 * 4].into();
    let mut previous = None;
    for (width, height) in [(4, 2), (2, 4), (2, 4)] {
        scene.set_glint(
            Some(render_model::EquipmentRaster {
                width,
                height,
                rgba8: Arc::clone(&pixels),
            }),
            [0.0; 3],
        );
        glint::upload(&scene, &device, &queue, &mut gpu);
        let texture = &gpu.glint.as_ref().unwrap()._texture;
        let size = [texture.width(), texture.height()];
        assert_eq!(size, [u32::from(width), u32::from(height)]);
        if let Some((previous_size, id)) = previous {
            assert_eq!(texture.id() == id, size == previous_size);
        }
        previous = Some((size, texture.id()));
    }
}

#[test]
fn oversized_dropped_glint_is_fitted_and_unchanged_uploads_are_retained() {
    let mut world = item_world();
    let device = world.resource::<RenderDevice>().clone();
    let queue = world.resource::<RenderQueue>().clone();
    let mut gpu = world.remove_resource::<ItemGpu>().unwrap();
    let mut scene = DroppedItemScene::default();
    let limit = device.limits().max_texture_dimension_2d;
    let width = u16::try_from(limit * 2).expect("the noop device admits oversized pack artwork");
    let pixels: Arc<[u8]> = vec![255; usize::from(width) * 4].into();
    scene.set_glint(
        Some(render_model::EquipmentRaster {
            width,
            height: 1,
            rgba8: Arc::clone(&pixels),
        }),
        [0.0; 3],
    );
    glint::upload(&scene, &device, &queue, &mut gpu);
    let uploaded = gpu.glint.as_ref().unwrap();
    assert_eq!(
        [uploaded._texture.width(), uploaded._texture.height()],
        [limit, 1]
    );
    assert!(Arc::ptr_eq(uploaded.pixels.as_ref().unwrap(), &pixels));
    let id = uploaded._texture.id();
    let allocations = test_alloc::thread_allocations();
    glint::upload(&scene, &device, &queue, &mut gpu);
    assert_eq!(test_alloc::thread_allocations() - allocations, 0);
    assert_eq!(gpu.glint.as_ref().unwrap()._texture.id(), id);
}

#[test]
fn adjacent_copies_of_one_mesh_share_an_instanced_draw_in_submission_order() {
    let draw = |vertices: Range<u32>, instance| ItemDraw {
        vertices,
        instance,
        cull_back: false,
        bounds: None,
    };
    let draws = [
        draw(0..36, 0),
        draw(0..36, 1),
        draw(0..36, 2),
        draw(36..42, 3),
        draw(0..36, 4),
        draw(0..36, 6),
    ];
    let mut submitted = Vec::new();
    for_each_instanced_run(&draws, None, |vertices, instances, culls| {
        submitted.push((vertices, instances, culls));
    });
    assert_eq!(
        submitted,
        [
            (0..36, 0..3, false),
            (36..42, 3..4, false),
            (0..36, 4..5, false),
            (0..36, 6..7, false),
        ]
    );
    let mut none = 0;
    for_each_instanced_run(&[], None, |_, _, _| none += 1);
    assert_eq!(none, 0);
}

#[test]
fn occupied_item_frames_retain_staging_and_draw_storage() {
    let mut world = item_world();
    let model = DroppedItemModel::Sprite(render_model::DroppedItemSprite {
        width: 1,
        height: 1,
        rgba8: Arc::from([255; 4]),
    });
    let instance = render_model::DroppedItemInstance {
        model: 0,
        world_from_item: render_model::dropped_item_transform([0.0; 3], 0.0, 1.0),
        block_level: 15,
        sky_level: 15,
        overlay_rgba8: 0,
        glint: false,
    };
    world
        .resource_mut::<DroppedItemScene>()
        .publish(1, Arc::from([model]), &[instance], &[], 1.0);
    let mut system = IntoSystem::into_system(prepare_items);
    system.initialize(&mut world);
    system.run((), &mut world).unwrap();
    let storage = |gpu: &ItemGpu| {
        (
            gpu.current_scratch.as_ptr(),
            gpu.instance_scratch.as_ptr(),
            gpu.draws.as_ptr(),
            gpu.mesh_buffer.as_ref().unwrap().id(),
            gpu._atlas.as_ref().unwrap().id(),
        )
    };
    let warmed = storage(world.resource::<ItemGpu>());
    for _ in 0..3 {
        system.run((), &mut world).unwrap();
        let gpu = world.resource::<ItemGpu>();
        assert_eq!(storage(gpu), warmed);
        assert_eq!(gpu.current_scratch, [true]);
        assert_eq!(gpu.draws.len(), 1);
        assert_eq!(gpu.draws[0].instance, 0);
        assert_eq!(gpu.identity_instance, 1);
        assert_eq!(gpu.instance_scratch.len(), 2);
    }
}

#[test]
fn admitted_framed_cubes_and_overflow_sprite_all_have_draw_ranges() {
    let cube = DroppedItemModel::Cube(render_model::DroppedItemCube {
        tile: 1,
        faces: std::array::from_fn(|_| Arc::from([255; 4])),
        tints: [render_model::OPAQUE_WHITE; 6],
    });
    let mut models = vec![cube; 42];
    models.push(DroppedItemModel::Sprite(render_model::DroppedItemSprite {
        width: 1,
        height: 1,
        rgba8: Arc::from([255; 4]),
    }));
    let budget = render_model::dropped_item::ItemModelBudget::new(
        assets::texture::policy::device_policy(),
        256,
        u64::MAX,
    );
    let prepared = PreparedModels::new(&models, &[true; 43], budget);
    assert_eq!(prepared.ranges.len(), 43);
    assert!(
        prepared
            .ranges
            .iter()
            .all(|range| !range.vertices.is_empty())
    );
    assert_eq!(
        prepared.atlas.len() / (prepared.side * prepared.side * 4) as usize,
        254
    );
}

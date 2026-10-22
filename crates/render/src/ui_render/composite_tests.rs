use super::*;
use render_model::{UiRenderBatch, UiScissor, UiTextureCatalog, UiTexturePage};

#[test]
fn composite_bindings_reuse_both_scene_orientations_and_expire_replaced_sources() {
    let world = super::super::ordered_command_tests::binding_world();
    let device = world.resource::<RenderDevice>();
    let cache = world.resource::<PipelineCache>();
    let pipeline = UiCompositePipeline::from_world(&mut World::new());
    let layout = cache.get_bind_group_layout(&pipeline.layout);
    let size = Extent3d {
        width: 2,
        height: 2,
        depth_or_array_layers: 1,
    };
    let layer = retained_layer(device, size);
    let scenes = [retained_layer(device, size), retained_layer(device, size)];
    let frame = frame_layer(&layer, false);
    let live_scenes = scenes.each_ref().map(|scene| scene.view.id());
    let first = frame.bindings.lock().unwrap().get(
        device,
        &layout,
        [&frame.view, &scenes[0].view],
        live_scenes,
    );
    let second = frame.bindings.lock().unwrap().get(
        device,
        &layout,
        [&frame.view, &scenes[1].view],
        live_scenes,
    );
    let allocated = test_alloc::thread_allocations();
    for index in 0..8 {
        let next = frame_layer(&layer, false);
        let group = next.bindings.lock().unwrap().get(
            device,
            &layout,
            [&next.view, &scenes[index % 2].view],
            live_scenes,
        );
        assert_eq!(
            group,
            if index % 2 == 0 {
                first.clone()
            } else {
                second.clone()
            }
        );
    }
    assert_eq!(test_alloc::thread_allocations(), allocated);
    let replacement = retained_layer(device, size);
    let changed = frame.bindings.lock().unwrap().get(
        device,
        &layout,
        [&frame.view, &replacement.view],
        [replacement.view.id(), scenes[1].view.id()],
    );
    assert_ne!(changed, first);
    assert_ne!(changed, second);
    assert_eq!(frame.bindings.lock().unwrap().entries.len(), 2);
    let next_pair = [retained_layer(device, size), retained_layer(device, size)];
    frame.bindings.lock().unwrap().get(
        device,
        &layout,
        [&frame.view, &next_pair[0].view],
        next_pair.each_ref().map(|scene| scene.view.id()),
    );
    let bindings = frame.bindings.lock().unwrap();
    assert_eq!(
        bindings.entries.len(),
        1,
        "retired targets must leave even if only one orientation is used"
    );
    assert_eq!(bindings.entries[0].0[1], next_pair[0].view.id());
    drop(bindings);
    let new_raster = frame_layer(&replacement, false);
    assert!(new_raster.bindings.lock().unwrap().entries.is_empty());
}

/// Supplies one valid triangle with an immutable texture catalog.
fn input() -> Arc<UiRenderInput> {
    Arc::new(UiRenderInput {
        revision: 1,
        viewport_size: [40; 2],
        safe_area: [0; 4],
        vertices: [[10.0, 10.0], [20.0, 10.0], [10.0, 20.0]]
            .map(|position| UiRenderVertex {
                position,
                clip_z: 0.0,
                clip_w: 1.0,
                uv: [0.0; 2],
                color: [255; 4],
                style_flags: 0,
                alpha_cutoff: -1.0,
                model_light: 1.0,
                overlay_color: [0.0; 4],
            })
            .into(),
        indices: Arc::from([0, 1, 2]),
        batches: Arc::from([UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, 40, 40),
            0,
            3,
            render_model::UI_BLEND_ALPHA,
        )]),
        textures: Arc::new(
            UiTextureCatalog::new(
                vec![UiTexturePage::owned([1, 1], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    })
}

/// Builds a completed full-target cache entry from its matching publication.
fn held(input: Arc<UiRenderInput>) -> HeldLayer {
    HeldLayer {
        content: UiLayerContent {
            revision: input.revision,
            texture_generation: 0,
            segment: 0..input.batches.len(),
            inverted: None,
            skip: None,
            viewport: None,
            model_depth: false,
        },
        encoded: true,
        publication: Some(input),
    }
}

#[test]
fn unchanged_publication_pixels_reuse_across_revisions_but_motion_has_damage() {
    let input = input();
    let held = held(Arc::clone(&input));
    let mut current = (*input).clone();
    current.revision = 2;
    let content = UiLayerContent {
        revision: 2,
        ..held.content.clone()
    };
    assert_eq!(
        held.damage(&content, &current, [40; 2]),
        super::super::damage::UiDamage::Unchanged
    );
    Arc::make_mut(&mut current.vertices)[0].position[0] = 8.0;
    assert_eq!(
        held.damage(&content, &current, [40; 2]),
        super::super::damage::UiDamage::Rect(UiScissor::new(7, 9, 14, 12))
    );
}

#[test]
fn partial_replay_requires_matching_complete_full_target_content() {
    for change in 0..10 {
        let input = input();
        let mut held = held(Arc::clone(&input));
        let mut current = (*input).clone();
        current.revision = 2;
        let mut content = UiLayerContent {
            revision: 2,
            ..held.content.clone()
        };
        let mut extent = [40; 2];
        match change {
            0 => held.encoded = false,
            1 => held.publication = None,
            2 => held.content.skip = Some(0..3),
            3 => content.skip = Some(0..3),
            4 => held.content.viewport = Some((UVec2::ZERO, UVec2::splat(40))),
            5 => content.viewport = Some((UVec2::ZERO, UVec2::splat(40))),
            6 => content.model_depth = true,
            7 => held.content.revision = 9,
            8 => content.revision = 9,
            9 => extent[0] = 41,
            _ => unreachable!(),
        }
        assert_eq!(
            held.damage(&content, &current, extent),
            super::super::damage::UiDamage::Full,
            "change {change}"
        );
    }
}

#[test]
fn damage_clear_is_single_sample_unblended_and_depth_free() {
    let descriptor = clear_pipeline_descriptor();
    assert!(descriptor.layout.is_empty());
    assert!(descriptor.vertex.buffers.is_empty());
    assert_eq!(descriptor.multisample.count, 1);
    assert!(descriptor.depth_stencil.is_none());
    let target = descriptor.fragment.unwrap().targets[0].clone().unwrap();
    assert_eq!(target.format, UI_LAYER_FORMAT);
    assert!(target.blend.is_none());
    assert_eq!(target.write_mask, ColorWrites::ALL);
}

#[test]
fn damage_clear_warmup_and_drawing_reuse_one_pipeline() {
    use crate::pipeline_warmup::{PrewarmPipelines, WarmView};

    let world = super::super::ordered_command_tests::binding_world();
    let cache = world.resource::<PipelineCache>();
    for draw_first in [false, true] {
        let mut pipeline = UiCompositePipeline::from_world(&mut World::new());
        let key = UiCompositeKey {
            format: render_scene::SCENE_COLOR_FORMAT,
        };
        if draw_first {
            pipeline.specialize(cache, key).unwrap();
        }
        let mut expected = pipeline.clear;
        for (msaa, hdr) in [(Msaa::Off, false), (Msaa::Sample4, true)] {
            let mut ids = Vec::new();
            pipeline
                .prewarm(
                    cache,
                    WarmView {
                        msaa,
                        hdr,
                        enhanced: false,
                        shader_pack: false,
                        output: None,
                    },
                    &mut ids,
                )
                .unwrap();
            let clear = pipeline.clear.expect("warmup queues the damage clear");
            assert!(ids.contains(&clear), "clear must hold the warmup gate");
            assert_eq!(*expected.get_or_insert(clear), clear);
            pipeline.specialize(cache, key).unwrap();
            assert_eq!(pipeline.clear, Some(clear));
        }
    }
}

/// Segment content is independent of the changing world and of other alpha ranges.
#[test]
fn independent_segment_rasters_reuse_and_invalidate_separately() {
    let world = super::super::ordered_command_tests::binding_world();
    let device = world.resource::<RenderDevice>();
    let size = Extent3d {
        width: 40,
        height: 40,
        depth_or_array_layers: 1,
    };
    let layers = [retained_layer(device, size), retained_layer(device, size)];
    let mut frame = frame_layer(&layers[1], false);
    frame.segments.push(frame_layer(&layers[0], false));
    let content = UiLayerContent {
        revision: 1,
        texture_generation: 0,
        skip: None,
        viewport: None,
        model_depth: false,
        segment: 0..1,
        inverted: Some(1),
    };
    let second = UiLayerContent {
        segment: 2..3,
        inverted: None,
        ..content.clone()
    };
    frame
        .segment(0, 2)
        .unwrap()
        .hold(Some((content.clone(), true)));
    frame
        .segment(1, 2)
        .unwrap()
        .hold(Some((second.clone(), true)));
    assert_ne!(
        frame.segment(0, 2).unwrap().texture.id(),
        frame.texture.id()
    );
    for _ in 0..3 {
        // Live world textures are deliberately not part of retained alpha content.
        assert_eq!(frame.segment(0, 2).unwrap().holds(&content), Some(true));
        assert_eq!(frame.segment(1, 2).unwrap().holds(&second), Some(true));
    }
    frame.segment(0, 2).unwrap().hold(None);
    assert_eq!(frame.segment(0, 2).unwrap().holds(&content), None);
    assert_eq!(frame.segment(1, 2).unwrap().holds(&second), Some(true));
    assert!(
        frame.segment(0, 3).is_none(),
        "insufficient rasters must use full replay"
    );
}

/// Extra layers are bounded and removed segments release their old raster storage.
#[test]
fn segment_storage_reuses_then_releases_topology_extent_and_device_lifetimes() {
    let world = super::super::ordered_command_tests::binding_world();
    let device = world.resource::<RenderDevice>();
    let size = Extent3d {
        width: 40,
        height: 40,
        depth_or_array_layers: 1,
    };
    let mut layers = Vec::new();
    synchronize_layers(&mut layers, device, size, 3);
    let identities: Vec<_> = layers.iter().map(|layer| layer.texture.id()).collect();
    synchronize_layers(&mut layers, device, size, 3);
    assert_eq!(
        layers
            .iter()
            .map(|layer| layer.texture.id())
            .collect::<Vec<_>>(),
        identities
    );
    synchronize_layers(&mut layers, device, size, 1);
    assert_eq!(layers.len(), 1);
    synchronize_layers(&mut layers, device, Extent3d { width: 80, ..size }, 2);
    assert!(
        layers
            .iter()
            .all(|layer| !identities.contains(&layer.texture.id()))
    );
    assert_eq!(retained_segment_count(9, size), 1);
    assert_eq!(
        retained_segment_count(
            8,
            Extent3d {
                width: 4096,
                height: 4096,
                ..size
            }
        ),
        1
    );
    assert_eq!(retained_segment_count(3, size), 3);
}

/// Publication topology is part of the raster identity even if geometry is unchanged.
#[test]
fn segment_boundary_changes_never_reuse_a_different_alpha_range() {
    let input = input();
    let held = held(Arc::clone(&input));
    for (segment, inverted) in [(0..0, None), (0..1, Some(1))] {
        let content = UiLayerContent {
            segment,
            inverted,
            ..held.content.clone()
        };
        assert_eq!(
            held.damage(&content, &input, [40; 2]),
            super::super::damage::UiDamage::Full
        );
    }
}

/// Device replacement and view removal release accepted publications as well as color surfaces.
#[test]
fn retained_segments_leave_no_orphan_publication_after_view_or_device_removal() {
    let world = super::super::ordered_command_tests::binding_world();
    let device = world.resource::<RenderDevice>();
    let size = Extent3d {
        width: 40,
        height: 40,
        depth_or_array_layers: 1,
    };
    let owner = Entity::from_bits(7);
    let mut store = UiLayerStore::default();
    store.synchronize_device(device);
    for remove_device in [false, true] {
        let publication = input();
        let weak = Arc::downgrade(&publication);
        let layers = vec![retained_layer(device, size), retained_layer(device, size)];
        let content = UiLayerContent {
            revision: 1,
            texture_generation: 0,
            skip: None,
            viewport: None,
            model_depth: false,
            segment: 0..1,
            inverted: None,
        };
        frame_layer(&layers[0], false).hold_publication(Some((content, true)), Some(publication));
        store.views.insert(owner, layers);
        assert!(weak.upgrade().is_some());
        if remove_device {
            // Model a new device observation; synchronization must discard every old view.
            store.device = None;
            store.synchronize_device(device);
        } else {
            store.retain_live_views(|_| false);
        }
        assert!(store.views.is_empty());
        assert!(weak.upgrade().is_none());
    }
}

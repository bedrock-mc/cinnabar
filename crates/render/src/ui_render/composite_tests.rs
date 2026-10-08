use super::*;
use render_model::{UiRenderBatch, UiScissor, UiTextureCatalog, UiTexturePage};

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
            format: TextureFormat::bevy_default(),
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

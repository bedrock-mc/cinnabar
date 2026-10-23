//! Element filter bounds and pyramid regions.

use super::*;
use render_model::UI_BLEND_ALPHA;
use std::sync::Arc;

#[test]
fn level_regions_cover_the_group_with_a_texel_of_reach() {
    let bounds = UiScissor::new(10, 20, 30, 10);
    assert_eq!(
        level_rect(bounds, 1, [100, 100]),
        Some(UiScissor::new(4, 9, 17, 7))
    );
    assert_eq!(
        level_rect(bounds, 3, [8, 8]),
        Some(UiScissor::new(0, 1, 6, 4))
    );
    assert_eq!(
        level_rect(UiScissor::new(90, 90, 10, 10), 1, [40, 40]),
        None
    );
}

#[test]
fn scaled_filter_bounds_keep_geometry_and_target_pixel_blur_reach() {
    let input = UiRenderInput {
        revision: 1,
        viewport_size: [800, 600],
        safe_area: [0; 4],
        vertices: [[120.0, 80.0], [180.0, 80.0], [180.0, 120.0]]
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
            UiScissor::new(0, 0, 800, 600),
            0,
            3,
            UI_BLEND_ALPHA,
        )]),
        textures: Arc::new(
            render_model::UiTextureCatalog::new(
                vec![render_model::UiTexturePage::owned([1, 1], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    };
    let run = [(
        0,
        &input.batches[0],
        UiTextureLocation {
            bucket: 0,
            layer: 0,
        },
    )];
    assert_eq!(
        group_bounds(&input, &run, 4.0, [400, 300], Some([800, 600])),
        Some(UiScissor::new(47, 27, 56, 46))
    );
    assert_eq!(
        group_bounds(&input, &run, 4.0, [400, 300], None),
        Some(UiScissor::new(107, 67, 86, 66))
    );
    assert_eq!(
        group_bounds(&input, &run, 4.0, [400, 300], Some([0, 600])),
        None
    );
}

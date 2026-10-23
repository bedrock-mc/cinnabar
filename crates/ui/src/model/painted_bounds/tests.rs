use crate::{UiBlendMode, UiDrawBatch, UiDrawList, UiPoint, UiRect, UiVertex};
use render_api::{UI_STYLE_COVERAGE_PAINT, UiCoveragePaint};

/// Places tested paint to the left of an opaque triangle, in one shared scissor.
fn drawing(paint: UiCoveragePaint, coverage: bool) -> UiDrawList {
    let vertices = [0., 50.]
        .into_iter()
        .flat_map(|x| {
            let paint = if x == 0. {
                paint
            } else {
                UiCoveragePaint {
                    fill: [[255; 4]; 2],
                    border: [0; 4],
                }
            };
            let (color, overlay_color) = if coverage {
                paint.vertex_colors()
            } else {
                (paint.fill[0], [0.; 4])
            };
            [[x, 0.], [x + 20., 0.], [x, 20.]].map(|position| UiVertex {
                position,
                clip_z: 0.,
                clip_w: 1.,
                uv: [0., 1.],
                color,
                model_light: 1.,
                overlay_color,
                style_flags: if coverage {
                    u32::from(UI_STYLE_COVERAGE_PAINT)
                } else {
                    0
                },
                alpha_cutoff: -1.,
            })
        })
        .collect();
    UiDrawList {
        revision: 0,
        vertices,
        indices: (0..6).collect(),
        batches: vec![UiDrawBatch {
            texture_page: 0,
            clip: UiRect::new(
                UiPoint::new(0., 0.).unwrap(),
                UiPoint::new(100., 100.).unwrap(),
            )
            .unwrap(),
            blend: UiBlendMode::Alpha,
            depth_test: false,
            depth_write: false,
            world_projection: false,
            isolated_depth_scope: None,
            filter_group: None,
            index_range: 0..6,
        }],
    }
}

#[test]
fn painted_bounds_exclude_fully_transparent_plain_and_coverage_paint() {
    for coverage in [false, true] {
        let paint = UiCoveragePaint {
            fill: [[255, 0, 0, 0]; 2],
            border: [0; 4],
        };
        let bounds = drawing(paint, coverage).painted_bounds().unwrap();
        assert_eq!(bounds.min().x(), 50.);
        assert_eq!(bounds.max().x(), 70.);
        let mut empty = drawing(paint, coverage);
        empty.batches[0].index_range = 0..3;
        assert!(empty.painted_bounds().is_none());
    }
}

#[test]
fn painted_bounds_preserve_effect_bounds_and_reject_malformed_transparent_geometry() {
    let paint = UiCoveragePaint {
        fill: [[0; 4]; 2],
        border: [0; 4],
    };
    let mut effect = drawing(paint, false);
    effect.batches[0].blend = UiBlendMode::Filter;
    assert_eq!(effect.painted_bounds().unwrap().min().x(), 0.);
    effect.vertices[0].position[0] = f32::NAN;
    assert!(effect.painted_bounds().is_none());
}

#[test]
fn painted_bounds_keep_nonzero_gradient_stops_borders_and_translucent_paint() {
    for paint in [
        UiCoveragePaint {
            fill: [[0; 4], [255; 4]],
            border: [0; 4],
        },
        UiCoveragePaint {
            fill: [[0; 4]; 2],
            border: [255; 4],
        },
        UiCoveragePaint {
            fill: [[255, 0, 0, 1]; 2],
            border: [0; 4],
        },
    ] {
        assert_eq!(drawing(paint, true).painted_bounds().unwrap().min().x(), 0.);
    }
}

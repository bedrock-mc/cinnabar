//! Saturation is one CPU-combined row mask, rendered through the ordinary sprite path.

use super::status_motion::{hunger_shake_offset, hunger_shakes};
use super::{Canvas, HungerPaint};
use render_api::sprite_mask::{MaskLayout, MaskPatch, SpriteMask};
use std::sync::Arc;

const RASTER_NAMESPACE: &str = "client-raster:hunger/";
const ICON_SIDE: f32 = 9.;
const EXTENT: [f32; 2] = [81., 11.];

/// Combines current and held-food bands before filtering, including each icon's independent shake.
/// Shared pixels take the maximum opacity-weighted coverage, so inversion is applied only once.
pub fn saturation_mask(
    source: Arc<SpriteMask>,
    hunger: &HungerPaint,
    updates: u64,
    size: [u32; 2],
) -> MaskLayout {
    let shaking = hunger_shakes(hunger.saturation_empty, hunger.current, updates);
    let mut patches = Vec::new();
    for index in 0..10 {
        let current = hunger
            .saturation_overlay
            .unwrap_or(0)
            .saturating_sub(index * 2)
            .min(2);
        let potential = hunger
            .food_saturation_preview
            .unwrap_or(0)
            .saturating_sub(index * 2)
            .min(2)
            .max(current);
        let x = 72. - index as f32 * 8.;
        let y = 1.
            + if shaking {
                hunger_shake_offset(index, updates)
            } else {
                0.
            };
        for (start, end, opacity) in [(0, current, 255), (current, potential, 110)] {
            if end > start {
                patches.push(MaskPatch {
                    bounds: [
                        x + start as f32 * 4.5,
                        y,
                        x + end as f32 * 4.5,
                        y + ICON_SIDE,
                    ],
                    uv: [start as f32 * 0.5, 0., end as f32 * 0.5, 1.],
                    opacity,
                });
            }
        }
    }
    MaskLayout {
        source,
        extent: EXTENT,
        size,
        patches,
    }
}

/// Draws one combined row in its control's clip, preserving tint and faded inverse coverage.
pub(super) fn paint(
    canvas: &mut Canvas<'_>,
    hunger: &HungerPaint,
    updates: u64,
    node: &ui_engine::engine::screen::CustomDraw<'_>,
    visible: [f32; 4],
) {
    let origin = [node.dest[0], node.dest[1]];
    if hunger.saturation_overlay.unwrap_or(0) == 0
        && hunger.food_saturation_preview.unwrap_or(0) == 0
    {
        return;
    }
    let Some([_, path, _]) = hunger.sprites else {
        return;
    };
    let px = canvas.px();
    let bounds = [
        origin[0] - 80. * px,
        origin[1] - px,
        origin[0] + px,
        origin[1] + 10. * px,
    ];
    if bounds[2] <= visible[0]
        || bounds[3] <= visible[1]
        || bounds[0] >= visible[2]
        || bounds[1] >= visible[3]
    {
        return;
    }
    let size = canvas.raster_size(bounds);
    let source_size = std::array::from_fn(|axis| {
        (size[axis] as f32 * ICON_SIDE / EXTENT[axis])
            .ceil()
            .max(1.) as u32
    });
    let Some(source) = canvas.outline_mask(path, source_size) else {
        return;
    };
    let layout = saturation_mask(source, hunger, updates, size);
    let mut color = hunger.overlay_color;
    let blend = if hunger.inverse_overlay {
        color[..3].fill(255);
        ui::UiBlendMode::Invert
    } else {
        ui::UiBlendMode::Alpha
    };
    let key = format!(
        "{}:{}",
        node.key,
        node.data
            .get(json_ui::CUSTOM_CONTROL_INSTANCE_KEY)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    );
    let _ = canvas.mask(
        RASTER_NAMESPACE,
        &key,
        layout,
        bounds,
        node.alpha(color),
        blend,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads production row pixels without a renderer, pack file, or GPU fixture.
    fn row(left: u8, right: u8, hunger: &HungerPaint, updates: u64) -> Vec<u8> {
        let mask = Arc::new(SpriteMask::outline([9, 9], [9, 9], |x, _| {
            if x == 0 {
                left
            } else if x == 8 {
                right
            } else {
                0
            }
        }));
        saturation_mask(mask, hunger, updates, [81, 11])
            .rasterize()
            .unwrap()
    }

    #[test]
    fn last_column_only_pack_art_survives_on_every_saturation_icon() {
        let pixels = row(
            0,
            255,
            &HungerPaint {
                saturation_overlay: Some(20),
                ..Default::default()
            },
            1,
        );
        assert_eq!(
            (0..81)
                .filter(|x| pixels[(4 * 81 + x) * 4 + 3] == 255)
                .count(),
            10
        );
    }

    #[test]
    fn unequal_edges_take_max_coverage_before_current_and_preview_opacity() {
        for (current, preview, expected) in [(Some(20), None, 255), (None, Some(20), 110)] {
            let pixels = row(
                128,
                255,
                &HungerPaint {
                    saturation_overlay: current,
                    food_saturation_preview: preview,
                    ..Default::default()
                },
                1,
            );
            for x in (8..81).step_by(8) {
                assert_eq!(pixels[(4 * 81 + x) * 4 + 3], expected);
            }
        }
    }

    #[test]
    fn current_and_held_saturation_have_disjoint_exact_coverage() {
        let hunger = HungerPaint {
            saturation_overlay: Some(2),
            food_saturation_preview: Some(4),
            ..Default::default()
        };
        let pixels = row(255, 255, &hunger, 1);
        assert_eq!(pixels[(4 * 81 + 80) * 4 + 3], 255);
        assert_eq!(pixels[(4 * 81 + 64) * 4 + 3], 110);
        assert_eq!(pixels[(4 * 81 + 72) * 4 + 3], 255);
        assert_eq!(pixels[(4 * 81 + 56) * 4 + 3], 0);
    }

    #[test]
    fn disabled_overlays_are_empty_and_preview_crops_keep_the_whole_mask() {
        assert!(
            row(255, 255, &HungerPaint::default(), 1)
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[3] == 0)
        );
        let pixels = row(
            0,
            255,
            &HungerPaint {
                food_saturation_preview: Some(1),
                ..Default::default()
            },
            1,
        );
        assert!(
            pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 0),
            "a left-half preview must not invent the source's right edge"
        );
    }

    #[test]
    fn overlapping_shaking_icons_preserve_the_union_of_their_sparse_edges() {
        let hunger = HungerPaint {
            current: 0,
            saturation_empty: true,
            saturation_overlay: Some(20),
            ..Default::default()
        };
        for updates in 1..20 {
            let pixels = row(0, 255, &hunger, updates);
            for index in 0..10 {
                let x = 80 - index * 8;
                let y = 1.
                    + if hunger_shakes(true, 0, updates) {
                        hunger_shake_offset(index, updates)
                    } else {
                        0.
                    };
                assert_eq!(pixels[((y as usize + 4) * 81 + x as usize) * 4 + 3], 255);
            }
        }
    }
}

//! Resource-pack saturation outlines share the hunger control's motion, clipping and scale.

use super::status_motion::{hunger_shake_offset, hunger_shakes};
use super::{Canvas, HungerPaint, UiVisual};

/// One non-overlapping horizontal part of a food icon, in half-shank units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Band {
    start: u32,
    end: u32,
    preview: bool,
}

/// Separates the known saturation from additional held-food saturation without overdraw.
fn bands(current: Option<u32>, potential: Option<u32>, index: u32) -> impl Iterator<Item = Band> {
    let local = |points: Option<u32>| points.unwrap_or(0).saturating_sub(index * 2).min(2);
    let current = local(current);
    let potential = local(potential).max(current);
    [
        (current > 0).then_some(Band {
            start: 0,
            end: current,
            preview: false,
        }),
        (potential > current).then_some(Band {
            start: current,
            end: potential,
            preview: true,
        }),
    ]
    .into_iter()
    .flatten()
}

/// Draws all outlines together after the base row so matching atlas pages share invert batches.
pub(super) fn paint(
    canvas: &mut Canvas<'_>,
    hunger: &HungerPaint,
    updates: u64,
    origin: [f32; 2],
    visible: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    let Some([_, full, _]) = hunger.sprites else {
        return;
    };
    let px = canvas.px();
    let shaking = hunger_shakes(hunger.saturation_empty, hunger.current, updates);
    for index in 0..10 {
        let x = origin[0] + (-8. - index as f32 * 8.) * px;
        let y = origin[1]
            + if shaking {
                hunger_shake_offset(index, updates) * px
            } else {
                0.
            };
        for band in bands(
            hunger.saturation_overlay,
            hunger.food_saturation_preview,
            index,
        ) {
            // Adjacent native nine-pixel sprites overlap by one pixel. Give that column
            // to the right-hand icon so an inverse outline never inverts it twice.
            let right = if index == 0 { 9. } else { 8. };
            let bounds = [
                x + band.start as f32 * 4.5 * px,
                y,
                x + (band.end as f32 * 4.5).min(right) * px,
                y + 9. * px,
            ];
            if bounds[2] <= visible[0]
                || bounds[3] <= visible[1]
                || bounds[0] >= visible[2]
                || bounds[1] >= visible[3]
            {
                continue;
            }
            let mut color = hunger.overlay_color;
            if hunger.inverse_overlay {
                color[..3].fill(255);
            }
            if band.preview {
                color[3] = (u16::from(color[3]) * 110 / 255) as u8;
            }
            let Some(sprite) = canvas.sprite(
                full,
                json_ui::UvRect::full(),
                alpha(color),
                Default::default(),
            ) else {
                continue;
            };
            let Some(visual) = outline(
                sprite,
                [
                    band.start as f32 * 0.5,
                    (band.end as f32 * 4.5).min(right) / 9.,
                ],
                hunger.inverse_overlay,
            ) else {
                continue;
            };
            let _ = canvas.push(visual, bounds);
        }
    }
}

/// Crops only the drawn range while preserving the full source rectangle for mask sampling.
fn outline(sprite: UiVisual, range: [f32; 2], inverse: bool) -> Option<UiVisual> {
    let (texture_page, source, color) = match sprite {
        UiVisual::Sprite {
            texture_page,
            uv,
            color,
        } => (texture_page, uv.map(f32::from), color),
        UiVisual::StyledSprite {
            texture_page,
            uv,
            color,
            ..
        } => (texture_page, uv, color),
        _ => return None,
    };
    let mut uv = source;
    let width = source[2] - source[0];
    uv[0] += width * range[0];
    uv[2] = source[0] + width * range[1];
    Some(UiVisual::SpriteOutline {
        texture_page,
        uv,
        source,
        color,
        blend: if inverse {
            ui::UiBlendMode::Invert
        } else {
            ui::UiBlendMode::Alpha
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every half-shank is painted at most once, even when a held item adds saturation.
    #[test]
    fn current_and_held_saturation_have_disjoint_exact_coverage() {
        for current in 0..=20 {
            for potential in 0..=20 {
                let mut painted = [false; 20];
                for index in 0..10 {
                    for band in bands(Some(current), Some(potential), index) {
                        for point in band.start..band.end {
                            let at = (index * 2 + point) as usize;
                            assert!(!painted[at], "inverse overdraw at {at}");
                            painted[at] = true;
                            assert_eq!(band.preview, at >= current as usize);
                        }
                    }
                }
                assert_eq!(
                    painted.iter().filter(|&&v| v).count(),
                    current.max(potential) as usize
                );
            }
        }
    }

    /// Empty or hidden rows do not produce an overlay; cropped source samples retain pack bounds.
    #[test]
    fn disabled_overlays_are_empty_and_preview_crops_keep_the_whole_mask() {
        assert_eq!(bands(None, None, 0).count(), 0);
        let sprite = UiVisual::Sprite {
            texture_page: 7,
            uv: [10, 20, 28, 38],
            color: [255, 198, 0, 128],
        };
        let Some(UiVisual::SpriteOutline {
            texture_page,
            uv,
            source,
            color,
            blend,
        }) = outline(sprite, [0.5, 1.], true)
        else {
            panic!("outline");
        };
        assert_eq!(texture_page, 7);
        assert_eq!(uv, [19., 20., 28., 38.]);
        assert_eq!(source, [10., 20., 28., 38.]);
        assert_eq!(color, [255, 198, 0, 128]);
        assert_eq!(blend, ui::UiBlendMode::Invert);
    }

    /// Inversion is a blend policy; selected RGB and the native food previews remain independent.
    #[test]
    fn inverse_mode_preserves_the_selected_color_and_independent_food_controls() {
        let mut paint = HungerPaint {
            current: 8,
            sprites: Some(["empty", "full", "half"]),
            ..Default::default()
        };
        let style: ui::mod_hud::HungerOverlay = serde_json::from_value(serde_json::json!({
            "saturation":true,"held_food":true,"potential_hunger":false,
            "potential_saturation":true,"color":[1.,0.25,0.,0.5],"inverse_color":true
        }))
        .unwrap();
        paint.set_overlay(
            &style,
            Some(2.),
            protocol::FoodProperties::new(4, 0.3).as_ref(),
        );
        assert_eq!(paint.overlay_color, [255, 64, 0, 128]);
        assert!(paint.inverse_overlay);
        assert_eq!(paint.saturation_overlay, Some(2));
        assert_eq!(paint.food_preview, None);
        assert_eq!(paint.food_saturation_preview, Some(5));
        assert!(
            paint
                .cells(0)
                .all(|cell| cell.size == [9., 9.] && cell.tint == [255; 3])
        );
        let legacy: ui::mod_hud::HungerOverlay =
            serde_json::from_value(serde_json::json!({"held_food":true})).unwrap();
        paint.set_overlay(
            &legacy,
            Some(2.),
            protocol::FoodProperties::new(4, 0.3).as_ref(),
        );
        assert_eq!(paint.food_preview, Some(12));
        assert_eq!(paint.food_saturation_preview, Some(5));
        assert!(!paint.inverse_overlay);
    }
}

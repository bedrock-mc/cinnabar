//! Draws shipped sprites, animation cells and tinted alpha masks.

use super::{Bounds, Canvas, Rgba, UiPresentationError, UiVisual};
use crate::ui_runtime::oreui_assets::OreUiSprite;

impl Canvas<'_> {
    fn native_sprite(&self, key: &str) -> Option<(u16, OreUiSprite)> {
        let originals = self.originals?;
        let key = originals
            .animations
            .get(key)
            .and_then(|frames| {
                let total = frames
                    .iter()
                    .map(|(_, millis)| u64::from(*millis))
                    .sum::<u64>();
                if total == 0 {
                    return None;
                }
                let mut at = ((self.seconds.max(0.0) * 1000.0) as u64) % total;
                frames.iter().find_map(|(key, millis)| {
                    if at < u64::from(*millis) {
                        Some(key.as_str())
                    } else {
                        at -= u64::from(*millis);
                        None
                    }
                })
            })
            .unwrap_or(key);
        let sprite = *originals.sprites.get(key)?;
        Some((originals.page.checked_add(sprite.page)?, sprite))
    }

    /// Draws shipped original pixels; false means the named artwork is unavailable.
    pub(in super::super) fn sprite(
        &mut self,
        key: &str,
        b: Bounds,
        color: Rgba,
    ) -> Result<bool, UiPresentationError> {
        let Some((texture_page, sprite)) = self.native_sprite(key) else {
            return Ok(false);
        };
        self.push(
            b,
            UiVisual::Sprite {
                texture_page,
                uv: sprite.bounds,
                color,
            },
        )?;
        Ok(true)
    }

    /// Fits original artwork inside a box without changing its aspect ratio.
    pub(in super::super) fn fitted_sprite(
        &mut self,
        key: &str,
        bounds: Bounds,
    ) -> Result<bool, UiPresentationError> {
        let Some((texture_page, sprite)) = self.native_sprite(key) else {
            return Ok(false);
        };
        let [left, top, right, bottom] = sprite.bounds;
        let width = f32::from(right - left);
        let height = f32::from(bottom - top);
        let scale = ((bounds[2] - bounds[0]) / width).min((bounds[3] - bounds[1]) / height);
        let x = (bounds[0] + bounds[2] - width * scale) * 0.5;
        let y = (bounds[1] + bounds[3] - height * scale) * 0.5;
        self.push(
            [x, y, x + width * scale, y + height * scale],
            UiVisual::Sprite {
                texture_page,
                uv: sprite.bounds,
                color: [255; 4],
            },
        )?;
        Ok(true)
    }

    /// Center-crops a background to fill its viewport without stretching its pixels.
    pub(in super::super) fn cover_sprite(
        &mut self,
        key: &str,
        bounds: Bounds,
    ) -> Result<bool, UiPresentationError> {
        let Some((texture_page, sprite)) = self.native_sprite(key) else {
            return Ok(false);
        };
        let [left, top, right, bottom] = sprite.bounds.map(f32::from);
        let width = right - left;
        let height = bottom - top;
        let scale = ((bounds[2] - bounds[0]) / width).max((bounds[3] - bounds[1]) / height);
        let crop_width = (bounds[2] - bounds[0]) / scale;
        let crop_height = (bounds[3] - bounds[1]) / scale;
        let x = left + (width - crop_width) * 0.5;
        let y = top + (height - crop_height) * 0.5;
        self.push(
            bounds,
            UiVisual::Sprite {
                texture_page,
                uv: [x, y, x + crop_width, y + crop_height].map(|value| value.round() as u16),
                color: [255; 4],
            },
        )?;
        Ok(true)
    }

    /// Selects one equal-width cell without resampling or repacking its horizontal sheet.
    pub(in super::super) fn sprite_frame(
        &mut self,
        key: &str,
        b: Bounds,
        color: Rgba,
        frame: u16,
        frames: u16,
    ) -> Result<bool, UiPresentationError> {
        let Some((texture_page, sprite)) = self.native_sprite(key) else {
            return Ok(false);
        };
        let [left, top, right, bottom] = sprite.bounds;
        let width = right - left;
        if frames == 0 || frame >= frames || !width.is_multiple_of(frames) {
            return Ok(false);
        }
        let side = width / frames;
        let left = left + side * frame;
        self.push(
            b,
            UiVisual::Sprite {
                texture_page,
                uv: [left, top, left + side, bottom],
                color,
            },
        )?;
        Ok(true)
    }

    /// Applies a role color to the original alpha shape of a monochrome image.
    pub(in super::super) fn masked_sprite(
        &mut self,
        key: &str,
        b: Bounds,
        color: Rgba,
    ) -> Result<bool, UiPresentationError> {
        self.rotated_masked_sprite(key, b, color, 0.0)
    }

    pub(in super::super) fn rotated_masked_sprite(
        &mut self,
        key: &str,
        b: Bounds,
        color: Rgba,
        angle_radians: f32,
    ) -> Result<bool, UiPresentationError> {
        let Some(originals) = self.originals else {
            return Ok(false);
        };
        let Some(sprite) = originals.masks.get(key) else {
            return Ok(false);
        };
        let Some(texture_page) = originals.page.checked_add(sprite.page) else {
            return Ok(false);
        };
        let visual = if angle_radians == 0.0 {
            UiVisual::Sprite {
                texture_page,
                uv: sprite.bounds,
                color,
            }
        } else {
            UiVisual::RotatedSprite {
                texture_page,
                uv: sprite.bounds,
                color,
                angle_radians,
            }
        };
        self.push(b, visual)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Originals;
    use super::*;
    use crate::ui_runtime::{
        oreui_assets::OreUiImages,
        presentation::{TextMetrics, tests::fixture_font},
    };
    use std::{collections::HashMap, sync::Arc};

    #[test]
    fn sprite_mask_preserves_its_role_color() {
        let sprite = OreUiSprite {
            page: 1,
            bounds: [3, 4, 15, 16],
        };
        let sprites = Arc::new(HashMap::from([("native".into(), sprite)]));
        let originals = Originals {
            page: 10,
            images: OreUiImages {
                pages: Vec::new(),
                sprites: sprites.clone(),
                loading_frames: Default::default(),
                animations: Default::default(),
            },
            sprites,
            masks: HashMap::from([(
                "native".into(),
                OreUiSprite {
                    bounds: [20, 30, 32, 42],
                    ..sprite
                },
            )]),
            loading_frames: Default::default(),
            animations: Default::default(),
        };
        let font = fixture_font();
        let (mut nodes, mut next, mut layouts) = (Vec::new(), 1, ui::TextLayoutCache::new(8, 4096));
        let metrics =
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
        let mut canvas = Canvas::new(
            &mut nodes,
            &mut next,
            &mut layouts,
            &font,
            metrics,
            0,
            Some(&originals),
        );
        canvas.alpha = 0.5;
        assert!(
            canvas
                .masked_sprite("native", [0.0, 0.0, 12.0, 12.0], [153, 153, 153, 200])
                .unwrap()
        );
        let sprites: Vec<_> = nodes
            .iter()
            .filter_map(|node| match node.visual() {
                UiVisual::Sprite {
                    texture_page,
                    uv,
                    color,
                } => Some((*texture_page, *uv, *color)),
                _ => None,
            })
            .collect();
        assert_eq!(sprites, [(11, [20, 30, 32, 42], [153, 153, 153, 100])]);
    }

    #[test]
    fn unchanged_native_art_reuses_pages_and_changed_art_replaces_its_range() {
        use crate::ui_runtime::{oreui_assets::OreUiPage, presentation::UiPresentationRuntime};
        let images = |count: usize| OreUiImages {
            pages: (0..count)
                .map(|page| OreUiPage {
                    dimensions: [2, 2],
                    pixels: vec![page as u8; 16].into(),
                })
                .collect(),
            sprites: Arc::new(HashMap::from([(
                "native".into(),
                OreUiSprite {
                    page: 0,
                    bounds: [0, 0, 2, 2],
                },
            )])),
            loading_frames: Default::default(),
            animations: Default::default(),
        };
        let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
        let start = runtime.textures.dynamic_start();
        let dynamic_count = runtime.textures.pages().len() - start;
        let first = images(1);
        runtime.enable_oreui_originals(first.clone()).unwrap();
        let registered = runtime.textures.clone();
        runtime.enable_oreui_originals(first).unwrap();
        assert!(
            Arc::ptr_eq(&registered, &runtime.textures),
            "unchanged art must retain the published texture catalog"
        );
        runtime.enable_oreui_originals(images(2)).unwrap();
        assert_eq!(runtime.textures.dynamic_start(), start + 2);
        assert_eq!(runtime.textures.pages().len(), start + 2 + dynamic_count);
        assert_eq!(runtime.textures.pages()[start + 1].pixels(), &[1; 16]);
        assert_eq!(
            runtime
                .form_presentation
                .oreui_originals
                .as_ref()
                .unwrap()
                .page,
            start as u16
        );
    }
}

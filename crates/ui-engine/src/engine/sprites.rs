//! Source-coordinate sprite sampling from ordinary atlases and original virtual images.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{Canvas, UiVisual, screen::PaintError};

impl Canvas<'_> {
    /// Snaps logical sprite bounds to physical pixels after a custom renderer positions them.
    pub fn snap_sprite_bounds(&self, [x0, y0, x1, y1]: [f32; 4]) -> [f32; 4] {
        super::pixel_snap::snapped(
            [x0, y0, x1 - x0, y1 - y0].map(f64::from),
            self.metrics.dpi_scale.get(),
            1.0,
        )
    }

    /// A sprite of the texture at `path` (server pack first, then the carrier),
    /// sampling the normalised `uv`; `None` when neither holds it.
    /// Maps source edges onto their atlas page without rounding fractional slice boundaries.
    pub fn sprite(
        &self,
        path: &str,
        uv: json_ui::UvRect,
        color: [u8; 4],
        filter: json_ui::SpriteFilter,
    ) -> Option<UiVisual> {
        self.sprite_with_repeat(path, uv, color, filter, None)
    }

    /// Maps a repeated region into its atlas page while retaining tile coordinates separately.
    pub(super) fn sprite_with_repeat(
        &self,
        path: &str,
        uv: json_ui::UvRect,
        color: [u8; 4],
        filter: json_ui::SpriteFilter,
        repeat: Option<json_ui::SpriteRepeat>,
    ) -> Option<UiVisual> {
        let Some((page, [x, y, w, h])) = self.textures.sprite(path) else {
            // An unresolved texture draws vanilla's default white texture.
            return self.textures.missing(path).then_some(UiVisual::Solid {
                texture_page: self.solid_page,
                color,
            });
        };
        let pixel = |base: f32, span: f32, t: f32| {
            let value = base + span * t;
            if value.is_nan() {
                0.0
            } else {
                value.clamp(0.0, f32::from(u16::MAX))
            }
        };
        let uv = [
            pixel(x, w, uv.u0),
            pixel(y, h, uv.v0),
            pixel(x, w, uv.u1),
            pixel(y, h, uv.v1),
        ];
        match repeat {
            Some(repeat) => UiVisual::repeated_sprite(page, uv, repeat, color, filter, false).ok(),
            None => Some(visual(page, uv, color, filter)),
        }
    }

    /// Paints an original virtual image, or consumes its pending draw until exact texels arrive.
    pub(super) fn paint_native_sprite(
        &mut self,
        path: &str,
        uv: json_ui::UvRect,
        color: [u8; 4],
        filter: json_ui::SpriteFilter,
        dest: [f32; 4],
        repeat: Option<json_ui::SpriteRepeat>,
    ) -> Result<bool, PaintError> {
        if !self.textures.native_sheet(path) {
            return Ok(false);
        }
        if let Some(quads) = self.textures.native_quads(path, uv, dest) {
            let mut seen = std::collections::BTreeSet::new();
            for quad in quads {
                let page = u16::try_from(quad.page).map_err(|_| PaintError::InvalidFontTexture)?;
                if let Some(repeat) = repeat {
                    if seen.insert(page)
                        && let Ok(visual) = UiVisual::repeated_sprite(
                            page,
                            quad.repeat_source,
                            repeat,
                            color,
                            filter,
                            true,
                        )
                    {
                        self.push(visual, dest)?;
                    }
                } else {
                    self.push(visual(page, quad.uv, color, filter), quad.dest)?;
                }
            }
        }
        Ok(true)
    }
}

/// Vanilla's image renderers that read only their control's data and the screen's artwork.
impl Canvas<'_> {
    /// Messaging art, drawn as its first frame.
    pub(super) fn animated_gif(
        &self,
        data: &BTreeMap<String, Value>,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<(UiVisual, [f32; 4])> {
        let path = data.get("#gif_path")?.as_str()?;
        let image = self.art.images?.get(path)?;
        let opacity = data
            .get("#alpha")
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        let tint = alpha([255, 255, 255, (255.0 * opacity) as u8]);
        Some((
            UiVisual::Sprite {
                texture_page: image.page,
                uv: image.uv,
                color: tint,
            },
            dest,
        ))
    }

    /// A friend row's gamerpic, which names its own image; the player's own is a screen's.
    pub(super) fn named_profile_image(
        &self,
        data: &BTreeMap<String, Value>,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<(UiVisual, [f32; 4])> {
        let Some(Value::String(path)) = data.get("#profile_image_options") else {
            return None;
        };
        let portrait = self.art.images?.get(path)?;
        Some((
            UiVisual::Sprite {
                texture_page: portrait.page,
                uv: portrait.uv,
                color: alpha([255; 4]),
            },
            dest,
        ))
    }
}

/// Uses integer sprite coordinates when possible and retains fractional filtering boundaries.
fn visual(page: u16, uv: [f32; 4], color: [u8; 4], filter: json_ui::SpriteFilter) -> UiVisual {
    let style = (u8::from(filter.grayscale) * ui::UI_STYLE_GRAYSCALE)
        | (u8::from(filter.bilinear) * ui::UI_STYLE_BILINEAR);
    if style == 0 && uv.iter().all(|edge| edge.fract() == 0.0) {
        UiVisual::Sprite {
            texture_page: page,
            uv: uv.map(|edge| edge as u16),
            color,
        }
    } else {
        UiVisual::StyledSprite {
            texture_page: page,
            uv,
            color,
            style,
        }
    }
}

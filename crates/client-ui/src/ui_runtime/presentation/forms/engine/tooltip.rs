//! Vanilla hover text geometry, painted through retained JSON-UI nodes.
//! See docs/reference/inventory-hover-tooltip.md for the vanilla rules.

use std::collections::BTreeMap;

use json_ui::{Rect, RectOut, TextureSource, nine_slice};
use serde_json::Value;
use ui::{TextShadow, UiVisual};

use super::text_paint::{UNWRAPPED_LOGICAL, width_64};
use {
    super::{Painter, UiPresentationError},
    ui::FONT_DESIGN_PIXEL_TEXELS,
};

pub(super) const RENDERER: &str = "hover_text_renderer";
pub(super) const BACKGROUND_TEXTURE: &str = "textures/ui/purpleBorder";

// Vanilla tooltip mouse offset, padding and text inset.
const MOUSE_OFFSET: [f32; 2] = [10.0, -10.0];
const BOX_EXTRA: f32 = 8.0;
const WIDTH_EXTRA: f32 = 1.0;
const TEXT_OFFSET: f32 = 5.0;
// Wrapped line pitch is the default font scale × 10.
const TEXT_PITCH: u32 = 10;

/// Current update takes a minimum first-line height, then adds wrap pitch for
/// each newline. Keep that rule even though the default bitmap metrics choose
/// pitch ten over the minimum eight.
fn text_height(font_scale: f32, wrap_height: f32, lines: u16) -> f32 {
    let minimum = ((font_scale - WIDTH_EXTRA) * 0.5 + WIDTH_EXTRA) * BOX_EXTRA;
    let pitch = wrap_height * font_scale;
    minimum.max(pitch) + f32::from(lines.saturating_sub(1)) * pitch
}

/// Native coordinates stay in virtual UI pixels until the final node emission.
/// A too-wide box flips left, then centers above the pointer if neither side fits.
fn box_rect(anchor: [f32; 2], text: [f32; 2], viewport: [f32; 2]) -> Rect {
    // Vanilla rounds the widest line's length upward first.
    let width = text[0].ceil() + WIDTH_EXTRA + BOX_EXTRA;
    let height = text[1].trunc() + BOX_EXTRA;
    let [mut dx, mut dy] = MOUSE_OFFSET;
    let bottom_overflow = anchor[1] + height + dy - viewport[1];
    if bottom_overflow > 0.0 {
        dy -= bottom_overflow;
    }
    if viewport[0] < anchor[0] + width + dx {
        dx = -(dx + width);
    }
    if anchor[0] + dx < 0.0 {
        dx = -0.5 * width;
        dy = -height;
    }
    Rect::new(
        f64::from(anchor[0] + dx),
        f64::from(anchor[1] + dy),
        f64::from(width),
        f64::from(height),
    )
}

impl Painter<'_> {
    pub(super) fn tooltip(
        &mut self,
        text: &str,
        data: &BTreeMap<String, Value>,
        dest: [f32; 4],
        alpha: &impl Fn([u8; 4]) -> [u8; 4],
    ) -> Result<(), UiPresentationError> {
        let Some(meta) = self.textures.texture(BACKGROUND_TEXTURE) else {
            // No fabricated flat background when an optional pack source is absent.
            return Ok(());
        };
        let anchor = self.art.pointer.unwrap_or([
            (dest[0] + dest[2]) * 0.5 / self.px,
            (dest[1] + dest[3]) * 0.5 / self.px,
        ]);
        let max_width = data
            .get("hover_text_max_width")
            // Constructor accepts the integer JSON variant only.
            .and_then(Value::as_i64)
            .filter(|width| *width > 0)
            .map_or(UNWRAPPED_LOGICAL, |width| width as f64 * f64::from(self.px));
        let mut request = self.metrics.request(text, width_64(max_width), self.font);
        request.line_height_64 = TEXT_PITCH * FONT_DESIGN_PIXEL_TEXELS * 64;
        let Ok(layout) = self.layouts.layout(request) else {
            return Ok(());
        };
        let extent = layout.size_64().map(|size| size as f32 / 64.0);
        // The accepted open-font path uses native default bitmap metrics.
        // Native box height comes from those metrics, not glyph ink overflow.
        let height = text_height(WIDTH_EXTRA, TEXT_PITCH as f32, layout.line_count());
        let logical_extent = [extent[0] / self.px, height];
        let viewport = [self.screen[2] / self.px, self.screen[3] / self.px];
        let background = box_rect(anchor, logical_extent, viewport);
        for quad in nine_slice(background, &meta) {
            if let Some(visual) = self.sprite(
                BACKGROUND_TEXTURE,
                quad.uv,
                alpha([255; 4]),
                json_ui::SpriteFilter::default(),
            ) {
                self.push(visual, self.snapped(&quad.dest))?;
            }
        }
        let origin = self.positioned(&RectOut {
            x: background.x + f64::from(TEXT_OFFSET),
            y: background.y + f64::from(TEXT_OFFSET),
            w: 0.0,
            h: 0.0,
        });
        self.push(
            UiVisual::Text {
                layout,
                color: alpha([255; 4]),
                // Vanilla draws hover text without shadow or outline.
                shadow: TextShadow::None,
            },
            [
                origin[0],
                origin[1],
                origin[0] + extent[0],
                origin[1] + extent[1],
            ],
        )
    }
}

#[cfg(test)]
mod tests;

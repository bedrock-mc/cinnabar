//! Rasterizes sign text into the 96x48 canvas the renderer maps onto a board face.
//!
//! The canvas is in Mojang design pixels (two font texels each). Line pitch, width and the
//! glow outline treatment follow the Java Edition sign and need native measurement for Bedrock.

use assets::CompiledFontCatalog;
use render_model::TEXT_CELL;
use std::hash::Hash;
use std::hash::Hasher;
use ui::{TextLayoutCache, TextLayoutRequest, TextStyle, UiScale};

const LINES: usize = 4;
const LINE_PITCH_PIXELS: i32 = 10;
/// Widest line before the text stops fitting the board.
const MAX_LINE_PIXELS: u32 = 90;
const TEXELS_PER_PIXEL: u32 = 2;
/// Wrap width wide enough that a measured line never wraps.
const WIDTH_PROBE_PIXELS: u32 = 4_096;
const ASCENT_TEXELS: u32 = 14;
const LINE_HEIGHT_TEXELS: u32 = 18;
const BLACK_OUTLINE_RGB: [u8; 3] = [0xF0, 0xEB, 0xCC];

/// One face's text as stored in the block entity, with its canvas cache key computed once
/// because every drawn sign looks its canvas up each frame.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SignTextSpec {
    text: String,
    /// `SignTextColor` as ARGB; alpha is ignored.
    color_argb: i32,
    glowing: bool,
    hide_glow_outline: bool,
    /// Hash of every field above; equal specs share one canvas.
    key: u64,
}

impl SignTextSpec {
    /// A face showing `text` in `SignTextColor` `color_argb`, glowing or not.
    pub(super) fn new(
        text: String,
        color_argb: i32,
        glowing: bool,
        hide_glow_outline: bool,
    ) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        color_argb.hash(&mut hasher);
        glowing.hash(&mut hasher);
        hide_glow_outline.hash(&mut hasher);
        Self {
            text,
            color_argb,
            glowing,
            hide_glow_outline,
            key: hasher.finish(),
        }
    }

    /// Whether the text ignores world light.
    pub(super) const fn glowing(&self) -> bool {
        self.glowing
    }

    /// Whether the face draws anything.
    pub(super) fn is_visible(&self) -> bool {
        self.text
            .chars()
            .any(|character| !character.is_whitespace())
    }

    /// Identifies this face's canvas; equal specs share one key.
    pub(super) const fn cache_key(&self) -> u64 {
        self.key
    }

    fn base_rgb(&self) -> [u8; 3] {
        let [_, red, green, blue] = self.color_argb.to_be_bytes();
        [red, green, blue]
    }

    fn outline_rgb(&self) -> [u8; 3] {
        let [red, green, blue] = self.base_rgb();
        if red == 0 && green == 0 && blue == 0 {
            BLACK_OUTLINE_RGB
        } else {
            // A darkened copy of the text color.
            [red, green, blue].map(|channel| (u16::from(channel) * 2 / 5) as u8)
        }
    }
}

/// Width of `text` in design pixels as one unwrapped line, or `None` if the font cannot lay it out.
pub(crate) fn line_width_design_pixels(
    text: &str,
    font: &CompiledFontCatalog,
    layouts: &mut TextLayoutCache,
) -> Option<f32> {
    let layout = layouts
        .layout(TextLayoutRequest {
            text,
            style: TextStyle::default(),
            width_64: WIDTH_PROBE_PIXELS * TEXELS_PER_PIXEL * 64,
            line_height_64: LINE_HEIGHT_TEXELS * 64,
            baseline_64: ASCENT_TEXELS * 64,
            scale: UiScale::default(),
            font,
            wrap: Default::default(),
        })
        .ok()?;
    Some(layout.size_64()[0] as f32 / (TEXELS_PER_PIXEL * 64) as f32)
}

struct Placed {
    /// Destination top-left in canvas pixels.
    origin: [i32; 2],
    /// Source rect in font-page texels `[x0, y0, x1, y1]`.
    source: [u16; 4],
    page: usize,
    rgb: [u8; 3],
}

/// Rasterizes `spec` into a `TEXT_CELL` RGBA8 canvas; `None` when the font cannot lay it out.
pub(super) fn rasterize(
    spec: &SignTextSpec,
    font: &CompiledFontCatalog,
    layouts: &mut TextLayoutCache,
) -> Option<Vec<u8>> {
    let [width, height] = TEXT_CELL.map(|value| value as i32);
    let mut placed = Vec::new();
    let block_top = (height - LINES as i32 * LINE_PITCH_PIXELS) / 2;
    for (index, line) in spec.text.split('\n').take(LINES).enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(layout) = layouts.layout(TextLayoutRequest {
            text: line,
            style: TextStyle::default(),
            width_64: MAX_LINE_PIXELS * TEXELS_PER_PIXEL * 64,
            line_height_64: LINE_HEIGHT_TEXELS * 64,
            baseline_64: ASCENT_TEXELS * 64,
            scale: UiScale::default(),
            font,
            wrap: Default::default(),
        }) else {
            continue;
        };
        let texel_64 = (TEXELS_PER_PIXEL * 64) as f32;
        let line_width = layout.size_64()[0] as f32 / texel_64;
        let left = (width as f32 - line_width) / 2.0;
        let top = block_top + index as i32 * LINE_PITCH_PIXELS;
        for glyph in layout.glyphs().iter().filter(|glyph| glyph.line == 0) {
            placed.push(Placed {
                origin: [
                    (left + glyph.bounds_64[0] as f32 / texel_64).round() as i32,
                    top + (glyph.bounds_64[1] as f32 / texel_64).round() as i32,
                ],
                source: glyph.uv,
                page: usize::from(glyph.page),
                rgb: glyph.style.color.rgb().unwrap_or_else(|| spec.base_rgb()),
            });
        }
    }
    let mut canvas = vec![0u8; (width * height * 4) as usize];
    if spec.glowing && !spec.hide_glow_outline {
        let outline = spec.outline_rgb();
        for glyph in &placed {
            blit(&mut canvas, font, glyph, outline, true);
        }
    }
    for glyph in &placed {
        blit(&mut canvas, font, glyph, glyph.rgb, false);
    }
    Some(canvas)
}

/// Copies one glyph, halving the texel grid to design pixels; with `dilate`, writes the
/// eight neighbors of every inked pixel that are still empty.
fn blit(canvas: &mut [u8], font: &CompiledFontCatalog, glyph: &Placed, rgb: [u8; 3], dilate: bool) {
    let Some(page) = font.pages().get(glyph.page) else {
        return;
    };
    let [width, height] = TEXT_CELL.map(|value| value as i32);
    let [x0, y0, x1, y1] = glyph.source.map(i32::from);
    let columns = (x1 - x0 + 1) / TEXELS_PER_PIXEL as i32;
    let rows = (y1 - y0 + 1) / TEXELS_PER_PIXEL as i32;
    for row in 0..rows {
        for column in 0..columns {
            let source_x = x0 + column * TEXELS_PER_PIXEL as i32;
            let source_y = y0 + row * TEXELS_PER_PIXEL as i32;
            let Some(alpha) = page_alpha(page, source_x, source_y) else {
                continue;
            };
            if alpha == 0 {
                continue;
            }
            let (x, y) = (glyph.origin[0] + column, glyph.origin[1] + row);
            let offsets: &[(i32, i32)] = if dilate {
                &[
                    (-1, -1),
                    (0, -1),
                    (1, -1),
                    (-1, 0),
                    (1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ]
            } else {
                &[(0, 0)]
            };
            for (dx, dy) in offsets {
                let (px, py) = (x + dx, y + dy);
                if px < 0 || py < 0 || px >= width || py >= height {
                    continue;
                }
                let index = ((py * width + px) * 4) as usize;
                if dilate && canvas[index + 3] != 0 {
                    continue;
                }
                canvas[index..index + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
    }
}

fn page_alpha(page: &assets::FontTexturePage, x: i32, y: i32) -> Option<u8> {
    let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
    if x >= page.width || y >= page.height {
        return None;
    }
    page.pixels
        .texel((y * page.width + x) as usize)
        .map(|texel| texel[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(text: &str) -> SignTextSpec {
        SignTextSpec::new(text.into(), -0x1000000, false, false)
    }

    #[test]
    fn color_decoding_ignores_alpha_and_black_outlines_are_light() {
        let face = SignTextSpec::new("x".into(), 0x00FF8040u32 as i32, false, false);
        assert_eq!(face.base_rgb(), [0xFF, 0x80, 0x40]);
        assert_eq!(spec("x").outline_rgb(), BLACK_OUTLINE_RGB);
        assert_eq!(face.outline_rgb(), [102, 51, 25]);
    }

    #[test]
    fn keys_track_every_visible_input() {
        let base = spec("hello");
        assert_eq!(base.cache_key(), spec("hello").cache_key());
        let glowing = SignTextSpec::new("hello".into(), -0x1000000, true, false);
        assert_ne!(base.cache_key(), glowing.cache_key());
        assert_ne!(base.cache_key(), spec("world").cache_key());
        assert!(!spec("  \n \n").is_visible());
        assert!(base.is_visible());
    }
}

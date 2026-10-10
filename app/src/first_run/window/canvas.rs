//! CPU drawing for the setup overlay: rectangles, the logo and Cinnangles Sans text into premultiplied
//! sRGB RGBA8.

use std::collections::HashMap;

use fontdue::{Font, FontSettings, Metrics};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// Insets every edge; negative values expand a focus outline.
    pub(super) fn inset(self, edge: f32) -> Self {
        Self {
            x: self.x + edge,
            y: self.y + edge,
            w: (self.w - 2.0 * edge).max(0.0),
            h: (self.h - 2.0 * edge).max(0.0),
        }
    }

    pub(super) fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// Straight-alpha RGBA8 image.
pub(super) struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub(super) struct Canvas {
    pub width: u32,
    pub height: u32,
    /// Premultiplied RGBA8, row-major.
    pub pixels: Vec<u8>,
}

impl Canvas {
    pub(super) fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
        }
    }

    /// Source-over of straight-alpha `color` at `coverage` (0..=255).
    fn blend(&mut self, x: i64, y: i64, color: [u8; 4], coverage: u32) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let alpha = u32::from(color[3]) * coverage / 255;
        if alpha == 0 {
            return;
        }
        let index = (y as usize * self.width as usize + x as usize) * 4;
        let pixel = &mut self.pixels[index..index + 4];
        for channel in 0..3 {
            let opacity = alpha as f32 / 255.0;
            let source = srgb_to_linear(color[channel]) * opacity;
            let destination = srgb_to_linear(pixel[channel]) * (1.0 - opacity);
            pixel[channel] = linear_to_srgb(source + destination);
        }
        pixel[3] = (alpha + u32::from(pixel[3]) * (255 - alpha) / 255) as u8;
    }

    /// Fills a rectangle with the same linear-light compositing as glyph coverage.
    pub(super) fn fill(&mut self, rect: Rect, color: [u8; 4]) {
        let (x0, y0) = (rect.x.round() as i64, rect.y.round() as i64);
        let (x1, y1) = (
            (rect.x + rect.w).round() as i64,
            (rect.y + rect.h).round() as i64,
        );
        let left = x0.clamp(0, i64::from(self.width)) as usize;
        let right = x1.clamp(0, i64::from(self.width)) as usize;
        if right <= left || color[3] == 0 {
            return;
        }
        // Rectangle coverage is constant; decode each possible destination channel once.
        let opacity = f32::from(color[3]) / 255.0;
        let channels: [[u8; 256]; 3] = std::array::from_fn(|channel| {
            let source = srgb_to_linear(color[channel]) * opacity;
            std::array::from_fn(|destination| {
                linear_to_srgb(source + srgb_to_linear(destination as u8) * (1.0 - opacity))
            })
        });
        for y in y0.max(0)..y1.min(i64::from(self.height)) {
            let start = (y as usize * self.width as usize + left) * 4;
            let end = (y as usize * self.width as usize + right) * 4;
            for pixel in self.pixels[start..end].as_chunks_mut::<4>().0 {
                for channel in 0..3 {
                    pixel[channel] = channels[channel][pixel[channel] as usize];
                }
                pixel[3] = (u32::from(color[3])
                    + u32::from(pixel[3]) * (255 - u32::from(color[3])) / 255)
                    as u8;
            }
        }
    }

    /// Draws a one-edge frame without double-compositing translucent corners.
    pub(super) fn frame(&mut self, rect: Rect, edge: f32, color: [u8; 4]) {
        self.fill(Rect { h: edge, ..rect }, color);
        self.fill(
            Rect {
                y: rect.y + rect.h - edge,
                h: edge,
                ..rect
            },
            color,
        );
        self.fill(
            Rect {
                y: rect.y + edge,
                w: edge,
                h: rect.h - 2.0 * edge,
                ..rect
            },
            color,
        );
        self.fill(
            Rect {
                x: rect.x + rect.w - edge,
                y: rect.y + edge,
                w: edge,
                h: rect.h - 2.0 * edge,
            },
            color,
        );
    }

    /// Nearest-neighbour scale of `image` into `rect`.
    pub(super) fn image(&mut self, image: &Image, rect: Rect) {
        if image.width == 0 || image.height == 0 || rect.w < 1.0 || rect.h < 1.0 {
            return;
        }
        let (x0, y0) = (rect.x.round() as i64, rect.y.round() as i64);
        let (w, h) = (rect.w.round() as i64, rect.h.round() as i64);
        for dy in 0..h {
            let sy = (dy * i64::from(image.height) / h) as usize;
            for dx in 0..w {
                let sx = (dx * i64::from(image.width) / w) as usize;
                let index = (sy * image.width as usize + sx) * 4;
                let texel = &image.rgba[index..index + 4];
                self.blend(
                    x0 + dx,
                    y0 + dy,
                    [texel[0], texel[1], texel[2], texel[3]],
                    255,
                );
            }
        }
    }
}

/// Decodes one stored sRGB channel for linear-light compositing.
fn srgb_to_linear(channel: u8) -> f32 {
    let value = f32::from(channel) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// Encodes a premultiplied linear channel for the GPU's sRGB texture decoder.
fn linear_to_srgb(value: f32) -> u8 {
    let encoded = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Cinnangles Sans rasterized on demand, cached per glyph and size.
pub(super) struct Text {
    font: Font,
    cache: HashMap<(char, u32), (Metrics, Vec<u8>)>,
}

impl Text {
    pub(super) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let font = Font::from_bytes(bytes, FontSettings::default()).ok()?;
        Some(Self {
            font,
            cache: HashMap::new(),
        })
    }

    /// Characters the font lacks draw as their ASCII stand-in.
    fn resolve(&self, c: char) -> char {
        if self.font.lookup_glyph_index(c) != 0 || c == ' ' {
            return c;
        }
        match c {
            '…' => '.',
            '·' => '-',
            _ => '?',
        }
    }

    fn glyph(&mut self, c: char, px: f32) -> &(Metrics, Vec<u8>) {
        let c = self.resolve(c);
        let font = &self.font;
        self.cache
            .entry((c, px.to_bits()))
            .or_insert_with(|| font.rasterize(c, px))
    }

    pub(super) fn line_height(&self, px: f32) -> f32 {
        self.font
            .horizontal_line_metrics(px)
            .map_or(px * 1.3, |metrics| metrics.new_line_size)
            .max(px)
    }

    pub(super) fn width(&mut self, text: &str, px: f32) -> f32 {
        text.chars()
            .map(|c| self.glyph(c, px).0.advance_width)
            .sum()
    }

    /// Greedy word wrap of each `\n`-separated paragraph to `max_width`.
    pub(super) fn wrap(&mut self, text: &str, px: f32, max_width: f32) -> Vec<String> {
        wrap(text, max_width, |s| self.width(s, px))
    }

    /// Draws `text` with its top-left at (`x`, `top`).
    pub(super) fn draw(
        &mut self,
        canvas: &mut Canvas,
        x: f32,
        top: f32,
        px: f32,
        color: [u8; 4],
        text: &str,
    ) {
        let ascent = self
            .font
            .horizontal_line_metrics(px)
            .map_or(px, |metrics| metrics.ascent);
        let baseline = (top + ascent).round() as i64;
        let mut pen = x;
        for c in text.chars() {
            let (metrics, bitmap) = self.glyph(c, px).clone();
            let left = (pen + metrics.xmin as f32).round() as i64;
            let glyph_top = baseline - i64::from(metrics.ymin) - metrics.height as i64;
            for row in 0..metrics.height {
                for column in 0..metrics.width {
                    let coverage = u32::from(bitmap[row * metrics.width + column]);
                    canvas.blend(
                        left + column as i64,
                        glyph_top + row as i64,
                        color,
                        coverage,
                    );
                }
            }
            pen += metrics.advance_width;
        }
    }
}

/// Word wrap by `width`; words wider than a line break between characters, so paths wrap too.
fn wrap(text: &str, max_width: f32, mut width: impl FnMut(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if width(&candidate) <= max_width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            for c in word.chars() {
                line.push(c);
                if width(&line) > max_width && line.chars().count() > 1 {
                    line.pop();
                    lines.push(std::mem::replace(&mut line, c.to_string()));
                }
            }
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_overlay_premultiplies_in_linear_light() {
        let mut canvas = Canvas::new(1, 1);
        canvas.blend(0, 0, [255, 0, 0, 128], 255);
        assert_eq!(canvas.pixels, vec![188, 0, 0, 128]);
    }

    #[test]
    fn fills_composite_premultiplied_source_over() {
        let mut canvas = Canvas::new(2, 1);
        canvas.fill(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            [255, 0, 0, 255],
        );
        canvas.fill(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 2.0,
                h: 1.0,
            },
            [0, 0, 255, 128],
        );
        assert_eq!(&canvas.pixels[..4], &[187, 0, 188, 255]);
        assert_eq!(&canvas.pixels[4..], &[0, 0, 188, 128]);
    }

    #[test]
    fn rectangle_lookup_matches_glyph_compositing_for_every_channel() {
        for alpha in [0, 51, 128, 255] {
            let mut expected = Canvas::new(256, 1);
            for value in 0..256 {
                let channel = value as u8;
                expected.blend(value, 0, [channel, channel, channel, 255], 255);
            }
            let mut actual = Canvas {
                width: expected.width,
                height: expected.height,
                pixels: expected.pixels.clone(),
            };
            let mut color = client_ui::oreui_theme::PRIMARY_ROLE.fill;
            color[3] = alpha;
            for value in 0..256 {
                expected.blend(value, 0, color, 255);
            }
            actual.fill(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 256.0,
                    h: 1.0,
                },
                color,
            );
            assert_eq!(actual.pixels, expected.pixels);
        }
    }

    #[test]
    fn long_words_break_to_fit() {
        let width = |s: &str| s.chars().count() as f32;
        assert_eq!(wrap("ab cd ef", 5.0, width), ["ab cd", "ef"]);
        assert_eq!(
            wrap("see /a/long/path ok", 6.0, width),
            ["see", "/a/lon", "g/path", "ok"]
        );
        assert_eq!(wrap("one\n\ntwo", 9.0, width), ["one", "", "two"]);
    }

    #[test]
    fn rects_hit_test_half_open() {
        let rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 5.0,
            h: 5.0,
        };
        assert!(rect.contains(10.0, 14.9));
        assert!(!rect.contains(15.0, 12.0));
    }
}

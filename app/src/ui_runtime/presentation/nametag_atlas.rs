//! Name-tag lines rasterized at font resolution into the shared atlas the tag billboards sample.

use std::{collections::HashMap, sync::Arc};

use assets::RuntimeFontCatalog;
use render::{NAMETAG_ATLAS_SIDE, NametagAtlasRect};
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, TEXT_BASELINE_64, TEXT_LINE_HEIGHT_64, TextLayoutCache,
    TextLayoutRequest, TextStyle, UiScale,
};

/// Widest line laid out before it would wrap, in font texels.
const MAX_LINE_TEXELS: u32 = NAMETAG_ATLAS_SIDE;

/// RGBA8 texels of one UI texture page a glyph samples.
#[derive(Clone, Copy)]
pub(crate) struct GlyphPage<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba8: &'a [u8],
}

/// The font's own pages, which lead the UI texture pages.
pub(crate) fn font_page(font: &RuntimeFontCatalog, page: usize) -> Option<GlyphPage<'_>> {
    font.pages().get(page).map(|page| GlyphPage {
        width: page.width,
        height: page.height,
        rgba8: &page.rgba8,
    })
}

/// One rasterized line: its atlas cell in texels and its width in font pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct AtlasLine {
    pub(super) cell: [u32; 4],
    pub(super) width_px: f32,
    /// Font pixels from the line box's top to the cell's top (negative for tall glyphs).
    pub(super) top_px: f32,
}

/// Shelf-packed line cells, rebuilt from scratch when a frame's lines no longer fit.
#[derive(Default)]
pub(crate) struct NametagAtlas {
    rectangles: Vec<NametagAtlasRect>,
    lines: HashMap<Arc<str>, AtlasLine>,
    shelf: [u32; 3],
    exhausted: bool,
    published: Option<Arc<[NametagAtlasRect]>>,
    revision: u64,
    palette: ui::FormattingPalette,
}

impl NametagAtlas {
    /// Retires cached line pixels when the active formatting colors change.
    pub(super) fn set_palette(&mut self, palette: ui::FormattingPalette) {
        if self.palette != palette {
            self.reset();
            self.palette = palette;
        }
    }

    /// The cell of `text`, rasterizing it on first use; `None` when the font cannot lay it out.
    pub(super) fn line<'p>(
        &mut self,
        text: &Arc<str>,
        font: &RuntimeFontCatalog,
        layouts: &mut TextLayoutCache,
        pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
    ) -> Option<AtlasLine> {
        if let Some(line) = self.lines.get(text) {
            return Some(*line);
        }
        let (width, height, top, rgba8, advance) =
            rasterize(text, font, layouts, pages, &self.palette)?;
        let origin = self.allocate(width, height)?;
        self.rectangles.push(NametagAtlasRect {
            cell: [origin[0], origin[1], width, height],
            rgba8: rgba8.into(),
        });
        let line = AtlasLine {
            cell: [origin[0], origin[1], width, height],
            width_px: advance as f32 / FONT_DESIGN_PIXEL_TEXELS as f32,
            top_px: top as f32 / FONT_DESIGN_PIXEL_TEXELS as f32,
        };
        self.lines.insert(Arc::clone(text), line);
        self.published = None;
        Some(line)
    }

    /// Forgets every line so the next frame packs only what it draws.
    pub(super) fn reset(&mut self) {
        self.lines.clear();
        self.shelf = [0; 3];
        self.exhausted = false;
        self.rectangles.clear();
        self.published = None;
    }

    /// Checks both retained line count and whether shelf space was exhausted.
    pub(super) fn has_room_for(&self, texts: usize) -> bool {
        !self.exhausted && self.lines.len().saturating_add(texts) < MAX_ATLAS_LINES
    }

    /// Retains immutable line pixels so skipped extractions can recover every update.
    pub(super) fn publish(&mut self) -> (Arc<[NametagAtlasRect]>, u64) {
        if self.published.is_none() {
            self.revision += 1;
            self.published = Some(self.rectangles.clone().into());
        }
        (
            Arc::clone(self.published.as_ref().expect("just published")),
            self.revision,
        )
    }

    /// Reserves a non-overlapping shelf cell.
    fn allocate(&mut self, width: u32, height: u32) -> Option<[u32; 2]> {
        let side = NAMETAG_ATLAS_SIDE;
        if width > side || height > side {
            return None;
        }
        let [mut x, mut y, mut shelf_height] = self.shelf;
        if x + width > side {
            (x, y, shelf_height) = (0, y + shelf_height, 0);
        }
        if y + height > side {
            self.exhausted = true;
            return None;
        }
        self.shelf = [x + width, y, shelf_height.max(height)];
        Some([x, y])
    }
}

/// Lines kept before the atlas is rebuilt, far above any frame's visible tags.
const MAX_ATLAS_LINES: usize = 4096;

/// `text` as one unwrapped line of font texels: `(width, height, RGBA8)`. Glyph texels keep
/// their own colour (image glyphs) times the `§` colour, white when unset.
fn rasterize<'p>(
    text: &str,
    font: &RuntimeFontCatalog,
    layouts: &mut TextLayoutCache,
    pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
    palette: &ui::FormattingPalette,
) -> Option<(u32, u32, i32, Vec<u8>, u32)> {
    let layout = layouts
        .layout(TextLayoutRequest {
            text,
            style: TextStyle::default(),
            width_64: MAX_LINE_TEXELS * 64,
            line_height_64: TEXT_LINE_HEIGHT_64,
            baseline_64: TEXT_BASELINE_64,
            scale: UiScale::default(),
            font,
            wrap: Default::default(),
        })
        .ok()?;
    let advance = layout.size_64()[0].div_ceil(64).max(1);
    let glyphs = || layout.glyphs().iter().filter(|glyph| glyph.line == 0);
    let top = glyphs()
        .map(|glyph| glyph.bounds_64[1].div_euclid(64))
        .min()
        .unwrap_or(0)
        .min(0);
    let bottom = glyphs()
        .map(|glyph| (glyph.bounds_64[3] + 63).div_euclid(64))
        .max()
        .unwrap_or(0)
        .max(TEXT_LINE_HEIGHT_64.div_ceil(64) as i32);
    let right = glyphs()
        .map(|glyph| (glyph.bounds_64[2] + 63).div_euclid(64))
        .max()
        .unwrap_or(0)
        .max(advance as i32);
    let (width, height) = (right as u32, (bottom - top) as u32);
    let mut canvas = vec![0u8; (width * height * 4) as usize];
    for glyph in glyphs() {
        let Some(page) = pages(usize::from(glyph.page)) else {
            continue;
        };
        let tint = palette.rgb(glyph.style.color).unwrap_or([255; 3]);
        // Sheet glyphs are drawn scaled into their bounds, so sample the source nearest-texel.
        let mut bounds = glyph.bounds_64.map(|value| value as f32 / 64.0);
        bounds[1] -= top as f32;
        bounds[3] -= top as f32;
        let [u0, v0, u1, v1] = glyph.uv.map(f32::from);
        let (source_width, source_height) = (u1 - u0 + 1.0, v1 - v0 + 1.0);
        let (dest_width, dest_height) = (bounds[2] - bounds[0], bounds[3] - bounds[1]);
        if dest_width <= 0.0 || dest_height <= 0.0 {
            continue;
        }
        for dy in bounds[1].floor().max(0.0) as u32..(bounds[3].ceil() as u32).min(height) {
            for dx in bounds[0].floor().max(0.0) as u32..(bounds[2].ceil() as u32).min(width) {
                let fx = (dx as f32 + 0.5 - bounds[0]) / dest_width;
                let fy = (dy as f32 + 0.5 - bounds[1]) / dest_height;
                if !(0.0..1.0).contains(&fx) || !(0.0..1.0).contains(&fy) {
                    continue;
                }
                let sx = (u0 + fx * source_width) as u32;
                let sy = (v0 + fy * source_height) as u32;
                if sx >= page.width || sy >= page.height {
                    continue;
                }
                let source = ((sy * page.width + sx) * 4) as usize;
                let Some(texel) = page.rgba8.get(source..source + 4) else {
                    continue;
                };
                if texel[3] == 0 {
                    continue;
                }
                let target = ((dy * width + dx) * 4) as usize;
                for channel in 0..3 {
                    canvas[target + channel] =
                        (u16::from(texel[channel]) * u16::from(tint[channel]) / 255) as u8;
                }
                canvas[target + 3] = texel[3];
            }
        }
    }
    Some((width, height, top, canvas, advance))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// Builds the labels used by the original full-atlas timing fixture.
    fn sample_atlas() -> (NametagAtlas, std::time::Duration) {
        let font = super::super::tests::fixture_font();
        let mut layouts = TextLayoutCache::new(256, 1 << 20);
        let mut atlas = NametagAtlas::default();
        let started = std::time::Instant::now();
        for index in 0..100 {
            let text = Arc::from(format!("Player {index}"));
            atlas
                .line(&text, &font, &mut layouts, &|page| font_page(&font, page))
                .unwrap();
            std::hint::black_box(atlas.publish());
        }
        let elapsed = started.elapsed();
        (atlas, elapsed)
    }

    /// Measures changing labels and fingerprints their published pixels.
    #[test]
    #[ignore = "release performance measurement"]
    fn frame_cost_bench_nametag_updates() {
        let (mut atlas, elapsed) = sample_atlas();
        let (rectangles, _) = atlas.publish();
        let mut pixels = vec![0; (NAMETAG_ATLAS_SIDE * NAMETAG_ATLAS_SIDE * 4) as usize];
        apply_rectangles(&mut pixels, &rectangles, &[]);
        let bytes: usize = rectangles
            .iter()
            .map(|rectangle| rectangle.rgba8.len())
            .sum();
        eprintln!(
            "NAMETAG_BENCH updates=100 ms={:.3} upload_bytes={bytes} sha256={:x}",
            elapsed.as_secs_f64() * 1000.0,
            Sha256::digest(&pixels)
        );
    }
    #[test]
    fn atlas_pixels_match_full_publication_baseline() {
        let (mut atlas, _) = sample_atlas();
        let mut pixels = vec![0; (NAMETAG_ATLAS_SIDE * NAMETAG_ATLAS_SIDE * 4) as usize];
        apply_rectangles(&mut pixels, &atlas.publish().0, &[]);
        assert_eq!(
            format!("{:x}", Sha256::digest(&pixels)),
            "1a17a5251c8766a32e7400d34848e3865bc55840ce6eb0d0ba843bcea676f7a5"
        );
    }

    /// Applies the same dirty rectangles submitted by the renderer.
    fn apply_rectangles(
        pixels: &mut [u8],
        current: &[NametagAtlasRect],
        previous: &[NametagAtlasRect],
    ) {
        for rectangle in NametagAtlasRect::updates(current, previous) {
            let [x, y, width, height] = rectangle.cell;
            for row in 0..height as usize {
                let target = (((y as usize + row) * NAMETAG_ATLAS_SIDE as usize) + x as usize) * 4;
                let source = row * width as usize * 4;
                pixels[target..target + width as usize * 4]
                    .copy_from_slice(&rectangle.rgba8[source..source + width as usize * 4]);
            }
        }
    }

    #[test]
    fn palette_change_rerasterizes_retained_name_lines() {
        let font = super::super::tests::fixture_font();
        let mut layouts = TextLayoutCache::new(8, 1 << 20);
        let mut atlas = NametagAtlas::default();
        let text = Arc::from("§2Player");
        let pages = |page| font_page(&font, page);
        atlas.line(&text, &font, &mut layouts, &pages).unwrap();
        let first = atlas.publish().0;
        assert!(
            first[0]
                .rgba8
                .chunks_exact(4)
                .any(|pixel| pixel == [0, 170, 0, 255])
        );
        atlas.set_palette(ui::FormattingPalette::from_globals(|name| {
            (name == "$2_color_format").then_some([0.976, 0.859, 0.427])
        }));
        atlas.line(&text, &font, &mut layouts, &pages).unwrap();
        let changed = atlas.publish().0;
        assert!(
            changed[0]
                .rgba8
                .chunks_exact(4)
                .any(|pixel| pixel == [249, 219, 109, 255])
        );
        assert_ne!(first[0].rgba8, changed[0].rgba8);
    }

    #[test]
    fn rectangles_preserve_skipped_publications_reset_and_old_readers() {
        let font = super::super::tests::fixture_font();
        let mut layouts = TextLayoutCache::new(8, 1 << 20);
        let mut atlas = NametagAtlas::default();
        assert!(atlas.publish().0.is_empty());
        let pages = |page| font_page(&font, page);
        atlas
            .line(&Arc::from("Player 0"), &font, &mut layouts, &pages)
            .unwrap();
        let first = atlas.publish().0;
        atlas
            .line(&Arc::from("§cPlayer 1"), &font, &mut layouts, &pages)
            .unwrap();
        let skipped = atlas.publish().0;
        atlas
            .line(&Arc::from("Player 2"), &font, &mut layouts, &pages)
            .unwrap();
        let latest = atlas.publish().0;
        assert_eq!(NametagAtlasRect::updates(&latest, &first).count(), 2);
        assert_eq!(NametagAtlasRect::updates(&latest, &latest).count(), 0);
        let mut incremental = vec![0; (NAMETAG_ATLAS_SIDE * NAMETAG_ATLAS_SIDE * 4) as usize];
        apply_rectangles(&mut incremental, &first, &[]);
        apply_rectangles(&mut incremental, &latest, &first);
        let mut full = vec![0; incremental.len()];
        apply_rectangles(&mut full, &latest, &[]);
        assert_eq!(incremental, full);
        atlas.reset();
        atlas
            .line(&Arc::from("New"), &font, &mut layouts, &pages)
            .unwrap();
        let reset = atlas.publish().0;
        assert_eq!(NametagAtlasRect::updates(&reset, &latest).count(), 1);
        assert_eq!(first.len(), 1);
        assert_eq!(skipped.len(), 2);
        assert!(Arc::ptr_eq(&first[0].rgba8, &latest[0].rgba8));
        assert!(
            NametagAtlasRect::updates(&reset, &latest)
                .next()
                .unwrap()
                .rgba8
                .len()
                < incremental.len()
        );
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;

    #[test]
    fn review_pixel_exhaustion_requests_a_clean_atlas_on_the_next_frame() {
        let mut atlas = NametagAtlas::default();
        assert!(
            atlas
                .allocate(NAMETAG_ATLAS_SIDE, NAMETAG_ATLAS_SIDE)
                .is_some()
        );
        assert!(atlas.allocate(1, 1).is_none());
        assert!(!atlas.has_room_for(1));
        atlas.reset();
        assert!(atlas.has_room_for(1));
        assert!(atlas.allocate(1, 1).is_some());
    }
}

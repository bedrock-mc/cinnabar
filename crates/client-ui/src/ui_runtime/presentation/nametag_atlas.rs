//! Name-tag lines rasterized at font resolution into the shared atlas the tag billboards sample.

use std::{collections::HashMap, sync::Arc};

use assets::RuntimeFontCatalog;
use render_model::{NAMETAG_ATLAS_SIDE, NametagAtlasRect};
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, TEXT_BASELINE_64, TEXT_BOLD_OFFSET_64, TEXT_LINE_HEIGHT_64,
    TextLayoutCache, TextLayoutRequest, TextStyle, UiScale,
};

/// Widest line laid out before it would wrap, in font texels.
const MAX_LINE_TEXELS: u32 = NAMETAG_ATLAS_SIDE;

/// Texels of one UI texture page a glyph samples.
#[derive(Clone, Copy)]
pub struct GlyphPage<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: GlyphPixels<'a>,
}

#[derive(Clone, Copy)]
pub enum GlyphPixels<'a> {
    Rgba8(&'a [u8]),
    /// One alpha byte per texel of a white page.
    Coverage(&'a [u8]),
}

impl GlyphPage<'_> {
    fn texel(&self, index: usize) -> Option<[u8; 4]> {
        match self.pixels {
            GlyphPixels::Rgba8(bytes) => bytes
                .get(index * 4..index * 4 + 4)
                .map(|texel| [texel[0], texel[1], texel[2], texel[3]]),
            GlyphPixels::Coverage(bytes) => bytes.get(index).map(|&alpha| [255, 255, 255, alpha]),
        }
    }
}

/// The font's own pages, which lead the UI texture pages.
pub fn font_page(font: &RuntimeFontCatalog, page: usize) -> Option<GlyphPage<'_>> {
    font.pages().get(page).map(|page| GlyphPage {
        width: page.width,
        height: page.height,
        pixels: match &page.pixels {
            assets::FontPixels::Rgba8(bytes) => GlyphPixels::Rgba8(bytes),
            assets::FontPixels::Coverage(bytes) => GlyphPixels::Coverage(bytes),
        },
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
pub struct NametagAtlas {
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
        .map(|glyph| {
            let extra = if glyph.style.bold
                && glyph.bounds_64[2] > glyph.bounds_64[0]
                && glyph.bounds_64[3] > glyph.bounds_64[1]
            {
                TEXT_BOLD_OFFSET_64 as i32
            } else {
                0
            };
            (glyph.bounds_64[2] + extra + 63).div_euclid(64)
        })
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
        rasterize_glyph(
            &mut canvas,
            [width, height],
            page,
            bounds,
            glyph.uv,
            tint,
            false,
        );
        if glyph.style.bold {
            let offset = TEXT_BOLD_OFFSET_64 as f32 / 64.0;
            bounds[0] += offset;
            bounds[2] += offset;
            rasterize_glyph(
                &mut canvas,
                [width, height],
                page,
                bounds,
                glyph.uv,
                tint,
                true,
            );
        }
    }
    Some((width, height, top, canvas, advance))
}

fn rasterize_glyph(
    canvas: &mut [u8],
    [width, height]: [u32; 2],
    page: GlyphPage<'_>,
    bounds: [f32; 4],
    uv: [u16; 4],
    tint: [u8; 3],
    composite: bool,
) {
    let [u0, v0, u1, v1] = uv.map(f32::from);
    let (source_width, source_height) = (u1 - u0, v1 - v0);
    let (dest_width, dest_height) = (bounds[2] - bounds[0], bounds[3] - bounds[1]);
    if source_width <= 0.0 || source_height <= 0.0 || dest_width <= 0.0 || dest_height <= 0.0 {
        return;
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
            let Some(texel) = page.texel((sy * page.width + sx) as usize) else {
                continue;
            };
            if texel[3] == 0 {
                continue;
            }
            let target = ((dy * width + dx) * 4) as usize;
            if composite {
                blend_glyph_texel(&mut canvas[target..target + 4], texel, tint);
            } else {
                for channel in 0..3 {
                    canvas[target + channel] =
                        (u16::from(texel[channel]) * u16::from(tint[channel]) / 255) as u8;
                }
                canvas[target + 3] = texel[3];
            }
        }
    }
}

fn blend_glyph_texel(target: &mut [u8], texel: [u8; 4], tint: [u8; 3]) {
    let source_alpha = u32::from(texel[3]);
    let target_alpha = u32::from(target[3]);
    let remaining = 255 - source_alpha;
    let alpha = source_alpha * 255 + target_alpha * remaining;
    for channel in 0..3 {
        let source = u32::from(texel[channel]) * u32::from(tint[channel]) / 255;
        let color =
            source * source_alpha * 255 + u32::from(target[channel]) * target_alpha * remaining;
        target[channel] = ((color + alpha / 2) / alpha) as u8;
    }
    target[3] = ((alpha + 127) / 255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn stem_font() -> RuntimeFontCatalog {
        stem_font_with_uv([0, 0, 4, 8], 6 * 64)
    }

    fn stem_font_with_uv(uv: [u16; 4], advance_64: i16) -> RuntimeFontCatalog {
        let mut pixels = vec![0; 5 * 8 * 4];
        for row in 0..8 {
            for column in 0..FONT_DESIGN_PIXEL_TEXELS as usize {
                pixels[(row * 5 + column) * 4..(row * 5 + column + 1) * 4]
                    .copy_from_slice(&[255; 4]);
            }
        }
        let page = assets::FontTexturePage {
            source_path: "font/stem.png".into(),
            source_bytes: pixels.len() as u32,
            source_sha256: [1; 32],
            pixels_sha256: Sha256::digest(&pixels).into(),
            width: 5,
            height: 8,
            pixels: assets::FontPixels::Rgba8(pixels.into()),
        };
        let glyphs = ['A', 'B', '\u{fffd}'].map(|codepoint| assets::GlyphMetrics {
            codepoint,
            page: 0,
            uv,
            bearing: [0, -7],
            advance_64,
        });
        let identity = [9; 32];
        let bytes = assets::encode_font_catalog(identity, &glyphs, &[page]).unwrap();
        RuntimeFontCatalog::decode(&bytes, identity).unwrap()
    }

    #[test]
    fn exclusive_glyph_bounds_preserve_every_stem_texel_and_continuous_bold_ink() {
        let font = stem_font_with_uv([0, 0, 2, 2], 4 * 64);
        let scaled = font.with_glyphs(
            &[assets::SheetGlyph {
                metrics: *font.glyph('A').unwrap(),
                draw_size_64: [4 * 64; 2],
            }],
            |_| true,
        );
        for (font, ink_size) in [(&font, 2), (&scaled, 4)] {
            let mut layouts = TextLayoutCache::new(2, 1 << 20);
            let pages = |page| font_page(font, page);
            let palette = ui::FormattingPalette::default();
            let plain = rasterize("A", font, &mut layouts, &pages, &palette).unwrap();
            let bold = rasterize("§lA", font, &mut layouts, &pages, &palette).unwrap();
            let top = TEXT_BASELINE_64 / 64 - 7;
            for y in top..top + ink_size {
                for x in 0..ink_size {
                    let at = ((y * plain.0 + x) * 4) as usize;
                    assert_eq!(&plain.3[at..at + 4], &[255; 4]);
                }
                for x in 0..ink_size + FONT_DESIGN_PIXEL_TEXELS {
                    let at = ((y * bold.0 + x) * 4) as usize;
                    assert_eq!(&bold.3[at..at + 4], &[255; 4]);
                }
            }
        }
    }

    #[test]
    fn degenerate_exclusive_uvs_do_not_sample_a_scaled_glyph() {
        let font = stem_font();
        let mut metrics = *font.glyph('A').unwrap();
        metrics.uv = [0; 4];
        let font = font.with_glyphs(
            &[assets::SheetGlyph {
                metrics,
                draw_size_64: [4 * 64; 2],
            }],
            |_| true,
        );
        let raster = rasterize(
            "§lA",
            &font,
            &mut TextLayoutCache::new(1, 1 << 20),
            &|page| font_page(&font, page),
            &ui::FormattingPalette::default(),
        )
        .unwrap();
        assert!(raster.3.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn bold_nametags_expand_ink_and_line_width_and_reset_preserves_tint() {
        let font = stem_font();
        let mut layouts = TextLayoutCache::new(8, 1 << 20);
        let palette = ui::FormattingPalette::default();
        let pages = |page| font_page(&font, page);
        let plain = rasterize("AB", &font, &mut layouts, &pages, &palette).unwrap();
        let styled = rasterize("§e§lA§rB", &font, &mut layouts, &pages, &palette).unwrap();
        assert_eq!(styled.4 - plain.4, FONT_DESIGN_PIXEL_TEXELS);
        let pixel = |raster: &(u32, u32, i32, Vec<u8>, u32), x, y| {
            let index = ((y * raster.0 + x) * 4) as usize;
            <[u8; 4]>::try_from(&raster.3[index..index + 4]).unwrap()
        };
        let row = TEXT_BASELINE_64 / 64 - 7;
        assert_eq!(pixel(&plain, FONT_DESIGN_PIXEL_TEXELS, row)[3], 0);
        assert_eq!(
            pixel(&styled, FONT_DESIGN_PIXEL_TEXELS, row),
            [255, 255, 85, 255]
        );
        assert_eq!(pixel(&styled, 6 + FONT_DESIGN_PIXEL_TEXELS, row), [255; 4]);
        assert_eq!(pixel(&styled, 6 + 2 * FONT_DESIGN_PIXEL_TEXELS, row)[3], 0);

        let mut atlas = NametagAtlas::default();
        let plain = atlas
            .line(&Arc::from("AB"), &font, &mut layouts, &pages)
            .unwrap();
        let bold = atlas
            .line(&Arc::from("§lAB"), &font, &mut layouts, &pages)
            .unwrap();
        assert_eq!(bold.width_px - plain.width_px, 2.0);
    }

    #[test]
    fn bold_copies_composite_partial_glyph_coverage() {
        let font = stem_font();
        let coverage = [128; 5 * 8];
        let pages = |_| {
            Some(GlyphPage {
                width: 5,
                height: 8,
                pixels: GlyphPixels::Coverage(&coverage),
            })
        };
        let mut layouts = TextLayoutCache::new(2, 1 << 20);
        let raster = rasterize(
            "§lA",
            &font,
            &mut layouts,
            &pages,
            &ui::FormattingPalette::default(),
        )
        .unwrap();
        let row = TEXT_BASELINE_64 / 64 - 7;
        let index = ((row * raster.0 + FONT_DESIGN_PIXEL_TEXELS) * 4) as usize;
        assert_eq!(&raster.3[index..index + 4], &[255, 255, 255, 192]);
    }

    #[test]
    fn bold_multiline_nametag_grows_the_plate_and_retains_reset_line_centering() {
        let font = stem_font();
        let build = |name: &str| {
            super::super::nametags::build_nametag_scene(
                &[super::super::nametags::tests::anchor(name)],
                &font,
                &mut TextLayoutCache::new(4, 1 << 20),
                &mut NametagAtlas::default(),
                &|page| font_page(&font, page),
            )
        };
        let plain = build("AB\nA");
        let bold = build("§lAB\n§rA");
        assert_eq!(plain.records.len(), 3);
        assert_eq!(bold.records.len(), 3);
        assert_eq!(bold.records[0].rect[0], plain.records[0].rect[0] - 1.0);
        assert_eq!(bold.records[0].rect[2], plain.records[0].rect[2] + 1.0);
        assert_eq!(bold.records[0].color, plain.records[0].color);
        assert_eq!(bold.records[2].rect, plain.records[2].rect);
    }

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
    fn atlas_publication_preserves_full_glyph_coverage_and_line_padding() {
        let (mut atlas, _) = sample_atlas();
        for rectangle in atlas.publish().0.iter() {
            let row_bytes = rectangle.cell[2] as usize * 4;
            for (row, pixels) in rectangle.rgba8.chunks_exact(row_bytes).enumerate() {
                let expected = if row < ui::FONT_INK_TEXELS as usize {
                    [255; 4]
                } else {
                    [0; 4]
                };
                assert!(pixels.chunks_exact(4).all(|pixel| pixel == expected));
            }
        }
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

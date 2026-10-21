//! Name-tag lines rasterized at font resolution into the shared atlas the tag billboards sample.

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

use assets::RuntimeFontCatalog;
use render_model::{NAMETAG_ATLAS_SIDE, NametagAtlasRect};
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, TEXT_BASELINE_64, TEXT_BOLD_OFFSET_64, TEXT_LINE_HEIGHT_64,
    TextLayout, TextLayoutCache, TextLayoutRequest, TextStyle, TextWrap, UiScale,
};

mod badge;
mod packing;
mod raster;
use raster::{measure_line, rasterize_line};

/// Widest line laid out before it would wrap, in font texels.
const MAX_LINE_TEXELS: u32 = NAMETAG_ATLAS_SIDE;
/// Transparent texels on each side of a sampled cell.
const CELL_GUTTER_TEXELS: u32 = 1;
const CELL_PADDING_TEXELS: u32 = CELL_GUTTER_TEXELS * 2;

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
    /// Vertical glyph bounds in font pixels, before line-box padding.
    pub(super) ink_y_px: [f32; 2],
}

/// Shelf-packed cells with unused lines reclaimed only when a miss needs room.
#[derive(Default)]
pub struct NametagAtlas {
    cells: Vec<AtlasCell>,
    lines: HashMap<Arc<str>, usize>,
    shelves: Vec<AtlasShelf>,
    requested: HashSet<Arc<str>>,
    reclamation: Option<Vec<ShelfReclamation>>,
    exhausted: bool,
    frame_active: bool,
    /// Actual attempts include lines reached only after earlier misses leave draw budget.
    frame_lines: Vec<Arc<str>>,
    /// Suppresses repeated rebuilds for an order that has already tried fresh packing.
    rebuilt_lines: Option<Vec<Arc<str>>>,
    rebuild_pending: bool,
    rebuilt_this_frame: bool,
    prepared_texts: Vec<Arc<str>>,
    prepared_badge: bool,
    published: Option<Arc<[NametagAtlasRect]>>,
    revision: u64,
    palette: ui::FormattingPalette,
    obfuscation: Option<ui::ObfuscationGlyphs>,
    animation_seed: u64,
}

struct AtlasCell {
    /// `None` pins the shared gem; text cells participate in normal line eviction.
    text: Option<Arc<str>>,
    line: AtlasLine,
    /// Padded upload and packing bounds around the sampled line cell.
    allocation: [u32; 4],
    rgba8: Arc<[u8]>,
    shelf: usize,
    animation: Option<u64>,
}

#[derive(Clone)]
struct ShelfReclamation {
    shelves: Range<usize>,
    free: Vec<Range<u32>>,
    /// Height needed by requested cells after unused lines are removed.
    retained_height: u32,
    /// Spatially empty candidates cannot release a cached-line slot.
    has_unused_lines: bool,
}

struct AtlasShelf {
    y: u32,
    height: u32,
    free: Vec<Range<u32>>,
}

impl NametagAtlas {
    /// Resolves the predicted budget before any records capture UVs, retrying fragmentation once.
    pub(super) fn prepare_frame<'p>(
        &mut self,
        texts: &[Arc<str>],
        badge: bool,
        font: &RuntimeFontCatalog,
        layouts: &mut TextLayoutCache,
        pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
    ) {
        if texts != self.prepared_texts || badge != self.prepared_badge {
            self.rebuilt_lines = None;
        }
        self.begin_frame(texts.iter());
        for retry in [false, true] {
            if retry {
                if !self.rebuild_pending || self.rebuilt_lines.is_some() {
                    break;
                }
                self.reset();
                self.begin_frame(texts.iter());
                self.rebuilt_this_frame = true;
            }
            if badge {
                self.badge();
            }
            for text in texts {
                self.line(text, font, layouts, pages);
            }
        }
        self.frame_lines.clear();
        self.prepared_texts.clear();
        self.prepared_texts.extend_from_slice(texts);
        self.prepared_badge = badge;
    }

    /// Uses the UI frame's seed for same-width animated glyph substitutions.
    pub(super) fn set_animation_seed(&mut self, seed: u64) {
        self.animation_seed = seed;
    }
    /// Retires cached line pixels when the active formatting colors change.
    pub(super) fn set_palette(&mut self, palette: ui::FormattingPalette) {
        if self.palette != palette {
            self.reset();
            self.palette = palette;
        }
    }

    /// Starts before any draw captures UVs, rebuilding after an unsatisfied miss if needed.
    /// Protects all potential hits in the draw-budget prefix before reclaiming unused cells.
    /// A stable request order gets one fresh packing attempt, including late budget admissions.
    pub(super) fn begin_frame<'a>(&mut self, texts: impl Iterator<Item = &'a Arc<str>>) {
        if self
            .rebuilt_lines
            .as_ref()
            .is_some_and(|lines| lines != &self.frame_lines)
        {
            self.rebuilt_lines = None;
        }
        if self.rebuild_pending && self.rebuilt_lines.is_none() {
            let lines = std::mem::take(&mut self.frame_lines);
            self.reset();
            self.rebuilt_lines = Some(lines);
            self.rebuilt_this_frame = true;
        }
        self.frame_lines.clear();
        self.frame_active = true;
        self.rebuild_pending = false;
        self.requested.clear();
        self.requested.extend(texts.cloned());
        self.reclamation = None;
        self.exhausted = false;
    }

    /// Returns cached pixels or reserves a cell before rasterizing a new line.
    pub(super) fn line<'p>(
        &mut self,
        text: &Arc<str>,
        font: &RuntimeFontCatalog,
        layouts: &mut TextLayoutCache,
        pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
    ) -> Option<AtlasLine> {
        if self.frame_active {
            self.frame_lines.push(Arc::clone(text));
            if self.requested.insert(Arc::clone(text)) {
                // Rejected earlier lines can leave budget for a later, initially unpinned line.
                self.reclamation = None;
            }
        }
        if let Some(&index) = self.lines.get(text) {
            if self.cells[index]
                .animation
                .is_some_and(|seed| seed != self.animation_seed)
            {
                let measured = measure_line(text, font, layouts)?;
                let pixels = rasterize_line(
                    &measured,
                    pages,
                    &self.palette,
                    ui::TextEffects {
                        obfuscation: self.obfuscation.as_ref(),
                        obfuscation_seed: self.animation_seed,
                        ..Default::default()
                    },
                );
                let pixels = padded_pixels(&pixels, measured.size);
                let cell = &mut self.cells[index];
                if cell.rgba8.as_ref() != pixels {
                    cell.rgba8 = pixels.into();
                    self.published = None;
                }
                cell.animation = Some(self.animation_seed);
            }
            return Some(self.cells[index].line);
        }
        let measured = measure_line(text, font, layouts)?;
        let animated = measured
            .layout
            .glyphs()
            .iter()
            .any(|glyph| glyph.style.obfuscated);
        if animated && self.obfuscation.is_none() {
            self.obfuscation = Some(ui::ObfuscationGlyphs::from_catalog(font));
        }
        let ([width, height], inset) = padded_cell(measured.size);
        // Pixel exhaustion signals a later caller rebuild; smaller misses still try to fit.
        if self.lines.len() >= MAX_ATLAS_LINES && !self.reclaim_shelf(None) {
            self.rebuild_pending |= self.frame_active;
            return None;
        }
        let mut placement = self.allocate(width, height);
        if placement.is_none() && self.reclaim_shelf(Some([width, height])) {
            placement = self.allocate(width, height);
        }
        let Some((shelf, origin)) = placement else {
            // Keep this latch separate from exploratory allocation and later successful misses.
            self.rebuild_pending |= self.frame_active;
            return None;
        };
        let pixels = rasterize_line(
            &measured,
            pages,
            &self.palette,
            ui::TextEffects {
                obfuscation: self.obfuscation.as_ref(),
                obfuscation_seed: self.animation_seed,
                ..Default::default()
            },
        );
        let rgba8 = padded_pixels(&pixels, measured.size);
        let line = AtlasLine {
            cell: [
                origin[0] + inset[0],
                origin[1] + inset[1],
                measured.size[0],
                measured.size[1],
            ],
            width_px: measured.advance as f32 / FONT_DESIGN_PIXEL_TEXELS as f32,
            top_px: measured.top as f32 / FONT_DESIGN_PIXEL_TEXELS as f32,
            ink_y_px: measured
                .ink_y
                .map(|value| value as f32 / FONT_DESIGN_PIXEL_TEXELS as f32),
        };
        self.reclamation = None;
        self.lines.insert(Arc::clone(text), self.cells.len());
        self.cells.push(AtlasCell {
            text: Some(Arc::clone(text)),
            line,
            allocation: [origin[0], origin[1], width, height],
            rgba8: rgba8.into(),
            shelf,
            animation: animated.then_some(self.animation_seed),
        });
        self.published = None;
        Some(line)
    }

    /// Invalidates all cells when font or palette identity changes.
    pub(super) fn reset(&mut self) {
        self.lines.clear();
        self.cells.clear();
        self.shelves.clear();
        self.requested.clear();
        self.reclamation = None;
        self.exhausted = false;
        self.frame_active = false;
        self.frame_lines.clear();
        self.rebuilt_lines = None;
        self.rebuild_pending = false;
        self.published = None;
        self.obfuscation = None;
        self.rebuilt_this_frame = false;
        self.prepared_texts.clear();
        self.prepared_badge = false;
    }

    /// Checks capacity for uncached lines only; callers must exclude their hits.
    pub(super) fn has_room_for(&self, misses: usize) -> bool {
        // Callers without frame liveness still need their explicit pixel-exhaustion reset.
        let exhausted = self.exhausted && self.requested.is_empty();
        misses == 0 || (!exhausted && self.lines.len().saturating_add(misses) <= MAX_ATLAS_LINES)
    }

    /// Retains immutable line pixels so skipped extractions can recover every update.
    pub(super) fn publish(&mut self) -> (Arc<[NametagAtlasRect]>, u64) {
        if self.rebuilt_this_frame {
            self.rebuilt_lines = Some(self.frame_lines.clone());
            self.rebuilt_this_frame = false;
        }
        if self.published.is_none() {
            self.revision += 1;
            self.published = Some(
                self.cells
                    .iter()
                    .map(|cell| NametagAtlasRect {
                        cell: cell.allocation,
                        rgba8: Arc::clone(&cell.rgba8),
                    })
                    .collect(),
            );
        }
        (
            Arc::clone(self.published.as_ref().expect("just published")),
            self.revision,
        )
    }
}

/// Pads bounded inner dimensions; full-span edges use the atlas sampler's existing clamp.
fn padded_cell(size: [u32; 2]) -> ([u32; 2], [u32; 2]) {
    let outer = size.map(|value| {
        value
            .saturating_add(CELL_PADDING_TEXELS)
            .min(NAMETAG_ATLAS_SIDE)
    });
    let inset = std::array::from_fn(|axis| (outer[axis] - size[axis]) / 2);
    (outer, inset)
}

/// Clears every allocated gutter while retaining the entire admitted inner raster.
fn padded_pixels(pixels: &[u8], size @ [width, _]: [u32; 2]) -> Vec<u8> {
    let ([outer_width, outer_height], [left, top]) = padded_cell(size);
    let stride = outer_width as usize * 4;
    let mut padded = vec![0; stride * outer_height as usize];
    for (row, source) in pixels.chunks_exact(width as usize * 4).enumerate() {
        let start = (row + top as usize) * stride + left as usize * 4;
        padded[start..start + source.len()].copy_from_slice(source);
    }
    padded
}

/// Measured pen width and glyph bounds without reserving atlas pixels.
pub(super) struct LineMetrics {
    pub width_px: f32,
    pub rect: [f32; 4],
    pub ink_y_px: [f32; 2],
}

/// Measures a slot independently of whether its pixels fit this frame.
pub(super) fn line_metrics(
    text: &str,
    font: &RuntimeFontCatalog,
    layouts: &mut TextLayoutCache,
) -> Option<LineMetrics> {
    let measured = measure_line(text, font, layouts)?;
    let texels = FONT_DESIGN_PIXEL_TEXELS as f32;
    Some(LineMetrics {
        width_px: measured.advance as f32 / texels,
        rect: [
            0.0,
            measured.top as f32 / texels,
            measured.size[0] as f32 / texels,
            (measured.top as f32 + measured.size[1] as f32) / texels,
        ],
        ink_y_px: measured.ink_y.map(|v| v as f32 / texels),
    })
}

/// Retained line bound, far above any frame's visible tags.
const MAX_ATLAS_LINES: usize = 4096;

#[cfg(test)]
mod tests {
    mod formatting;
    use super::*;
    use sha2::{Digest, Sha256};

    /// Rasterizes measured ink for the glyph-coverage assertions below.
    fn rasterize<'p>(
        text: &str,
        font: &RuntimeFontCatalog,
        layouts: &mut TextLayoutCache,
        pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
        palette: &ui::FormattingPalette,
    ) -> Option<(u32, u32, i32, Vec<u8>, u32)> {
        let measured = measure_line(text, font, layouts)?;
        let pool = ui::ObfuscationGlyphs::from_catalog(font);
        let pixels = rasterize_line(
            &measured,
            pages,
            palette,
            ui::TextEffects {
                obfuscation: Some(&pool),
                ..Default::default()
            },
        );
        Some((
            measured.size[0],
            measured.size[1],
            measured.top,
            pixels,
            measured.advance,
        ))
    }

    /// Supplies a small font with predictable stem coverage.
    fn stem_font() -> RuntimeFontCatalog {
        stem_font_with_uv([0, 0, 4, 8], 6 * 64)
    }

    /// Supplies a small font with chosen glyph bounds and advance.
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
    fn bold_multiline_nametag_grows_only_its_lines_plate_and_retains_reset_line_centering() {
        let font = stem_font();
        let build = |name: &str| {
            super::super::nametags::build_nametag_scene(
                &[super::super::nametags::tests::anchor(name)],
                None,
                &font,
                &mut TextLayoutCache::new(4, 1 << 20),
                &mut NametagAtlas::default(),
                &|page| font_page(&font, page),
            )
        };
        let plain = build("AB\nA");
        let bold = build("§lAB\n§rA");
        assert_eq!(plain.records.len(), 4);
        assert_eq!(bold.records.len(), 4);
        assert_eq!(bold.records[0].rect[0], plain.records[0].rect[0] - 1.0);
        assert_eq!(bold.records[0].rect[2], plain.records[0].rect[2] + 1.0);
        assert_eq!(bold.records[0].color, plain.records[0].color);
        assert_eq!(bold.records[1].rect, plain.records[1].rect);
        assert_eq!(bold.records[3].rect, plain.records[3].rect);
        for scene in [&plain, &bold] {
            assert!(scene.records.iter().all(|record| record.depth_tested == 0));
            assert!(
                scene.records[2..]
                    .iter()
                    .all(|record| record.color == [1.0; 4])
            );
        }
    }

    /// Builds the labels used by the original full-atlas timing fixture.
    fn sample_atlas() -> (NametagAtlas, std::time::Duration) {
        let font = ui_engine::test_support::fixture_font();
        let mut layouts = TextLayoutCache::new(256, 1 << 20);
        let mut atlas = NametagAtlas::default();
        let started = web_time::Instant::now();
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
                for (column, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
                    let ink = row > 0
                        && row <= ui::FONT_INK_TEXELS as usize
                        && column > 0
                        && column + 1 < rectangle.cell[2] as usize;
                    assert_eq!(*pixel, if ink { [255; 4] } else { [0; 4] });
                }
            }
        }
    }

    /// Applies the same dirty rectangles submitted by the renderer.
    pub(super) fn apply_rectangles(
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
        let font = ui_engine::test_support::fixture_font();
        let mut layouts = TextLayoutCache::new(8, 1 << 20);
        let mut atlas = NametagAtlas::default();
        let text = Arc::from("§2Player");
        let pages = |page| font_page(&font, page);
        atlas.line(&text, &font, &mut layouts, &pages).unwrap();
        let first = atlas.publish().0;
        assert!(
            first[0]
                .rgba8
                .as_chunks::<4>()
                .0
                .contains(&[0, 170, 0, 255])
        );
        atlas.set_palette(ui::FormattingPalette::from_globals(|name| {
            (name == "$2_color_format").then_some([0.976, 0.859, 0.427])
        }));
        atlas.line(&text, &font, &mut layouts, &pages).unwrap();
        let changed = atlas.publish().0;
        assert!(
            changed[0]
                .rgba8
                .as_chunks::<4>()
                .0
                .contains(&[249, 219, 109, 255])
        );
        assert_ne!(first[0].rgba8, changed[0].rgba8);
    }

    #[test]
    fn rectangles_preserve_skipped_publications_reset_and_old_readers() {
        let font = ui_engine::test_support::fixture_font();
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
    fn retained_text_without_liveness_keeps_the_explicit_reset_fallback() {
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

#[cfg(test)]
#[path = "nametag_atlas/eviction_tests.rs"]
mod eviction_tests;

#[cfg(test)]
#[path = "nametag_atlas/churn_tests.rs"]
mod churn_tests;

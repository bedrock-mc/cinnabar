use super::*;

/// Supplies distinct same-width coverage rasters, including one private-use icon.
fn coverage_font() -> RuntimeFontCatalog {
    let mut pixels = vec![0; 6 * 8];
    for row in 0..8 {
        pixels[row * 6] = 255;
        pixels[row * 6 + 3] = 255;
        pixels[row * 6 + 4..row * 6 + 6].fill(255);
    }
    let page = assets::FontTexturePage {
        source_path: "font/formatting.png".into(),
        source_bytes: pixels.len() as u32,
        source_sha256: [1; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 6,
        height: 8,
        pixels: assets::FontPixels::Coverage(pixels.into()),
    };
    let glyphs = ['A', 'B', '\u{e000}', '\u{fffd}'].map(|codepoint| {
        let left = match codepoint {
            'A' => 0,
            'B' => 2,
            _ => 4,
        };
        assets::GlyphMetrics {
            codepoint,
            page: 0,
            uv: [left, 0, left + 2, 8],
            bearing: [0, -7],
            advance_64: 3 * 64,
        }
    });
    let identity = [4; 32];
    RuntimeFontCatalog::decode(
        &assets::encode_font_catalog(identity, &glyphs, &[page]).unwrap(),
        identity,
    )
    .unwrap()
}

#[test]
fn private_use_coverage_icons_keep_white_ink_under_formatting_colors() {
    let font = coverage_font();
    let raster = rasterize(
        "§cA\u{e000}",
        &font,
        &mut TextLayoutCache::new(1, 1 << 20),
        &|page| font_page(&font, page),
        &ui::FormattingPalette::default(),
    )
    .unwrap();
    let pixels: Vec<_> = raster
        .3
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] != 0)
        .collect();
    assert!(pixels.iter().any(|pixel| pixel[..3] != [255; 3]));
    assert!(pixels.contains(&&[255; 4]));
}

#[test]
fn obfuscated_runs_use_same_width_substitutions() {
    let font = coverage_font();
    let pages = |page| font_page(&font, page);
    let mut layouts = TextLayoutCache::new(2, 1 << 20);
    let palette = ui::FormattingPalette::default();
    let plain = rasterize("AAAAAAAAAAAAAAAA", &font, &mut layouts, &pages, &palette).unwrap();
    let obfuscated =
        rasterize("§kAAAAAAAAAAAAAAAA", &font, &mut layouts, &pages, &palette).unwrap();
    assert_eq!(
        (plain.0, plain.1, plain.4),
        (obfuscated.0, obfuscated.1, obfuscated.4)
    );
    assert_ne!(plain.3, obfuscated.3);
}

#[test]
fn obfuscation_animates_in_place_without_churning_static_cells_or_layouts() {
    let font = coverage_font();
    let mut atlas = NametagAtlas::default();
    let mut layouts = TextLayoutCache::new(4, 1 << 20);
    let pages = |page| font_page(&font, page);
    let animated = Arc::from("§kA");
    let plain = Arc::from("A");
    let static_line = atlas.line(&plain, &font, &mut layouts, &pages).unwrap();
    let static_pixels = Arc::clone(&atlas.cells[atlas.lines[&plain]].rgba8);
    let line = atlas.line(&animated, &font, &mut layouts, &pages).unwrap();
    let first = atlas.publish().0;
    let mut changed = false;
    for seed in 1..32 {
        atlas.set_animation_seed(seed);
        assert_eq!(
            atlas.line(&animated, &font, &mut layouts, &pages),
            Some(line)
        );
        let (published, revision) = atlas.publish();
        changed |= published[atlas.lines[&animated]].rgba8 != first[atlas.lines[&animated]].rgba8;
        assert_eq!(
            atlas.line(&animated, &font, &mut layouts, &pages),
            Some(line)
        );
        assert_eq!(atlas.publish().1, revision);
        assert_eq!(
            atlas.line(&plain, &font, &mut layouts, &pages),
            Some(static_line)
        );
        assert!(Arc::ptr_eq(
            &static_pixels,
            &atlas.cells[atlas.lines[&plain]].rgba8
        ));
        assert_eq!(atlas.cells.len(), 2);
    }
    assert!(changed);
    assert_eq!(layouts.built_layout_count(), 2);
}

#[test]
fn italic_rasters_slant_without_changing_the_advance() {
    let font = stem_font();
    let pages = |page| font_page(&font, page);
    let mut layouts = TextLayoutCache::new(4, 1 << 20);
    let palette = ui::FormattingPalette::default();
    let plain = rasterize("A", &font, &mut layouts, &pages, &palette).unwrap();
    let italic = rasterize("§oA", &font, &mut layouts, &pages, &palette).unwrap();
    assert_eq!(plain.4, italic.4);
    assert_ne!(plain.3, italic.3);
    let first_ink = |row: u32| {
        (0..italic.0)
            .find(|x| italic.3[((row * italic.0 + x) * 4 + 3) as usize] != 0)
            .unwrap()
    };
    let top = TEXT_BASELINE_64 / 64 - 7;
    assert!(first_ink(top) > first_ink(top + 7));
}

#[test]
fn leading_negative_bearings_preserve_every_ink_texel() {
    let font = stem_font();
    let mut metrics = *font.glyph('A').unwrap();
    metrics.bearing[0] = -2;
    let font = font.with_glyphs(
        &[assets::SheetGlyph {
            metrics,
            draw_size_64: [4 * 64, 8 * 64],
        }],
        |_| true,
    );
    let (_, _, _, pixels, _) = rasterize(
        "A",
        &font,
        &mut TextLayoutCache::new(1, 1 << 20),
        &|page| font_page(&font, page),
        &ui::FormattingPalette::default(),
    )
    .unwrap();
    assert_eq!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] != 0)
            .count(),
        16
    );
}

#[test]
fn published_cells_have_transparent_edges_after_reuse() {
    let font = stem_font();
    let mut atlas = NametagAtlas::default();
    let mut layouts = TextLayoutCache::new(2, 1 << 20);
    for text in [Arc::from("A"), Arc::from("§lB")] {
        atlas.begin_frame(std::iter::once(&text));
        atlas
            .line(&text, &font, &mut layouts, &|page| font_page(&font, page))
            .unwrap();
        for cell in atlas.publish().0.iter() {
            let [_, _, width, height] = cell.cell;
            for y in 0..height {
                for x in 0..width {
                    if x == 0 || y == 0 || x + 1 == width || y + 1 == height {
                        assert_eq!(cell.rgba8[((y * width + x) * 4 + 3) as usize], 0);
                    }
                }
            }
        }
    }
}

#[test]
fn atlas_edge_cells_keep_the_original_line_width_and_clear_possible_neighbors() {
    let base = stem_font();
    let coverage = [255; 5 * 8];
    let pages = |_| {
        Some(GlyphPage {
            width: 5,
            height: 8,
            pixels: GlyphPixels::Coverage(&coverage),
        })
    };
    let mut metrics = *base.glyph('A').unwrap();
    metrics.bearing[1] = -(TEXT_BASELINE_64 as i16 / 64);
    for width in [NAMETAG_ATLAS_SIDE - 1, NAMETAG_ATLAS_SIDE] {
        let font = base.with_glyphs(
            &[assets::SheetGlyph {
                metrics,
                draw_size_64: [width * 64, 8 * 64],
            }],
            |_| true,
        );
        let mut atlas = NametagAtlas::default();
        let line = atlas
            .line(
                &Arc::from("A"),
                &font,
                &mut TextLayoutCache::new(1, 1 << 20),
                &pages,
            )
            .unwrap();
        assert_eq!(line.cell[0], 0);
        assert_eq!(line.cell[2], width);
        let (published, _) = atlas.publish();
        let cell = &published[0];
        assert_eq!(cell.cell[2], NAMETAG_ATLAS_SIDE);
        let row = line.cell[1] - cell.cell[1];
        let last_ink = ((row * cell.cell[2] + width - 1) * 4 + 3) as usize;
        assert_eq!(cell.rgba8[last_ink], 255);
        if width < NAMETAG_ATLAS_SIDE {
            assert_eq!(cell.rgba8[last_ink + 4], 0);
        }
    }
}

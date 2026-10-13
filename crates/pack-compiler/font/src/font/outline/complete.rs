//! Bounded compilation of every Unicode scalar mapped by one outline face.

use super::*;
use std::collections::BTreeMap;

/// Admits the union of provider character maps, keeping primary artwork on overlap.
pub(super) fn compile(
    sources: &[(&Path, &[u8], &Font)],
    manifest: [u8; 32],
    config: OutlineFontConfig,
) -> Result<CompiledFontCarrier, FontCompileError> {
    let mut codepoints = BTreeMap::new();
    codepoints.insert(
        config.replacement_codepoint,
        (
            0,
            config.replacement_codepoint,
            mathematical::Style::default(),
        ),
    );
    for (provider, (_, _, font)) in sources.iter().enumerate() {
        for &codepoint in font.chars().keys() {
            codepoints.entry(codepoint).or_insert((
                provider,
                codepoint,
                mathematical::Style::default(),
            ));
            if codepoints.len() > assets::MAX_FONT_GLYPHS {
                return Err(invalid("outline character maps exceed the glyph bound"));
            }
        }
    }
    if config.synthesize_mathematical_letters {
        for (codepoint, base, style) in mathematical::variants() {
            if sources[0].2.lookup_glyph_index(base) != 0 {
                codepoints.entry(codepoint).or_insert((0, base, style));
                if codepoints.len() > assets::MAX_FONT_GLYPHS {
                    return Err(invalid("mathematical fallback exceeds the glyph bound"));
                }
            }
        }
    }
    let mut charge = 0;
    let mut selected = Vec::with_capacity(codepoints.len());
    for (codepoint, (provider, base, style)) in codepoints {
        let font = sources[provider].2;
        let metrics = if font.lookup_glyph_index(base) == 0 {
            let width = config.pixel_height * 5 / 8;
            let height = config.pixel_height * 7 / 8;
            charge = providers::add_charge(charge, (width * height * 2) as usize)?;
            None
        } else {
            let metrics = font.metrics(base, config.pixel_height as f32);
            charge = providers::add_charge(charge, providers::metric_charge(codepoint, &metrics)?)?;
            charge = providers::add_charge(charge, style.charge(&metrics))?;
            Some(metrics)
        };
        selected.push((codepoint, provider, base, style, metrics));
    }
    let mut rasterized: Vec<Vec<RasterizedGlyph>> =
        (0..sources.len()).map(|_| Vec::new()).collect();
    for (codepoint, provider, base, style, metrics) in selected {
        let mut glyph = match metrics {
            Some(metrics) => rasterize_checked(
                sources[provider].2,
                base,
                config.pixel_height,
                if provider == 0 {
                    config.advances
                } else {
                    GlyphAdvances::Source
                },
                Some(&metrics),
            )?,
            None => synthetic_replacement(config.pixel_height)?,
        };
        glyph.codepoint = codepoint;
        if let Some(bearing) = config.ascii_bearing {
            glyph.bearing[0] = glyph.bearing[0]
                .checked_add(bearing.offset(base))
                .ok_or_else(|| metric_error(codepoint, "ASCII bearing"))?;
        }
        if matches!(codepoint, ' ' | '\u{a0}')
            && let Some(advance) = config.space_advance_64
        {
            glyph.advance_64 = advance;
        }
        style.apply(&mut glyph)?;
        rasterized[provider].push(glyph);
    }
    let mut glyphs = Vec::new();
    let mut pages = Vec::new();
    let mut source_total = 0_u64;
    let mut decoded_bytes = 0_u64;
    for (provider, rasterized) in rasterized.iter_mut().enumerate() {
        // Tallest-first shelves keep atlas size independent of Unicode block order.
        rasterized.sort_unstable_by_key(|glyph| (std::cmp::Reverse(glyph.height), glyph.codepoint));
        let side = if provider == 0 {
            config.atlas_side
        } else {
            config.fallback_atlas_side()
        };
        let ranges = page_ranges(rasterized, side)?;
        let (source_path, source_bytes, _) = sources[provider];
        source_total += source_bytes.len() as u64 * ranges.len() as u64;
        decoded_bytes += coverage_len(side)? as u64 * ranges.len() as u64;
        if source_total > MAX_FONT_SOURCE_BYTES {
            return Err(FontCompileError::SourceTooLarge {
                path: source_path.into(),
            });
        }
        if decoded_bytes > MAX_FONT_SOURCE_BYTES
            || pages.len() + ranges.len() > assets::MAX_FONT_PAGES
        {
            return Err(invalid("complete outline pages exceed carrier bounds"));
        }
        for (index, (first, last)) in ranges.into_iter().enumerate() {
            let (mut next, pixels) = pack(&rasterized[first..last], side)?;
            for glyph in &mut next {
                glyph.page = pages.len() as u16;
            }
            glyphs.extend(next);
            pages.push(providers::page(
                &format!(
                    "font/provider-{provider}-{}px-{index:03}.png",
                    config.pixel_height
                ),
                source_bytes,
                pixels,
                side,
            )?);
        }
    }
    glyphs.sort_unstable_by_key(|glyph| glyph.codepoint);
    let bytes = encode_font_catalog(manifest, &glyphs, &pages)?;
    let carrier_sha256 = bytes[bytes.len() - 32..]
        .try_into()
        .map_err(|_| invalid("carrier digest missing"))?;
    Ok(CompiledFontCarrier {
        report: FontCompileReport {
            schema: FONT_CARRIER_SCHEMA,
            glyphs: glyphs.len(),
            pages: pages.len(),
            source_bytes: source_total,
            decoded_bytes,
            source_manifest_sha256: manifest,
            carrier_sha256,
        },
        bytes,
    })
}

/// Splits shelves at page boundaries and rejects oversized glyphs before page allocation.
fn page_ranges(
    glyphs: &[RasterizedGlyph],
    side: u32,
) -> Result<Vec<(usize, usize)>, FontCompileError> {
    let mut ranges = Vec::new();
    let (mut first, mut x, mut y, mut row) = (0, ATLAS_PADDING, ATLAS_PADDING, 0);
    for (index, glyph) in glyphs.iter().enumerate() {
        if glyph.width + 2 * ATLAS_PADDING > side || glyph.height + 2 * ATLAS_PADDING > side {
            return Err(FontCompileError::OutlineAtlasFull { side });
        }
        if x + glyph.width + ATLAS_PADDING > side {
            x = ATLAS_PADDING;
            y += row + ATLAS_PADDING;
            row = 0;
        }
        if y + glyph.height + ATLAS_PADDING > side {
            ranges.push((first, index));
            if ranges.len() == assets::MAX_FONT_PAGES {
                return Err(invalid("complete outline exceeds page bound"));
            }
            first = index;
            x = ATLAS_PADDING;
            y = ATLAS_PADDING;
            row = 0;
        }
        x += glyph.width + ATLAS_PADDING;
        row = row.max(glyph.height);
    }
    if first < glyphs.len() {
        ranges.push((first, glyphs.len()));
    }
    Ok(ranges)
}

use super::*;
use std::mem::size_of;

const MAX_SELECTED: usize = 32_768;
const MAX_ALPHA_BYTES: usize = 32 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
const FALLBACK_SIDE: u32 = 2_048;
const MAX_FALLBACK_PAGES: usize = 3;
const FALLBACK_RANGES: &[(u32, u32)] = &[
    (0x2190, 0x23ff),
    (0x2460, 0x27bf),
    (0x3000, 0x30ff),
    (0x3400, 0x4dbf),
    (0x4e00, 0x9fff),
];

struct Selected {
    codepoint: char,
    primary: bool,
    metrics: Option<fontdue::Metrics>,
}

/// Primary outline artwork plus a bounded, secondary CJK provider.
/// All selected alpha/metadata buffers are admitted before the first raster call.
pub fn compile_outline_font_with_fallback(
    primary_path: &Path,
    primary_bytes: &[u8],
    fallback_path: &Path,
    fallback_bytes: &[u8],
    manifest: [u8; 32],
    config: OutlineFontConfig,
) -> Result<CompiledFontCarrier, FontCompileError> {
    validate_config(primary_bytes, manifest, config)?;
    if config.pixel_height != 18
        || config.atlas_side != 1_024
        || fallback_bytes.is_empty()
        || primary_bytes
            .len()
            .checked_add(fallback_bytes.len())
            .is_none_or(|n| n > 32 * 1024 * 1024)
    {
        return Err(invalid(
            "two-provider source/configuration exceeds its policy",
        ));
    }
    let parse = |path: &Path, bytes: &[u8]| {
        Font::from_bytes(bytes, FontSettings::default()).map_err(|detail| {
            FontCompileError::OutlineFont {
                path: path.to_path_buf(),
                detail: detail.to_string().into_boxed_str(),
            }
        })
    };
    let primary = parse(primary_path, primary_bytes)?;
    let fallback = parse(fallback_path, fallback_bytes)?;
    // The complete fixed range enumeration is itself bounded, before retention.
    let declared = REVIEWED_RANGES
        .iter()
        .chain(FALLBACK_RANGES)
        .try_fold(1usize, |n, &(a, z)| {
            n.checked_add(usize::try_from(z - a + 1).ok()?)
        })
        .ok_or_else(|| invalid("font scalar enumeration overflows"))?;
    if declared > MAX_SELECTED {
        return Err(invalid("font scalar enumeration exceeds policy"));
    }
    let metadata = declared
        .checked_mul(
            size_of::<char>()
                + size_of::<Selected>()
                + size_of::<RasterizedGlyph>()
                + 3 * size_of::<GlyphMetrics>(),
        )
        .and_then(|bytes| {
            bytes.checked_add(
                4 * (size_of::<FontTexturePage>() + assets::MAX_FONT_PATH_BYTES)
                    + 3 * size_of::<(usize, usize)>(),
            )
        })
        .ok_or_else(|| invalid("font metadata accounting overflows"))?;
    if metadata > MAX_METADATA_BYTES {
        return Err(invalid("font metadata exceeds policy"));
    }
    let mut scalars = Vec::new();
    scalars
        .try_reserve_exact(declared)
        .map_err(|_| invalid("font scalar allocation refused"))?;
    scalars.extend(
        REVIEWED_RANGES
            .iter()
            .chain(FALLBACK_RANGES)
            .flat_map(|&(a, z)| a..=z)
            .filter_map(char::from_u32),
    );
    scalars.push(REQUIRED_REPLACEMENT);
    scalars.sort_unstable();
    scalars.dedup();
    let mut selected = Vec::new();
    selected
        .try_reserve_exact(scalars.len())
        .map_err(|_| invalid("font preflight allocation refused"))?;
    let mut alpha_charge = 0usize;
    for codepoint in scalars {
        let in_primary = REVIEWED_RANGES
            .iter()
            .any(|&(a, z)| (a..=z).contains(&u32::from(codepoint)));
        let is_replacement = codepoint == REQUIRED_REPLACEMENT;
        let use_primary =
            (in_primary || is_replacement) && primary.lookup_glyph_index(codepoint) != 0;
        let use_fallback = !is_replacement
            && !use_primary
            && FALLBACK_RANGES
                .iter()
                .any(|&(a, z)| (a..=z).contains(&u32::from(codepoint)))
            && fallback.lookup_glyph_index(codepoint) != 0;
        if !use_primary && !use_fallback && !is_replacement {
            continue;
        }
        let metrics = if is_replacement && !use_primary {
            // Matches the existing primary synthetic replacement, with no bitmap yet.
            alpha_charge = add_charge(alpha_charge, 2 * 11 * 15)?;
            None
        } else {
            let font = if use_primary { &primary } else { &fallback };
            let metrics = font.metrics(codepoint, 18.0);
            alpha_charge = add_charge(alpha_charge, metric_charge(codepoint, &metrics)?)?;
            Some(metrics)
        };
        selected.push(Selected {
            codepoint,
            primary: use_primary || is_replacement,
            metrics,
        });
    }
    let mut primary_glyphs = Vec::new();
    let mut fallback_glyphs = Vec::new();
    primary_glyphs
        .try_reserve_exact(selected.iter().filter(|s| s.primary).count())
        .map_err(|_| invalid("primary glyph allocation refused"))?;
    fallback_glyphs
        .try_reserve_exact(selected.iter().filter(|s| !s.primary).count())
        .map_err(|_| invalid("fallback glyph allocation refused"))?;
    // No raster call above this point. Both vectors' alpha and metadata are admitted.
    for selected in selected {
        let glyph = match selected.metrics {
            None => synthetic_replacement(18)?,
            Some(metrics) => rasterize_checked(
                if selected.primary {
                    &primary
                } else {
                    &fallback
                },
                selected.codepoint,
                18,
                if selected.primary {
                    config.advances
                } else {
                    GlyphAdvances::Source
                },
                Some(&metrics),
            )?,
        };
        let bound = selected
            .metrics
            .map_or(11 * 15, |m| (m.width * m.height).max(1));
        if glyph.alpha.len() > bound || glyph.alpha.len() != (glyph.width * glyph.height) as usize {
            return Err(invalid("trimmed glyph exceeds its admitted buffer"));
        }
        if selected.primary {
            primary_glyphs.push(glyph);
        } else {
            fallback_glyphs.push(glyph);
        }
    }
    let ranges = page_ranges(&fallback_glyphs)?;
    let source_total = fallback_bytes
        .len()
        .checked_mul(ranges.len())
        .and_then(|n| n.checked_add(primary_bytes.len()))
        .ok_or_else(|| invalid("page source accounting overflows"))?;
    let decoded_total = ranges
        .len()
        .checked_mul(FALLBACK_SIDE as usize * FALLBACK_SIDE as usize * 4)
        .and_then(|n| n.checked_add(1_024 * 1_024 * 4))
        .ok_or_else(|| invalid("page pixel accounting overflows"))?;
    if source_total > MAX_FONT_SOURCE_BYTES as usize || decoded_total > 64 * 1024 * 1024 {
        return Err(invalid("font pages exceed carrier byte policy"));
    }
    let (mut glyphs, pixels) = pack(&primary_glyphs, config.atlas_side)?;
    glyphs
        .try_reserve_exact(fallback_glyphs.len())
        .map_err(|_| invalid("glyph table allocation refused"))?;
    let mut pages = vec![page("font/atlas-18px.png", primary_bytes, pixels, 1_024)?];
    for (first, last) in ranges {
        let (mut next, pixels) = pack(&fallback_glyphs[first..last], FALLBACK_SIDE)?;
        let index = u16::try_from(pages.len()).map_err(|_| invalid("font page index overflows"))?;
        for glyph in &mut next {
            glyph.page = index;
        }
        glyphs.extend(next);
        pages.push(page(
            &format!("font/fallback-cjk-18px-{:03}.png", pages.len() - 1),
            fallback_bytes,
            pixels,
            FALLBACK_SIDE,
        )?);
    }
    glyphs.sort_unstable_by_key(|glyph| glyph.codepoint);
    let source_bytes = pages.iter().map(|p| u64::from(p.source_bytes)).sum();
    let decoded_bytes = pages.iter().map(|p| p.pixels.bytes().len() as u64).sum();
    let bytes = encode_font_catalog(manifest, &glyphs, &pages)?;
    let carrier_sha256 = bytes[bytes.len() - 32..]
        .try_into()
        .map_err(|_| invalid("carrier digest missing"))?;
    Ok(CompiledFontCarrier {
        report: FontCompileReport {
            schema: FONT_CARRIER_SCHEMA,
            glyphs: glyphs.len(),
            pages: pages.len(),
            source_bytes,
            decoded_bytes,
            source_manifest_sha256: manifest,
            carrier_sha256,
        },
        bytes,
    })
}

fn add_charge(previous: usize, amount: usize) -> Result<usize, FontCompileError> {
    previous
        .checked_add(amount)
        .filter(|n| *n <= MAX_ALPHA_BYTES)
        .ok_or_else(|| invalid("font alpha scratch exceeds policy"))
}

fn metric_charge(codepoint: char, metrics: &fontdue::Metrics) -> Result<usize, FontCompileError> {
    let pixels = metrics
        .width
        .checked_mul(metrics.height)
        .filter(|n| *n <= 4_096)
        .ok_or_else(|| metric_error(codepoint, "bitmap"))?;
    if metrics.width > 64
        || metrics.height > 64
        || !metrics.advance_width.is_finite()
        || !metrics.advance_height.is_finite()
        || !metrics.bounds.xmin.is_finite()
        || !metrics.bounds.ymin.is_finite()
        || !metrics.bounds.width.is_finite()
        || !metrics.bounds.height.is_finite()
        || !(f64::from(metrics.advance_width).round() * 64.0).is_finite()
        || (f64::from(metrics.advance_width).round() * 64.0) < f64::from(i16::MIN)
        || (f64::from(metrics.advance_width).round() * 64.0) > f64::from(i16::MAX)
        || i16::try_from(metrics.xmin).is_err()
        || metrics
            .xmin
            .checked_add(metrics.width as i32)
            .is_none_or(|x| i16::try_from(x).is_err())
        || metrics
            .ymin
            .checked_neg()
            .is_none_or(|y| i16::try_from(y).is_err())
        || metrics
            .ymin
            .checked_add(metrics.height as i32)
            .and_then(i32::checked_neg)
            .is_none_or(|y| i16::try_from(y).is_err())
    {
        return Err(metric_error(codepoint, "preflight"));
    }
    pixels
        .checked_add(pixels.max(1))
        .ok_or_else(|| invalid("alpha accounting overflows"))
}

fn page(
    path: &str,
    source: &[u8],
    rgba8: Box<[u8]>,
    side: u32,
) -> Result<FontTexturePage, FontCompileError> {
    Ok(FontTexturePage {
        source_path: path.into(),
        source_bytes: u32::try_from(source.len())
            .map_err(|_| invalid("font source length overflows"))?,
        source_sha256: Sha256::digest(source).into(),
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: side,
        height: side,
        pixels: FontPixels::Rgba8(rgba8),
    })
}

fn page_ranges(glyphs: &[RasterizedGlyph]) -> Result<Vec<(usize, usize)>, FontCompileError> {
    let mut ranges = Vec::with_capacity(MAX_FALLBACK_PAGES);
    let (mut first, mut x, mut y, mut row) = (0, ATLAS_PADDING, ATLAS_PADDING, 0);
    for (index, glyph) in glyphs.iter().enumerate() {
        if glyph.width > 64 || glyph.height > 64 {
            return Err(invalid("fallback glyph does not fit its page"));
        }
        if x + glyph.width + 1 > FALLBACK_SIDE {
            x = 1;
            y = y
                .checked_add(row + 1)
                .ok_or_else(|| invalid("page cursor overflows"))?;
            row = 0;
        }
        if y + glyph.height + 1 > FALLBACK_SIDE {
            ranges.push((first, index));
            if ranges.len() == MAX_FALLBACK_PAGES {
                return Err(invalid("fallback requires a fourth page"));
            }
            first = index;
            x = 1;
            y = 1;
            row = 0;
        }
        x += glyph.width + 1;
        row = row.max(glyph.height);
    }
    if first < glyphs.len() {
        ranges.push((first, glyphs.len()));
    }
    Ok(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_charge_checks_cumulative_overflow_before_raster() {
        assert!(add_charge(MAX_ALPHA_BYTES, 1).is_err());
        assert!(add_charge(usize::MAX, 1).is_err());
        assert_eq!(add_charge(MAX_ALPHA_BYTES - 1, 1).unwrap(), MAX_ALPHA_BYTES);
    }
    #[test]
    fn blank_metrics_and_hostile_dimensions_have_explicit_preflight_bounds() {
        let mut metrics = fontdue::Metrics::default();
        assert_eq!(metric_charge(' ', &metrics).unwrap(), 1);
        metrics.width = 65;
        assert!(metric_charge('X', &metrics).is_err());
        metrics.width = usize::MAX;
        metrics.height = 2;
        assert!(metric_charge('X', &metrics).is_err());
        metrics = fontdue::Metrics::default();
        metrics.bounds.width = f32::NAN;
        assert!(metric_charge('X', &metrics).is_err());
    }

    #[test]
    fn selected_glyphs_are_never_pruned_to_fit_three_pages() {
        let glyph = || RasterizedGlyph {
            codepoint: '世',
            width: 64,
            height: 64,
            bearing: [0, 0],
            advance_64: 18 * 64,
            alpha: Box::default(),
        };
        // 31 rows of 31 glyphs fit each page with the fixed one-pixel gaps.
        let glyphs: Vec<_> = (0..(31 * 31 * 3)).map(|_| glyph()).collect();
        assert_eq!(page_ranges(&glyphs).unwrap().len(), 3);
        let mut overflow = glyphs;
        overflow.push(glyph());
        assert!(page_ranges(&overflow).is_err());
    }
}

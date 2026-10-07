//! Bounded native fallback rasters, selected from the installed face priority chain.

use super::super::super::super::{FontCompileError, invalid};
use super::{ATLAS_PADDING, NATIVE_SDF_EM_PIXELS, RasterizedGlyph, atlas, freetype, sdf};
use assets::{FontPixels, FontRendering, FontTexturePage, RuntimeFontCatalog, encode_font_catalog};
use sha2::{Digest, Sha256};

/// Rasterizes only requested Unicode scalars, keeping primary-face glyphs out of the atlas.
pub fn compile_native_fallback_fonts(
    sources: &[&[u8]],
    codepoints: &[char],
    atlas_side: u32,
    maximum_pages: usize,
) -> Result<RuntimeFontCatalog, FontCompileError> {
    if sources.is_empty()
        || sources.len() > 16
        || codepoints.len() > assets::MAX_FONT_GLYPHS
        || !atlas_side.is_power_of_two()
        || !(256..=assets::FONT_FALLBACK_ATLAS_SIDE).contains(&atlas_side)
        || maximum_pages == 0
        || maximum_pages > assets::MAX_FONT_FALLBACK_PAGES
        || sources
            .iter()
            .any(|s| s.is_empty() || s.len() as u64 > assets::MAX_FONT_SOURCE_BYTES)
        || sources
            .iter()
            .try_fold(0u64, |total, source| total.checked_add(source.len() as u64))
            .is_none_or(|total| total > assets::MAX_FONT_SOURCE_BYTES)
    {
        return Err(invalid("native fallback sources exceed bounds"));
    }
    let mut hash = Sha256::new();
    let mut faces = Vec::new();
    for source in sources {
        hash.update(Sha256::digest(source));
        faces.push(freetype::Face::new(source, NATIVE_SDF_EM_PIXELS)?);
    }
    let identity = hash.finalize().into();
    let line = faces[0].line_metrics()?;
    let mut seen = std::collections::BTreeSet::new();
    let mut glyphs = Vec::new();
    let mut budget = RasterBudget::new(atlas_side, maximum_pages);
    for &ch in codepoints {
        if !seen.insert(ch) {
            continue;
        }
        if let Some(face) = faces.iter_mut().find(|face| face.has(ch)) {
            glyphs.push(budget.convert(face.rasterize(ch)?)?);
        }
    }
    if glyphs.is_empty() {
        return Err(invalid("native fallback has no supported glyphs"));
    }
    let atlas = atlas::pack_pages_bounded(glyphs, atlas_side, false, maximum_pages)?;
    catalog(identity, line, atlas)
}

struct RasterBudget {
    side: u32,
    remaining: usize,
}

impl RasterBudget {
    fn new(side: u32, maximum_pages: usize) -> Self {
        Self {
            side,
            remaining: side as usize * side as usize * maximum_pages,
        }
    }

    fn convert(&mut self, glyph: RasterizedGlyph) -> Result<RasterizedGlyph, FontCompileError> {
        let [width, height] = sdf::extent(glyph.width, glyph.height)
            .ok_or_else(|| invalid("native fallback glyph extent exceeds bounds"))?;
        if [width, height].into_iter().any(|extent| {
            extent
                .checked_add(ATLAS_PADDING * 2)
                .is_none_or(|extent| extent > self.side)
        }) {
            return Err(FontCompileError::OutlineAtlasFull { side: self.side });
        }
        let bytes = width as usize * height as usize;
        if bytes > self.remaining {
            return Err(invalid(
                "native fallback raster bytes exceed their page budget",
            ));
        }
        let glyph = sdf::glyph(glyph)?;
        self.remaining -= bytes;
        Ok(glyph)
    }
}

fn catalog(
    identity: [u8; 32],
    line: assets::FontLineMetrics,
    mut atlas: atlas::Atlas,
) -> Result<RuntimeFontCatalog, FontCompileError> {
    let atlas_side = atlas.side;
    atlas.glyphs.sort_unstable_by_key(|glyph| glyph.codepoint);
    let pages: Vec<_> = atlas
        .pages
        .into_iter()
        .enumerate()
        .map(|(index, pixels)| {
            let hash = Sha256::digest(&pixels).into();
            FontTexturePage {
                source_path: format!("font/runtime-fallback-{index:03}.png").into(),
                source_bytes: pixels.len() as u32,
                source_sha256: hash,
                pixels_sha256: hash,
                width: atlas_side,
                height: atlas_side,
                pixels: FontPixels::Rgba8(pixels),
            }
        })
        .collect();
    let bytes = encode_font_catalog(identity, &atlas.glyphs, &pages)?;
    Ok(RuntimeFontCatalog::decode(&bytes, identity)?
        .with_line_metrics(line)?
        .with_rendering(FontRendering::NativeSdf)
        .with_coverage_pages())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster(width: u32, height: u32) -> RasterizedGlyph {
        RasterizedGlyph {
            codepoint: 'A',
            width,
            height,
            bearing: [0, 0],
            advance_64: 64,
            alpha: vec![255; width as usize * height as usize].into_boxed_slice(),
        }
    }

    #[test]
    fn oversized_fallback_glyph_rejects_before_distance_field_allocation() {
        let mut budget = RasterBudget::new(256, 1);
        sdf::FIELD_ALLOCATIONS.with(|count| count.set(0));
        assert!(matches!(
            budget.convert(raster(249, 1)),
            Err(FontCompileError::OutlineAtlasFull { .. })
        ));
        assert_eq!(sdf::FIELD_ALLOCATIONS.with(std::cell::Cell::get), 0);
    }

    #[test]
    fn aggregate_fallback_raster_budget_stops_before_allocating_another_field() {
        let mut budget = RasterBudget::new(256, 1);
        sdf::FIELD_ALLOCATIONS.with(|count| count.set(0));
        for _ in 0..4 {
            let glyph = budget.convert(raster(118, 118)).unwrap();
            assert_eq!((glyph.width, glyph.height), (126, 126));
        }
        assert_eq!(sdf::FIELD_ALLOCATIONS.with(std::cell::Cell::get), 4);
        assert!(budget.convert(raster(118, 118)).is_err());
        assert_eq!(sdf::FIELD_ALLOCATIONS.with(std::cell::Cell::get), 4);
    }

    #[test]
    fn multi_digit_page_indices_keep_glyphs_and_texels_in_source_order() {
        let side = 256;
        let glyphs = ('A'..='K')
            .enumerate()
            .map(|(index, codepoint)| RasterizedGlyph {
                codepoint,
                width: 254,
                height: 254,
                bearing: [0, 0],
                advance_64: 64,
                alpha: vec![index as u8 + 1; 254 * 254].into_boxed_slice(),
            })
            .collect();
        let atlas = atlas::pack_pages_bounded(glyphs, side, false, 11).unwrap();
        let font = catalog(
            [1; 32],
            assets::FontLineMetrics {
                em_64: 52 * 64,
                ascent_64: 40 * 64,
                descent_64: 12 * 64,
            },
            atlas,
        )
        .unwrap();
        assert_eq!(font.pages().len(), 11);
        for (index, ch) in ('A'..='K').enumerate() {
            let glyph = font.glyph(ch).unwrap();
            let page = &font.pages()[usize::from(glyph.page)];
            let texel = usize::from(glyph.uv[1]) * page.width as usize + usize::from(glyph.uv[0]);
            assert_eq!(page.pixels.bytes()[texel], index as u8 + 1);
        }
    }
}

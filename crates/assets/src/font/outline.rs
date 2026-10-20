//! Bounded outline rasters with stable logical metrics for private native UI fonts.

use super::{FontPixels, FontTexturePage, RuntimeFontCatalog, encode_font_catalog};
use crate::{CellGlyph, pack_cells};
use sha2::{Digest, Sha256};

/// Builds a bounded atlas while preserving logical glyph sizes at the requested density.
pub fn rasterize_outline_font(
    bytes: &[u8],
    em: f32,
    raster_scale: u32,
    atlas_side: u32,
) -> Result<RuntimeFontCatalog, String> {
    if !em.is_finite()
        || !(1.0..=64.0).contains(&em)
        || !(1..=4).contains(&raster_scale)
        || !(1..=4096).contains(&atlas_side)
    {
        return Err("outline font raster dimensions exceed bounds".into());
    }
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(str::to_owned)?;
    let mut cells = Vec::new();
    let characters = (0x20..=0x7e).filter_map(char::from_u32).chain([
        '\u{b7}', '\u{d7}', '\u{2014}', '\u{2022}', '\u{2039}', '\u{203a}', '\u{2212}', '\u{fffd}',
    ]);
    for codepoint in characters {
        let source = if font.lookup_glyph_index(codepoint) == 0 {
            '?'
        } else {
            codepoint
        };
        let metrics = font.metrics(source, em);
        if metrics.width > 64 || metrics.height > 64 || !metrics.advance_width.is_finite() {
            return Err("font glyph dimensions exceed local bounds".into());
        }
        // Leave a logical pixel around ink so filtering sees transparent coverage at
        // glyph edges. Moving the bearing keeps the ink and advance in the same place.
        // Empty glyphs keep their advance and get a transparent texel with valid carrier UVs.
        let empty = metrics.width == 0 || metrics.height == 0;
        let padding = u32::from(!empty);
        let logical_size = if empty {
            [1, 1]
        } else {
            [metrics.width as u32, metrics.height as u32].map(|size| size + padding * 2)
        };
        let size = if empty {
            [1, 1]
        } else {
            logical_size.map(|size| size * raster_scale)
        };
        let mut rgba8 = vec![255; size[0] as usize * size[1] as usize * 4];
        for pixel in rgba8.as_chunks_mut::<4>().0 {
            pixel[3] = 0;
        }
        if !empty {
            let (dense, alpha) = font.rasterize(source, em * raster_scale as f32);
            let left =
                dense.xmin - metrics.xmin * raster_scale as i32 + (padding * raster_scale) as i32;
            let top = (metrics.ymin + metrics.height as i32) * raster_scale as i32
                - (dense.ymin + dense.height as i32)
                + (padding * raster_scale) as i32;
            if left < 0
                || top < 0
                || left as usize + dense.width > size[0] as usize
                || top as usize + dense.height > size[1] as usize
            {
                return Err("font raster exceeds logical glyph bounds".into());
            }
            for y in 0..dense.height {
                for x in 0..dense.width {
                    let target =
                        (((top as usize + y) * size[0] as usize + left as usize + x) * 4) + 3;
                    rgba8[target] = alpha[y * dense.width + x];
                }
            }
        }
        cells.push(CellGlyph {
            codepoint,
            size,
            rgba8: rgba8.into(),
            bearing: [
                (metrics.xmin - padding as i32) as i16,
                -(metrics.ymin + metrics.height as i32 + padding as i32) as i16,
            ],
            advance_64: (metrics.advance_width * 64.0)
                .round()
                .clamp(0.0, i16::MAX as f32) as i16,
            draw_size_64: logical_size.map(|value| value * 64),
        });
    }
    let atlas = pack_cells(&cells, 0, atlas_side, 1);
    if atlas.glyphs.len() != cells.len() || atlas.pages.len() != 1 {
        return Err("font exceeds local atlas bounds".into());
    }
    let source_hash: [u8; 32] = Sha256::digest(bytes).into();
    let pages = atlas
        .pages
        .into_iter()
        .map(|pixels| FontTexturePage {
            source_path: "font/personal-panel.png".into(),
            source_bytes: bytes.len() as u32,
            source_sha256: source_hash,
            pixels_sha256: Sha256::digest(&pixels).into(),
            width: atlas_side,
            height: atlas_side,
            pixels: FontPixels::Rgba8(pixels.into()),
        })
        .collect::<Vec<_>>();
    let glyphs = atlas
        .glyphs
        .iter()
        .map(|glyph| glyph.metrics)
        .collect::<Vec<_>>();
    let encoded =
        encode_font_catalog(source_hash, &glyphs, &pages).map_err(|error| error.to_string())?;
    RuntimeFontCatalog::decode(&encoded, source_hash)
        .map(|font| {
            font.with_glyphs(&atlas.glyphs, |_| true)
                .with_linear_sampling()
                .with_rendering(super::FontRendering::OutlineCoverage)
                .with_coverage_pages()
        })
        .map_err(|error| error.to_string())
}

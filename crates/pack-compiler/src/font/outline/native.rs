use assets::{FontFaceRaster, FontRendering};

use super::*;

mod freetype;
mod sdf;

/// Em of the distance-field raster that draws every size from [`NATIVE_SDF_MIN_PIXELS`] up.
pub const NATIVE_SDF_EM_PIXELS: u32 = 52;
/// Physical text heights below this draw from exact-size coverage rasters.
pub const NATIVE_SDF_MIN_PIXELS: u32 = 10;
/// Largest square native page; taller pages follow. The GPU keeps only drawn glyphs, in one
/// 1024² layer per page.
const MAX_NATIVE_PAGE_SIDE: u32 = 2_048;

/// Returns unhinted FreeType rasters of every mapped character: a distance field at
/// [`NATIVE_SDF_EM_PIXELS`], then exact coverage for each smaller pixel height.
pub fn compile_native_outline_rasters(
    source_path: &Path,
    source_bytes: &[u8],
    source_manifest_sha256: [u8; 32],
) -> Result<Vec<FontFaceRaster>, FontCompileError> {
    let source_hash: [u8; 32] = Sha256::digest(source_bytes).into();
    let source_length =
        u32::try_from(source_bytes.len()).map_err(|_| FontCompileError::SourceTooLarge {
            path: source_path.into(),
        })?;
    let source = (source_length, source_hash);
    let mut rasters = vec![compile(
        source_bytes,
        source,
        source_manifest_sha256,
        NATIVE_SDF_EM_PIXELS,
        FontRendering::NativeSdf,
    )?];
    for pixels in 1..NATIVE_SDF_MIN_PIXELS {
        rasters.push(compile(
            source_bytes,
            source,
            source_manifest_sha256,
            pixels,
            FontRendering::NativeCoverage,
        )?);
    }
    Ok(rasters)
}

fn compile(
    source_bytes: &[u8],
    (source_length, source_hash): (u32, [u8; 32]),
    source_manifest_sha256: [u8; 32],
    pixel_height: u32,
    rendering: FontRendering,
) -> Result<FontFaceRaster, FontCompileError> {
    let mut face = freetype::Face::new(source_bytes, pixel_height)?;
    let mut codepoints = face.codepoints()?;
    codepoints.push(REQUIRED_REPLACEMENT);
    codepoints.sort_unstable();
    codepoints.dedup();
    let mut rasterized = codepoints
        .into_iter()
        .map(|ch| {
            let glyph = face.rasterize(ch)?;
            if rendering == FontRendering::NativeSdf {
                sdf::glyph(glyph)
            } else {
                Ok(glyph)
            }
        })
        .collect::<Result<Vec<_>, FontCompileError>>()?;
    for glyph in &mut rasterized {
        if glyph.width == 0 || glyph.height == 0 {
            glyph.width = 1;
            glyph.height = 1;
            glyph.alpha = vec![0].into_boxed_slice();
        }
    }
    let mut side = 64;
    let mut packed = shelves(&rasterized, side, side);
    while side < MAX_NATIVE_PAGE_SIDE && packed.len() > 1 {
        side *= 2;
        packed = shelves(&rasterized, side, side);
    }
    if packed.len() > 1 {
        packed = shelves(&rasterized, side, MAX_FONT_PAGE_SIDE);
    }
    let mut glyphs = Vec::with_capacity(rasterized.len());
    let mut pages = Vec::with_capacity(packed.len());
    for page in packed {
        let width = page
            .iter()
            .map(|&(i, [x, _])| x + rasterized[i].width)
            .max()
            .unwrap_or(0)
            + ATLAS_PADDING;
        let height = page
            .iter()
            .map(|&(i, [_, y])| y + rasterized[i].height)
            .max()
            .unwrap_or(0)
            + ATLAS_PADDING;
        let mut coverage = vec![0; width as usize * height as usize];
        for &(index, [x, y]) in &page {
            let glyph = &rasterized[index];
            for row in 0..glyph.height {
                let target = ((y + row) * width + x) as usize;
                let source = (row * glyph.width) as usize;
                coverage[target..target + glyph.width as usize]
                    .copy_from_slice(&glyph.alpha[source..source + glyph.width as usize]);
            }
            glyphs.push(GlyphMetrics {
                codepoint: glyph.codepoint,
                page: pages.len() as u16,
                uv: [x, y, x + glyph.width, y + glyph.height].map(|value| value as u16),
                bearing: glyph.bearing,
                advance_64: glyph.advance_64,
            });
        }
        let coverage = coverage.into_boxed_slice();
        pages.push(FontTexturePage {
            source_path: format!("font/native-{pixel_height}px-{:03}.png", pages.len()).into(),
            source_bytes: source_length,
            source_sha256: source_hash,
            pixels_sha256: Sha256::digest(&coverage).into(),
            width,
            height,
            pixels: FontPixels::Coverage(coverage.into()),
        });
    }
    glyphs.sort_unstable_by_key(|glyph| glyph.codepoint);
    Ok(FontFaceRaster {
        em_pixels: pixel_height,
        rendering,
        carrier: encode_font_catalog(source_manifest_sha256, &glyphs, &pages)?,
    })
}

/// Places glyphs tallest first on shelves within `width` by `height` pages, returning each page's
/// glyph indices and their top-left texels.
fn shelves(glyphs: &[RasterizedGlyph], width: u32, height: u32) -> Vec<Vec<(usize, [u32; 2])>> {
    let mut order: Vec<_> = (0..glyphs.len()).collect();
    order.sort_by_key(|&index| {
        (
            std::cmp::Reverse(glyphs[index].height),
            glyphs[index].codepoint,
        )
    });
    let mut pages = vec![Vec::new()];
    let (mut x, mut y, mut row) = (ATLAS_PADDING, ATLAS_PADDING, 0);
    for index in order {
        let glyph = &glyphs[index];
        if x + glyph.width + ATLAS_PADDING > width {
            (x, y, row) = (ATLAS_PADDING, y + row + ATLAS_PADDING, 0);
        }
        if y + glyph.height + ATLAS_PADDING > height && !pages.last().unwrap().is_empty() {
            pages.push(Vec::new());
            (x, y, row) = (ATLAS_PADDING, ATLAS_PADDING, 0);
        }
        pages.last_mut().unwrap().push((index, [x, y]));
        x += glyph.width + ATLAS_PADDING;
        row = row.max(glyph.height);
    }
    pages
}

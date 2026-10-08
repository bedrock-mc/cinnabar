//! Remove unused atlas padding without changing any glyph's metrics or texels.

use assets::{FontPixels, RuntimeFontCatalog, encode_font_catalog};
use sha2::{Digest, Sha256};

use super::{CompiledFontCarrier, FontCompileError, invalid};

/// Trims each page to a power-of-two rectangle containing every glyph and its
/// sampling gutter. The carrier still covers exactly the same code points.
pub fn compact_font_pages(
    mut compiled: CompiledFontCarrier,
) -> Result<CompiledFontCarrier, FontCompileError> {
    let font = RuntimeFontCatalog::decode(&compiled.bytes, compiled.report.source_manifest_sha256)?;
    let mut used = vec![[1_u32; 2]; font.pages().len()];
    for glyph in font.glyphs() {
        let extent = &mut used[usize::from(glyph.page)];
        extent[0] = extent[0].max(u32::from(glyph.uv[2]).saturating_add(1));
        extent[1] = extent[1].max(u32::from(glyph.uv[3]).saturating_add(1));
    }
    let mut pages = font.pages().to_vec();
    for (page, used) in pages.iter_mut().zip(used) {
        let width = used[0].next_power_of_two().min(page.width);
        let height = used[1].next_power_of_two().min(page.height);
        if [width, height] == [page.width, page.height] {
            continue;
        }
        let source_stride = page.width as usize * 4;
        let stride = width as usize * 4;
        let mut pixels = Vec::with_capacity(stride * height as usize);
        let rgba8 = page
            .pixels
            .rgba8()
            .ok_or_else(|| invalid("compiled font page has no RGBA8 texels"))?;
        for row in rgba8.chunks_exact(source_stride).take(height as usize) {
            pixels.extend_from_slice(&row[..stride]);
        }
        page.width = width;
        page.height = height;
        page.pixels_sha256 = Sha256::digest(&pixels).into();
        page.pixels = FontPixels::Rgba8(pixels.into());
    }
    compiled.bytes = encode_font_catalog(
        compiled.report.source_manifest_sha256,
        font.glyphs(),
        &pages,
    )?;
    compiled.report.decoded_bytes = pages
        .iter()
        .map(|page| page.pixels.bytes().len() as u64)
        .sum();
    compiled.report.carrier_sha256 = compiled.bytes[compiled.bytes.len() - 32..]
        .try_into()
        .map_err(|_| invalid("compacted font carrier digest is missing"))?;
    Ok(compiled)
}

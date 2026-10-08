//! Compile a server pack's Unicode sheets with the native session glyph owner.

use std::{collections::BTreeMap, io::Cursor, path::Path};

use assets::{
    CellGlyph, FontPixels, FontTexturePage, GlyphSheet, MAX_FONT_PAGE_SIDE, MAX_FONT_PAGES,
    MAX_FONT_SOURCE_BYTES, RuntimeFontCatalog, SHEET_GRID, encode_font_catalog, extract_cells,
    pack_cells,
};
use image::{ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};

use super::{
    CompiledFontCarrier, FontCompileError, invalid, read_source, require_real_directory,
    resolve_real_source,
};

const PAGE_SIDE: u32 = 1024;
// Two native 4096² server sheets each decode to 64 MiB. Their cropped
// glyphs are small; bound source decoding separately from retained cell art.
const MAX_SHEET_DECODED_BYTES: u64 = MAX_FONT_SOURCE_BYTES * 4;
const PAGE_PIXEL_BYTES: u64 = PAGE_SIDE as u64 * PAGE_SIDE as u64 * 4;

/// Adds the pack's `font/glyph_XX.png` sheets over an authenticated base font.
/// The native sheet metrics remain authoritative; nearest-neighbour expansion
/// makes their drawn size representable by the existing MCBEFONT1 texel grid.
pub fn overlay_font_glyph_sheets(
    mut compiled: CompiledFontCarrier,
    pack: &Path,
) -> Result<CompiledFontCarrier, FontCompileError> {
    require_real_directory(&pack.join("font"))?;
    let font = RuntimeFontCatalog::decode(&compiled.bytes, compiled.report.source_manifest_sha256)?;
    let mut cells = Vec::new();
    let mut sources = Sha256::new();
    let mut source_bytes = 0_u64;
    let mut decoded_bytes = 0_u64;
    let mut cell_bytes = 0_usize;
    for high_byte in 0..=u8::MAX {
        let path = [
            format!("font/glyph_{high_byte:02X}.png"),
            format!("font/glyph_{high_byte:02x}.png"),
        ]
        .into_iter()
        .find(|path| pack.join(path).exists());
        let Some(path) = path else { continue };
        let source = resolve_real_source(pack, &path)?;
        let bytes = read_source(&source, MAX_FONT_SOURCE_BYTES.saturating_sub(source_bytes))?;
        source_bytes += bytes.len() as u64;
        sources.update(path.as_bytes());
        sources.update(Sha256::digest(&bytes));
        let sheet = decode(high_byte, &bytes, MAX_SHEET_DECODED_BYTES - decoded_bytes)?;
        decoded_bytes += sheet.rgba8.len() as u64;
        for cell in extract_cells(&sheet) {
            if cells.len() >= assets::MAX_FONT_GLYPHS {
                return Err(invalid("glyph sheets exceed the font cell budget"));
            }
            let cell = drawable_cell(cell, MAX_FONT_SOURCE_BYTES as usize - cell_bytes)?;
            cell_bytes += cell.rgba8.len();
            cells.push(cell);
        }
    }
    if cells.is_empty() {
        return Err(invalid("glyph pack contains no Unicode font sheets"));
    }
    let first_page = u16::try_from(font.pages().len())
        .map_err(|_| invalid("font page index exceeds its carrier representation"))?;
    let base_pixels: u64 = font
        .pages()
        .iter()
        .map(|page| page.pixels.bytes().len() as u64)
        .sum();
    let pixel_pages = ((MAX_FONT_SOURCE_BYTES - base_pixels) / PAGE_PIXEL_BYTES) as usize;
    let atlas = pack_cells(
        &cells,
        first_page,
        PAGE_SIDE,
        MAX_FONT_PAGES
            .saturating_sub(font.pages().len())
            .min(pixel_pages),
    );
    if atlas.glyphs.len() != cells.len() {
        return Err(invalid(
            "glyph sheets exceed the compiled font atlas budget",
        ));
    }
    let extra = font.with_glyphs(&atlas.glyphs, |_| true);
    let source_sha256 = sources.finalize().into();
    let mut pages = font.pages().to_vec();
    for (index, pixels) in atlas.pages.into_iter().enumerate() {
        pages.push(FontTexturePage {
            source_path: format!("font/server-glyphs-{index:03}.png").into(),
            source_bytes: u32::try_from(source_bytes)
                .map_err(|_| invalid("glyph source byte count exceeds its representation"))?,
            source_sha256,
            pixels_sha256: Sha256::digest(&pixels).into(),
            width: PAGE_SIDE,
            height: PAGE_SIDE,
            pixels: FontPixels::Rgba8(pixels),
        });
    }
    let mut glyphs = extra.glyphs().to_vec();
    // The carrier's canonical source ordering may differ from a session's
    // trailing-page ordering. Rebase every page index before encoding it.
    let mut order = (0..pages.len()).collect::<Vec<_>>();
    order.sort_by_key(|&index| (pages[index].source_path.clone(), pages[index].source_sha256));
    let remap = order
        .iter()
        .enumerate()
        .map(|(next, &previous)| (previous, next as u16))
        .collect::<BTreeMap<_, _>>();
    for glyph in &mut glyphs {
        glyph.page = remap[&usize::from(glyph.page)];
    }
    let pages = order
        .into_iter()
        .map(|index| pages[index].clone())
        .collect::<Vec<_>>();
    compiled.bytes = encode_font_catalog(compiled.report.source_manifest_sha256, &glyphs, &pages)?;
    compiled.report.glyphs = glyphs.len();
    compiled.report.pages = pages.len();
    compiled.report.source_bytes = pages.iter().map(|page| u64::from(page.source_bytes)).sum();
    compiled.report.decoded_bytes = pages.iter().map(|page| page.pixels.bytes().len() as u64).sum();
    compiled.report.carrier_sha256 = compiled.bytes[compiled.bytes.len() - 32..]
        .try_into()
        .map_err(|_| invalid("glyph font carrier digest is missing"))?;
    Ok(compiled)
}

fn decode(high_byte: u8, bytes: &[u8], remaining: u64) -> Result<GlyphSheet, FontCompileError> {
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png)
        .into_dimensions()
        .map_err(|error| invalid(format!("invalid glyph sheet PNG: {error}")))?;
    if width == 0
        || height == 0
        || width > MAX_FONT_PAGE_SIDE
        || height > MAX_FONT_PAGE_SIDE
        || !width.is_multiple_of(SHEET_GRID)
        || !height.is_multiple_of(SHEET_GRID)
    {
        return Err(invalid("glyph sheet is not a bounded 16-by-16 cell grid"));
    }
    if u64::from(width) * u64::from(height) * 4 > remaining {
        return Err(invalid(
            "glyph sheets exceed the aggregate decoded source budget",
        ));
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_FONT_PAGE_SIDE);
    limits.max_image_height = Some(MAX_FONT_PAGE_SIDE);
    limits.max_alloc = Some(u64::from(width) * u64::from(height) * 8);
    reader.limits(limits);
    let rgba8 = reader
        .decode()
        .map_err(|error| invalid(format!("invalid glyph sheet pixels: {error}")))?
        .into_rgba8()
        .into_raw()
        .into_boxed_slice();
    Ok(GlyphSheet {
        high_byte,
        width,
        height,
        rgba8,
    })
}

fn drawable_cell(mut cell: CellGlyph, remaining: usize) -> Result<CellGlyph, FontCompileError> {
    if cell.size == [0, 0] {
        // The font carrier requires a nonempty UV rectangle. Keep native blank
        // sheet cells blank and zero-width rather than showing a replacement.
        if remaining < 4 {
            return Err(invalid("glyph cells exceed the retained pixel budget"));
        }
        cell.size = [1, 1];
        cell.rgba8 = vec![0; 4].into();
        return Ok(cell);
    }
    let size = cell.draw_size_64.map(|value| value / 64);
    if cell
        .draw_size_64
        .iter()
        .any(|value| !value.is_multiple_of(64))
        || size.contains(&0)
        || size.iter().any(|value| *value > PAGE_SIDE - 2)
    {
        return Err(invalid(
            "glyph drawn size cannot fit the font carrier texel grid",
        ));
    }
    let pixel_bytes = size[0] as usize * size[1] as usize * 4;
    if pixel_bytes > remaining {
        return Err(invalid("glyph cells exceed the retained pixel budget"));
    }
    let mut pixels = vec![0; pixel_bytes];
    for y in 0..size[1] {
        for x in 0..size[0] {
            let source_x = x * cell.size[0] / size[0];
            let source_y = y * cell.size[1] / size[1];
            let source = ((source_y * cell.size[0] + source_x) * 4) as usize;
            let target = ((y * size[0] + x) * 4) as usize;
            pixels[target..target + 4].copy_from_slice(&cell.rgba8[source..source + 4]);
        }
    }
    cell.size = size;
    cell.rgba8 = pixels.into();
    Ok(cell)
}

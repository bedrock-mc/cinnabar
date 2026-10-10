//! Session bitmap fonts: Unicode sheets and Bedrock's mapped `default8.png` cells.

use std::{io::Cursor, sync::Arc};

use assets::{CellGlyph, GlyphSheet, SHEET_GRID, extract_cells, texel_size_64};
use image::{ImageFormat, ImageReader, Limits};
use resource_pack::LayeredPackView;

use client_ui::ui_runtime::presentation::SessionGlyphSheets;

mod mapping;
mod metadata;

const MAX_SHEET_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SHEET_SIDE: u32 = 4096;
const MAX_CELLS: usize = 65_536;

/// Crops every sheet present in the stack into glyph cells, or `None` when it has none. Each
/// sheet is dropped once cropped so a large one is not retained for the session.
pub(super) fn compile_session_glyphs(view: &LayeredPackView) -> Option<Arc<SessionGlyphSheets>> {
    let cells = bitmap(view, Some("font/default8"), Some("font/glyph_"));
    let mut named = std::collections::BTreeMap::new();
    named.insert("rune".into(), bitmap(view, Some("font/ascii_sga"), None));
    named.insert("unicode".into(), bitmap(view, None, Some("font/glyph_")));
    metadata::apply(view, &cells, &mut named);
    let cells = named.remove("default").unwrap_or(cells);
    (!cells.is_empty() || named.values().any(|cells| !cells.is_empty()))
        .then(|| Arc::new(SessionGlyphSheets::with_named(cells, named)))
}

/// Reads a named bitmap's default8 mapping and Unicode pages using one locale.
fn bitmap(view: &LayeredPackView, ascii: Option<&str>, unicode: Option<&str>) -> Vec<CellGlyph> {
    let mut cells = ascii
        .and_then(|path| {
            read_named_sheet(view, 0, &format!("{}.png", path.trim_end_matches(".png")))
        })
        .map(|sheet| default_cells(&sheet))
        .unwrap_or_default();
    let mapped: std::collections::HashSet<_> = cells.iter().map(|cell| cell.codepoint).collect();
    let mut bytes: usize = cells.iter().map(|cell| cell.rgba8.len()).sum();
    if let Some(prefix) = unicode {
        for high_byte in 0..=u8::MAX {
            let sheet = [
                format!("{prefix}{high_byte:02X}.png"),
                format!("{prefix}{high_byte:02x}.png"),
            ]
            .into_iter()
            .find_map(|name| read_named_sheet(view, high_byte, &name));
            if let Some(sheet) = sheet {
                for cell in extract_cells(&sheet) {
                    if mapped.contains(&cell.codepoint) {
                        continue;
                    }
                    bytes += cell.rgba8.len();
                    if cells.len() >= MAX_CELLS || bytes > assets::MAX_FONT_SOURCE_BYTES as usize {
                        return cells;
                    }
                    cells.push(cell);
                }
            }
        }
    }
    cells
}

/// A malformed optional sheet falls through to the next valid layer.
fn read_named_sheet(view: &LayeredPackView, high_byte: u8, name: &str) -> Option<GlyphSheet> {
    let localized = format!(
        "texts/{}/{}",
        super::resource_packs::active_language_code(),
        name
    );
    [localized.as_str(), name].into_iter().find_map(|path| {
        view.read_layers_capped(path, MAX_SHEET_SOURCE_BYTES)
            .rev()
            .find_map(|bytes| decode_sheet(high_byte, &bytes))
    })
}

/// Decodes only a valid, bounded 16-by-16 bitmap sheet.
fn decode_sheet(high_byte: u8, bytes: &[u8]) -> Option<GlyphSheet> {
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png)
        .into_dimensions()
        .ok()?;
    if width == 0
        || height == 0
        || width > MAX_SHEET_SIDE
        || height > MAX_SHEET_SIDE
        || !width.is_multiple_of(SHEET_GRID)
        || !height.is_multiple_of(SHEET_GRID)
    {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SHEET_SIDE);
    limits.max_image_height = Some(MAX_SHEET_SIDE);
    limits.max_alloc = Some(u64::from(MAX_SHEET_SIDE) * u64::from(MAX_SHEET_SIDE) * 8);
    reader.limits(limits);
    let rgba8 = reader
        .decode()
        .ok()?
        .into_rgba8()
        .into_raw()
        .into_boxed_slice();
    Some(GlyphSheet {
        high_byte,
        width,
        height,
        rgba8,
    })
}

/// The bitmap's cells use ASCII_CHAR_INDICES, not a Latin-1 byte mapping.
fn default_cells(sheet: &GlyphSheet) -> Vec<CellGlyph> {
    let cell_width = sheet.width / SHEET_GRID;
    let cell_height = sheet.height / SHEET_GRID;
    let texel = texel_size_64(0, cell_width);
    let mut cells: Vec<_> = extract_cells(sheet).into_iter().collect();
    for cell in &mut cells {
        let index = u32::from(cell.codepoint);
        cell.codepoint = char::from_u32(mapping::DEFAULT_CODEPOINTS[index as usize]).unwrap();
        if cell.codepoint == ' ' {
            cell.size = [0, 0];
            cell.rgba8 = Box::default();
            cell.draw_size_64 = [0, 0];
            // Vanilla's space is half the normalized eight-pixel cell.
            cell.advance_64 = (texel_size_64(0, 1) / 2) as i16;
            continue;
        }
        let origin = [
            index % SHEET_GRID * cell_width,
            index / SHEET_GRID * cell_height,
        ];
        let left = (0..cell_width)
            .find(|x| {
                (0..cell_height).any(|y| {
                    let pixel = ((origin[1] + y) * sheet.width + origin[0] + x) as usize;
                    sheet.rgba8[pixel * 4 + 3] != 0
                })
            })
            .unwrap_or(0);
        cell.bearing[0] = ((left * texel + 32) / 64) as i16;
        cell.advance_64 = if cell.size == [0, 0] {
            (texel_size_64(0, 1) / 8) as i16
        } else {
            (u32::try_from(cell.advance_64).unwrap_or(0) + left * texel).min(i16::MAX as u32) as i16
        };
    }
    cells
}

#[cfg(test)]
mod tests;

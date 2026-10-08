//! Bedrock `font/glyph_XX.png` sheets: a 16x16 grid of cells per high byte. Each cell is
//! trimmed to its opaque box and packed into atlas pages. Private-use sheets (E0-F8) draw at
//! one GUI px per texel; every other sheet is normalised so a cell is 8 GUI px wide. Cells
//! are centred vertically on the text line.

use crate::GlyphMetrics;

pub const SHEET_GRID: u32 = 16;
/// Font atlas texels per GUI pixel; glyph metrics are in atlas texels, like the base font's.
const DESIGN_PIXEL_TEXELS: u32 = 2;
/// Width in texels of a normalised (non-private-use) cell: 8 GUI px.
const NORMALISED_CELL: u32 = 8 * DESIGN_PIXEL_TEXELS;
/// Texels from the line's baseline up to the top of the text line, and the line's height.
const ASCENT: i64 = 14;
const LINE: i64 = 16;
const GUTTER: u32 = 1;
const PRIVATE_USE_SHEETS: std::ops::RangeInclusive<u8> = 0xe0..=0xf8;

/// One decoded sheet in straight-alpha RGBA8.
#[derive(Debug)]
pub struct GlyphSheet {
    pub high_byte: u8,
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

/// One cell cropped to its opaque box, with the metrics it draws with; `size` is `[0, 0]`
/// for a blank cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellGlyph {
    pub codepoint: char,
    pub size: [u32; 2],
    pub rgba8: Box<[u8]>,
    pub bearing: [i16; 2],
    pub advance_64: i16,
    /// Drawn size in 1/64 unscaled px.
    pub draw_size_64: [u32; 2],
}

/// A packed glyph plus the size it is drawn at, in 1/64 unscaled px.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetGlyph {
    pub metrics: GlyphMetrics,
    pub draw_size_64: [u32; 2],
}

#[derive(Debug, Default)]
pub struct GlyphAtlas {
    pub pages: Vec<Box<[u8]>>,
    pub glyphs: Vec<SheetGlyph>,
}

/// 1/64 atlas texels drawn per sheet texel for a sheet whose cells are `cell_width` texels wide.
pub fn texel_size_64(high_byte: u8, cell_width: u32) -> u32 {
    if PRIVATE_USE_SHEETS.contains(&high_byte) {
        DESIGN_PIXEL_TEXELS * 64
    } else {
        NORMALISED_CELL * 64 / cell_width
    }
}

fn valid(sheet: &GlyphSheet) -> bool {
    let bytes = (sheet.width as usize)
        .checked_mul(sheet.height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    sheet.width != 0
        && sheet.height != 0
        && sheet.width.is_multiple_of(SHEET_GRID)
        && sheet.height.is_multiple_of(SHEET_GRID)
        && sheet.width / SHEET_GRID <= 512
        && bytes == Some(sheet.rgba8.len())
}

/// Every cell of `sheet` cropped to its opaque box; empty for a malformed sheet.
pub fn extract_cells(sheet: &GlyphSheet) -> Vec<CellGlyph> {
    if !valid(sheet) {
        return Vec::new();
    }
    let (cell_w, cell_h) = (sheet.width / SHEET_GRID, sheet.height / SHEET_GRID);
    let texel_64 = i64::from(texel_size_64(sheet.high_byte, cell_w));
    let bearing_x =
        i16::from(PRIVATE_USE_SHEETS.contains(&sheet.high_byte)) * DESIGN_PIXEL_TEXELS as i16;
    let pixel = |x: u32, y: u32| ((y * sheet.width + x) * 4) as usize;
    let mut cells = Vec::new();
    for index in 0..SHEET_GRID * SHEET_GRID {
        let Some(codepoint) = char::from_u32(u32::from(sheet.high_byte) << 8 | index) else {
            continue;
        };
        let (origin_x, origin_y) = (index % SHEET_GRID * cell_w, index / SHEET_GRID * cell_h);
        let (mut left, mut right, mut top, mut bottom) = (cell_w, 0, cell_h, 0);
        for y in 0..cell_h {
            for x in 0..cell_w {
                if sheet.rgba8[pixel(origin_x + x, origin_y + y) + 3] != 0 {
                    left = left.min(x);
                    right = right.max(x + 1);
                    top = top.min(y);
                    bottom = bottom.max(y + 1);
                }
            }
        }
        if right == 0 {
            cells.push(CellGlyph {
                codepoint,
                size: [0, 0],
                rgba8: Box::default(),
                bearing: [0, 0],
                advance_64: 0,
                draw_size_64: [0, 0],
            });
            continue;
        }
        let (width, height) = (right - left, bottom - top);
        let mut rgba8 = Vec::with_capacity((width * height * 4) as usize);
        for y in top..bottom {
            let start = pixel(origin_x + left, origin_y + y);
            rgba8.extend_from_slice(&sheet.rgba8[start..start + (width * 4) as usize]);
        }
        // Cell centre on the text line's centre, then down to the cropped top row.
        let offset_64 = (LINE * 64 - i64::from(cell_h) * texel_64) / 2 + i64::from(top) * texel_64;
        let bearing_y = ((-ASCENT * 64 + offset_64 + 32).div_euclid(64)) as i16;
        cells.push(CellGlyph {
            codepoint,
            size: [width, height],
            rgba8: rgba8.into(),
            bearing: [bearing_x, bearing_y],
            advance_64: (i64::from(width) * texel_64 + i64::from(DESIGN_PIXEL_TEXELS) * 64)
                .min(i64::from(i16::MAX)) as i16,
            draw_size_64: [
                (i64::from(width) * texel_64) as u32,
                (i64::from(height) * texel_64) as u32,
            ],
        });
    }
    cells
}

/// Packs `cells` into bounded square pages, reusing identical rasters with independent metrics.
/// Private-use cells take priority; cells that do not fit are dropped.
pub fn pack_cells(cells: &[CellGlyph], first_page: u16, side: u32, max_pages: usize) -> GlyphAtlas {
    use std::{cmp::Reverse, collections::HashMap};

    let mut ordered: Vec<&CellGlyph> = cells.iter().collect();
    ordered.sort_by_key(|cell| {
        (
            !private_use(cell.codepoint),
            Reverse(cell.size[1]),
            Reverse(cell.size[0]),
            cell.codepoint,
        )
    });
    type Placement = (u16, [u16; 4]); // page, uv
    let mut rasters: HashMap<([u32; 2], &[u8]), Placement> = HashMap::new();
    let mut atlas = GlyphAtlas::default();
    let mut cursor = [0u32; 2];
    let mut row_height = 0u32;
    let page_bytes = (side * side * 4) as usize;
    for cell in ordered {
        let metrics = |page: u16, uv: [u16; 4]| SheetGlyph {
            metrics: GlyphMetrics {
                codepoint: cell.codepoint,
                page,
                uv,
                bearing: cell.bearing,
                advance_64: cell.advance_64,
            },
            draw_size_64: cell.draw_size_64,
        };
        let [width, height] = cell.size;
        if width == 0 || height == 0 {
            atlas.glyphs.push(metrics(first_page, [0; 4]));
            continue;
        }
        let raster = (cell.size, cell.rgba8.as_ref());
        if let Some(&(page, uv)) = rasters.get(&raster) {
            atlas.glyphs.push(metrics(page, uv));
            continue;
        }
        let padded = [width + GUTTER * 2, height + GUTTER * 2];
        if padded[0] > side || padded[1] > side {
            continue;
        }
        if cursor[0] + padded[0] > side {
            cursor = [0, cursor[1] + row_height];
            row_height = 0;
        }
        if atlas.pages.is_empty() || cursor[1] + padded[1] > side {
            if atlas.pages.len() >= max_pages {
                continue;
            }
            if !atlas.pages.is_empty() {
                cursor = [0, 0];
                row_height = 0;
            }
            atlas.pages.push(vec![0; page_bytes].into());
        }
        let page = atlas.pages.last_mut().expect("page just ensured");
        for y in 0..padded[1] {
            let source_y = y.saturating_sub(GUTTER).min(height - 1);
            for x in 0..padded[0] {
                let source_x = x.saturating_sub(GUTTER).min(width - 1);
                let source = ((source_y * width + source_x) * 4) as usize;
                let target = (((cursor[1] + y) * side + cursor[0] + x) * 4) as usize;
                page[target..target + 4].copy_from_slice(&cell.rgba8[source..source + 4]);
            }
        }
        let [left, top] = [cursor[0] + GUTTER, cursor[1] + GUTTER];
        let page = first_page + (atlas.pages.len() - 1) as u16;
        let uv = [
            left as u16,
            top as u16,
            (left + width) as u16,
            (top + height) as u16,
        ];
        rasters.insert(raster, (page, uv));
        atlas.glyphs.push(metrics(page, uv));
        cursor[0] += padded[0];
        row_height = row_height.max(padded[1]);
    }
    atlas.glyphs.sort_by_key(|glyph| {
        (
            !private_use(glyph.metrics.codepoint),
            glyph.metrics.codepoint,
        )
    });
    atlas
}

fn private_use(codepoint: char) -> bool {
    u8::try_from(codepoint as u32 >> 8)
        .is_ok_and(|high_byte| PRIVATE_USE_SHEETS.contains(&high_byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sheet of `cell`-px cells with each `(index, [left, right), [top, bottom))` filled solid.
    fn sheet(high_byte: u8, cell: u32, filled: &[(u32, [u32; 2], [u32; 2])]) -> GlyphSheet {
        let side = cell * SHEET_GRID;
        let mut rgba8 = vec![0u8; (side * side * 4) as usize];
        for &(index, [left, right], [top, bottom]) in filled {
            let (cell_x, cell_y) = (index % SHEET_GRID, index / SHEET_GRID);
            for y in top..bottom {
                for x in left..right {
                    let at = (((cell_y * cell + y) * side + cell_x * cell + x) * 4) as usize;
                    rgba8[at..at + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        GlyphSheet {
            high_byte,
            width: side,
            height: side,
            rgba8: rgba8.into(),
        }
    }

    fn cell(cells: &[CellGlyph], c: char) -> &CellGlyph {
        cells.iter().find(|cell| cell.codepoint == c).unwrap()
    }

    #[test]
    fn cells_crop_to_their_opaque_box() {
        let cells = extract_cells(&sheet(
            0xe0,
            16,
            &[(0, [2, 10], [3, 12]), (17, [0, 16], [0, 16])],
        ));
        assert_eq!(cells.len(), 256);
        assert_eq!(cell(&cells, '\u{e000}').size, [8, 9]);
        assert_eq!(cell(&cells, '\u{e011}').size, [16, 16]);
        assert_eq!(cell(&cells, '\u{e005}').size, [0, 0]);
    }

    #[test]
    fn private_use_cells_draw_one_gui_px_per_texel_centred_on_the_line() {
        let cells = extract_cells(&sheet(
            0xe1,
            8,
            &[(3, [0, 8], [0, 8]), (0xff, [2, 4], [3, 4])],
        ));
        let full = cell(&cells, '\u{e103}');
        assert_eq!(full.draw_size_64, [16 * 64, 16 * 64]);
        assert_eq!(full.advance_64, 18 * 64);
        assert_eq!(full.bearing, [2, -14]);
        let narrow = cell(&cells, '\u{e1ff}');
        assert_eq!(narrow.draw_size_64, [4 * 64, 2 * 64]);
        assert_eq!(narrow.advance_64, 6 * 64);
        // Row 3 of an 8 px cell drawn 2 texels per px sits 6 texels below the line top.
        assert_eq!(narrow.bearing[1], -14 + 6);
        assert_eq!(cell(&cells, '\u{e100}').advance_64, 0);
    }

    #[test]
    fn tall_cells_centre_their_content_on_the_line() {
        // A 64 px cell whose art is rows 28..35 lands on the 8 px line's centre.
        let cells = extract_cells(&sheet(0xe0, 64, &[(1, [0, 33], [28, 35])]));
        let glyph = cell(&cells, '\u{e001}');
        assert_eq!(glyph.size, [33, 7]);
        assert_eq!(glyph.draw_size_64, [66 * 64, 14 * 64]);
        assert_eq!(glyph.bearing[1], -14 + (16 - 128) / 2 + 56);
    }

    #[test]
    fn other_sheets_normalise_cells_to_eight_px() {
        let cells = extract_cells(&sheet(0x4e, 16, &[(1, [0, 16], [0, 16])]));
        let glyph = cell(&cells, '\u{4e01}');
        assert_eq!(glyph.draw_size_64, [16 * 64, 16 * 64]);
        assert_eq!(glyph.advance_64, 18 * 64);
    }

    #[test]
    fn packing_places_cropped_cells_and_orders_private_use_first() {
        let mut cells = extract_cells(&sheet(0x00, 16, &[(1, [0, 16], [0, 16])]));
        cells.extend(extract_cells(&sheet(0xe0, 16, &[(1, [0, 16], [0, 16])])));
        let atlas = pack_cells(&cells, 7, 256, 1);
        assert_eq!(atlas.glyphs[0].metrics.codepoint, '\u{e000}');
        let glyph = atlas
            .glyphs
            .iter()
            .find(|g| g.metrics.codepoint == '\u{e001}')
            .unwrap();
        assert_eq!(glyph.metrics.page, 7);
        assert_eq!(glyph.metrics.uv[2] - glyph.metrics.uv[0], 16);
        assert_eq!(atlas.pages.len(), 1);
    }

    fn raster(codepoint: char, size: [u32; 2], color: u8) -> CellGlyph {
        CellGlyph {
            codepoint,
            size,
            rgba8: [color, 40, 80, 255]
                .repeat((size[0] * size[1]) as usize)
                .into(),
            bearing: [2, -14],
            advance_64: 10 * 64,
            draw_size_64: size.map(|value| value * 64),
        }
    }

    #[test]
    fn identical_rasters_share_space_with_independent_glyph_metrics() {
        let first = raster('\u{e001}', [4, 4], 120);
        let mut second = first.clone();
        second.codepoint = '\u{e002}';
        second.bearing = [-2, -9];
        second.advance_64 = 13 * 64;
        second.draw_size_64 = [8 * 64, 9 * 64];
        let mut ordinary = first.clone();
        ordinary.codepoint = 'A';
        let cells = [ordinary, first, second];
        let atlas = pack_cells(&cells, 3, 6, 1);
        assert_eq!(atlas.pages.len(), 1);
        assert_eq!(atlas.glyphs.len(), cells.len());
        assert_eq!(atlas.glyphs.last().unwrap().metrics.codepoint, 'A');
        for cell in &cells {
            let glyph = atlas
                .glyphs
                .iter()
                .find(|glyph| glyph.metrics.codepoint == cell.codepoint)
                .unwrap();
            assert_eq!(glyph.metrics.bearing, cell.bearing);
            assert_eq!(glyph.metrics.advance_64, cell.advance_64);
            assert_eq!(glyph.draw_size_64, cell.draw_size_64);
            assert_eq!(glyph.metrics.uv, atlas.glyphs[0].metrics.uv);
        }
    }

    #[test]
    fn distinct_colors_and_raster_dimensions_keep_separate_allocations() {
        let cells = [
            raster('\u{e001}', [4, 4], 120),
            raster('\u{e002}', [4, 4], 180),
            raster('\u{e003}', [2, 8], 120),
        ];
        let atlas = pack_cells(&cells, 0, 16, 1);
        assert_eq!(atlas.glyphs.len(), cells.len());
        for (index, cell) in cells.iter().enumerate() {
            let glyph = &atlas.glyphs[index];
            let [left, top, right, bottom] = glyph.metrics.uv;
            assert_eq!(
                [u32::from(right - left), u32::from(bottom - top)],
                cell.size
            );
            let pixel = ((u32::from(top) * 16 + u32::from(left)) * 4) as usize;
            assert_eq!(
                &atlas.pages[glyph.metrics.page as usize][pixel..pixel + 4],
                &cell.rgba8[..4]
            );
        }
        assert_ne!(atlas.glyphs[0].metrics.uv, atlas.glyphs[1].metrics.uv);
        assert_ne!(atlas.glyphs[0].metrics.uv, atlas.glyphs[2].metrics.uv);
    }

    #[test]
    fn packing_groups_tall_cells_before_short_cells() {
        let cells = [
            raster('\u{e001}', [6, 2], 120),
            raster('\u{e002}', [6, 10], 180),
            raster('\u{e003}', [6, 10], 240),
        ];
        let atlas = pack_cells(&cells, 0, 16, 1);
        assert_eq!(atlas.pages.len(), 1);
        assert_eq!(atlas.glyphs.len(), cells.len());
    }

    #[test]
    fn repeated_large_rasters_leave_room_for_a_later_small_icon() {
        let mut cells: Vec<_> = (0..4)
            .map(|index| raster(char::from_u32(0xe100 + index).unwrap(), [6, 6], 120))
            .collect();
        let icon = raster('\u{e200}', [2, 2], 180);
        cells.push(icon.clone());
        let atlas = pack_cells(&cells, 0, 16, 1);
        assert_eq!(atlas.pages.len(), 1);
        assert_eq!(atlas.glyphs.len(), cells.len());
        let glyph = atlas.glyphs.last().unwrap();
        assert_eq!(glyph.metrics.codepoint, icon.codepoint);
        assert_eq!(glyph.draw_size_64, icon.draw_size_64);
        let [left, top, right, bottom] = glyph.metrics.uv;
        assert_eq!([right - left, bottom - top], [2, 2]);
        let pixel = ((u32::from(top) * 16 + u32::from(left)) * 4) as usize;
        assert_eq!(&atlas.pages[0][pixel..pixel + 4], &icon.rgba8[..4]);
    }

    #[test]
    fn oversized_cells_are_dropped_and_page_overflow_is_capped() {
        let huge = extract_cells(&sheet(0xe0, 64, &[(0, [0, 64], [0, 64])]));
        let none = pack_cells(&huge, 0, 32, 1);
        assert!(
            none.glyphs
                .iter()
                .all(|g| g.metrics.codepoint != '\u{e000}')
        );
        let full: Vec<_> = (0..256).map(|i| (i, [0, 16], [0, 16])).collect();
        let many: Vec<CellGlyph> = (0xe0..0xf0)
            .flat_map(|b| extract_cells(&sheet(b, 16, &full)))
            .map(|mut cell| {
                let scalar = (cell.codepoint as u32).to_le_bytes();
                cell.rgba8[..2].copy_from_slice(&scalar[..2]);
                cell
            })
            .collect();
        let capped = pack_cells(&many, 0, 256, 2);
        assert_eq!(capped.pages.len(), 2);
        assert!(capped.glyphs.len() < many.len());
    }

    #[test]
    fn malformed_sheets_yield_no_cells() {
        let bad = GlyphSheet {
            high_byte: 0xe0,
            width: 17,
            height: 16,
            rgba8: vec![0; 17 * 16 * 4].into(),
        };
        let short = GlyphSheet {
            high_byte: 0xe0,
            width: 16,
            height: 16,
            rgba8: vec![0; 4].into(),
        };
        assert!(extract_cells(&bad).is_empty() && extract_cells(&short).is_empty());
    }
}

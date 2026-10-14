//! Original monochrome artwork for symbols outside the shipped outline maps.

use super::*;

pub(super) const BED_CODEPOINT: char = '\u{1f6cf}';

const BED: [&[u8; 16]; 10] = [
    b"##..............",
    b"##..............",
    b"##.####.........",
    b"##.####.######..",
    b"##.###########..",
    b"################",
    b"################",
    b"##............##",
    b"##............##",
    b"##............##",
];

/// Scales the original cell to the requested em and checks the raster admission bounds.
fn bed_dimensions(em: u32) -> Result<[u32; 2], FontCompileError> {
    let width = em * BED[0].len() as u32 / assets::FONT_RASTER_EM_PIXELS;
    let height = em * BED.len() as u32 / assets::FONT_RASTER_EM_PIXELS;
    if width == 0 || height == 0 || width > 64 || height > 64 {
        return Err(metric_error(BED_CODEPOINT, "symbol bitmap"));
    }
    Ok([width, height])
}

/// Charges the symbol's coverage allocation before any carrier raster buffers are allocated.
pub(super) fn bed_alpha_charge(em: u32) -> Result<usize, FontCompileError> {
    let [width, height] = bed_dimensions(em)?;
    Ok((width * height) as usize)
}

/// Draws a headboard, pillow, mattress and legs with a two-texel baseline clearance.
pub(super) fn bed(em: u32) -> Result<RasterizedGlyph, FontCompileError> {
    let [width, height] = bed_dimensions(em)?;
    let mut alpha = vec![0; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let row = y as usize * BED.len() / height as usize;
            let column = x as usize * BED[0].len() / width as usize;
            alpha[(y * width + x) as usize] = if BED[row][column] == b'#' { 255 } else { 0 };
        }
    }
    let gap = (em * 2 / assets::FONT_RASTER_EM_PIXELS).max(1);
    Ok(RasterizedGlyph {
        codepoint: BED_CODEPOINT,
        width,
        height,
        bearing: [0, -((height + gap) as i16)],
        advance_64: ((width + gap) * FIXED_POINT_DENOMINATOR as u32) as i16,
        alpha: alpha.into_boxed_slice(),
    })
}

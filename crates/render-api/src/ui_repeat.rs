//! Sampling contract for repeated atlas regions without repeated geometry.

/// Repeats the horizontal tile coordinate within the source bounds in `overlay_color`.
pub const UI_STYLE_REPEAT_X: u16 = 1 << 11;
/// Repeats the vertical tile coordinate within the source bounds in `overlay_color`.
pub const UI_STYLE_REPEAT_Y: u16 = 1 << 12;
/// Identifies vertices whose UVs use tile units and whose overlay holds source texel bounds.
pub const UI_STYLE_REPEAT: u16 = UI_STYLE_REPEAT_X | UI_STYLE_REPEAT_Y;
/// Clips a repeated original-image fragment to the content owned by its texture page.
pub const UI_STYLE_REPEAT_PAGE: u16 = 1 << 13;

/// Maps tile coordinates into oriented source bounds `(x, y, width, height)`.
pub fn source_uv(uv: [f32; 2], bounds: [f32; 4], flags: u16) -> [f32; 2] {
    std::array::from_fn(|axis| {
        let repeat = [UI_STYLE_REPEAT_X, UI_STYLE_REPEAT_Y][axis];
        let coord = if flags & repeat != 0 {
            uv[axis] - uv[axis].floor()
        } else {
            uv[axis]
        };
        bounds[axis] + coord * bounds[axis + 2]
    })
}

/// Keeps linear filtering inside the source texel centers at each tile's edges.
pub fn filtered_uv(uv: [f32; 2], bounds: [f32; 4]) -> [f32; 2] {
    std::array::from_fn(|axis| {
        let lo = bounds[axis].min(bounds[axis] + bounds[axis + 2]);
        let size = bounds[axis + 2].abs();
        let inset = (size * 0.5).min(0.5);
        uv[axis].clamp(lo + inset, lo + size - inset)
    })
}

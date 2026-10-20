//! Sampling contract for repeated atlas regions without repeated geometry.

/// Repeats the horizontal tile coordinate within the source bounds in `overlay_color`.
pub const UI_STYLE_REPEAT_X: u16 = 1 << 12;
/// Repeats the vertical tile coordinate within the source bounds in `overlay_color`.
pub const UI_STYLE_REPEAT_Y: u16 = 1 << 13;
/// Identifies vertices whose UVs use tile units and whose overlay holds source texel bounds.
pub const UI_STYLE_REPEAT: u16 = UI_STYLE_REPEAT_X | UI_STYLE_REPEAT_Y;
/// Clips a repeated original-image fragment to the content owned by its texture page.
pub const UI_STYLE_REPEAT_PAGE: u16 = 1 << 14;

/// Shader companion of [`decode_uv`], using the renderer's shared style constants.
pub const WGSL_DECODER: &str = include_str!("ui_repeat.wgsl");

/// Decodes a repeated region, rejecting fragments owned by another physical page.
/// Source bounds use texels; coordinates use tile units. Linear filtering stays
/// inside source bounds and may use a page's filter halo.
pub fn decode_uv(
    coords: [f32; 2],
    bounds: [f32; 4],
    flags: u16,
    dimensions: [u32; 2],
    content: Option<[u32; 4]>,
) -> Option<[f32; 2]> {
    let mut uv = source_uv(coords, bounds, flags);
    if flags & u16::from(crate::UI_STYLE_BILINEAR) != 0 {
        uv = filtered_uv(uv, bounds);
    }
    if flags & UI_STYLE_REPEAT_PAGE != 0 {
        if dimensions.contains(&0) {
            return None;
        }
        let content = content.unwrap_or([0, 0, dimensions[0], dimensions[1]]);
        for axis in 0..2 {
            let size = dimensions[axis] as f32;
            let lo = content[axis] as f32;
            let hi = content[axis + 2] as f32;
            if (uv[axis] < lo && lo > 0.0) || (uv[axis] >= hi && hi < size) {
                return None;
            }
            uv[axis] = uv[axis].clamp(0.5, (size - 0.5).max(0.5));
        }
    }
    Some(uv)
}

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

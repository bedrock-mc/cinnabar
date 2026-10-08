//! Native fire topology: one supported mesh, then attachment masks for both
//! positional texture choices and horizontal UV orientations.

pub const FIRE_ATTACHMENT_MASK_COUNT: u32 = 1 << 5;
pub const FIRE_TEMPLATE_COUNT: u32 = 1 + FIRE_ATTACHMENT_MASK_COUNT * 4;
pub const FIRE_SUPPORTED_QUAD_COUNT: u32 = 8;

/// Mask order is west, east, north, south, above.
#[must_use]
pub const fn fire_attachment_template_offset(mask: u8, alternate: bool, flip_u: bool) -> u32 {
    1 + mask as u32 + FIRE_ATTACHMENT_MASK_COUNT * (alternate as u32 * 2 + flip_u as u32)
}

#[must_use]
pub const fn fire_template_quad_count(offset: u32) -> u32 {
    if offset == 0 {
        return FIRE_SUPPORTED_QUAD_COUNT;
    }
    let mask = (offset - 1) % FIRE_ATTACHMENT_MASK_COUNT;
    (mask & 15).count_ones() * 2 + ((mask >> 4) & 1) * 2
}

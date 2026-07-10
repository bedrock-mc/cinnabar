/// Returns a stable opaque debug colour for a raw block runtime value.
///
/// This intentionally uses only integer arithmetic so a runtime value maps to
/// the same colour on every supported platform. The hashed hue plus bounded
/// saturation/value ranges keep adjacent palette entries easy to distinguish.
#[must_use]
pub fn debug_color(runtime_id: u32) -> [u8; 4] {
    let hash = mix(runtime_id);
    let hue = hash % (6 * 256);
    let sector = hue / 256;
    let offset = hue % 256;
    let saturation = 160 + ((hash >> 16) & 0x3f);
    let value = 192 + ((hash >> 24) & 0x3f);
    let chroma = value * saturation / 255;
    let secondary = if sector.is_multiple_of(2) {
        chroma * offset / 255
    } else {
        chroma * (255 - offset) / 255
    };
    let minimum = value - chroma;

    let (red, green, blue) = match sector {
        0 => (chroma, secondary, 0),
        1 => (secondary, chroma, 0),
        2 => (0, chroma, secondary),
        3 => (0, secondary, chroma),
        4 => (secondary, 0, chroma),
        _ => (chroma, 0, secondary),
    };
    [
        (red + minimum) as u8,
        (green + minimum) as u8,
        (blue + minimum) as u8,
        255,
    ]
}

const fn mix(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}

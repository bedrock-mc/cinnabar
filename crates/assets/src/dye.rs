//! Shared dye palette and actor color-index conversion.

/// RGB dye colors in Bedrock color order, black through white.
pub(crate) const DYE_RGB: [[u8; 3]; 16] = [
    [0x1D, 0x1D, 0x21],
    [0xB0, 0x2E, 0x26],
    [0x5E, 0x7C, 0x16],
    [0x83, 0x54, 0x32],
    [0x3C, 0x44, 0xAA],
    [0x89, 0x32, 0xB8],
    [0x16, 0x9C, 0x9C],
    [0x9D, 0x9D, 0x97],
    [0x47, 0x4F, 0x52],
    [0xF3, 0x8B, 0xAA],
    [0x80, 0xC7, 0x1F],
    [0xFE, 0xD8, 0x3D],
    [0x3A, 0xB3, 0xDA],
    [0xC7, 0x4E, 0xBD],
    [0xF9, 0x80, 0x1D],
    [0xF0, 0xF0, 0xF0],
];

/// Gamma-space RGBA for an actor palette index, wrapping to its low four bits.
/// White is neutral; the remaining dye entries carry zero alpha.
#[must_use]
pub fn actor_palette_color(index: u8) -> [f32; 4] {
    let index = index & 0xF;
    if index == 0 {
        return [1.0; 4];
    }
    let [r, g, b] = DYE_RGB[usize::from(15 - index)].map(|value| f32::from(value) / 255.0);
    [r, g, b, 0.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_and_banner_indices_share_rgb_with_neutral_actor_white() {
        assert_eq!(actor_palette_color(0), [1.0; 4]);
        for index in 1..16 {
            let rgb = crate::banner::color_rgb(i64::from(15 - index))
                .map(|value| f32::from(value) / 255.0);
            assert_eq!(actor_palette_color(index), [rgb[0], rgb[1], rgb[2], 0.0]);
            assert_eq!(actor_palette_color(index + 16), actor_palette_color(index));
        }
    }
}

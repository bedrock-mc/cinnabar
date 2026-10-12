//! Banner dye colors shared by placed models and inventory thumbnails.

use crate::dye::DYE_RGB;

/// Maximum overlay layers admitted by banner presentation, excluding the base.
pub const MAX_BANNER_LAYERS: usize = 16;

/// Gamma-space RGB for a Bedrock banner base or pattern color.
#[must_use]
pub fn color_rgb(bedrock_value: i64) -> [u8; 3] {
    DYE_RGB[(bedrock_value & 0xF) as usize]
}

/// Linear-space RGB for placed banner vertex tints.
#[must_use]
pub fn color_linear(bedrock_value: i64) -> [f32; 3] {
    color_rgb(bedrock_value).map(|value| {
        let value = f32::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bedrock_banner_aux_counts_from_black_to_white() {
        assert_eq!(color_rgb(0), [0x1D, 0x1D, 0x21]);
        assert_eq!(color_rgb(1), [0xB0, 0x2E, 0x26]);
        assert_eq!(color_rgb(15), [0xF0; 3]);
        assert_eq!(color_rgb(-1), color_rgb(15));
        assert!(color_linear(15)[0] > 0.8);
        assert!(color_linear(0)[0] < 0.05);
    }
}

mod patterns;
pub use patterns::{
    BANNER_PATTERNS, BannerPattern, pattern_by_code, pattern_by_ui_id, pattern_texture,
    pattern_ui_id,
};

/// Resolves an exact named dye item to its Bedrock color index.
pub fn dye_color(identifier: &str) -> Option<u8> {
    const NAMES: [&str; 16] = [
        "black",
        "red",
        "green",
        "brown",
        "blue",
        "purple",
        "cyan",
        "light_gray",
        "gray",
        "pink",
        "lime",
        "yellow",
        "light_blue",
        "magenta",
        "orange",
        "white",
    ];
    let name = identifier
        .strip_prefix("minecraft:")?
        .strip_suffix("_dye")?;
    NAMES
        .iter()
        .position(|candidate| *candidate == name)
        .map(|index| index as u8)
}

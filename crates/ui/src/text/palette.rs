//! Formatting colors loaded from the active JSON-UI global variables.
//!
//! Vanilla reads RGB triples from the global colour definitions into its shared
//! format-code colour table.

use super::BedrockColor;

/// The active pack's formatting colors, separate from cached text geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormattingPalette([Option<[u8; 3]>; BedrockColor::PartyBlue as usize + 1]);

impl FormattingPalette {
    /// Reads vanilla's global names; absent or malformed triples become white.
    pub fn from_globals(mut lookup: impl FnMut(&str) -> Option<[f32; 3]>) -> Self {
        let mut palette = Self::default();
        for &(color, name) in GLOBALS {
            let rgb = lookup(name).unwrap_or([1.0; 3]);
            palette.0[color as usize] =
                Some(rgb.map(|component| (component.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
        palette
    }

    /// Resolves a formatting color while preserving the label's RGB for unformatted text.
    pub fn rgb(&self, color: BedrockColor) -> Option<[u8; 3]> {
        self.0[color as usize].or_else(|| color.rgb())
    }
}

/// The color table's global keys, in vanilla formatting-code order.
const GLOBALS: &[(BedrockColor, &str)] = &[
    (BedrockColor::Black, "$0_color_format"),
    (BedrockColor::DarkBlue, "$1_color_format"),
    (BedrockColor::DarkGreen, "$2_color_format"),
    (BedrockColor::DarkAqua, "$3_color_format"),
    (BedrockColor::DarkRed, "$4_color_format"),
    (BedrockColor::DarkPurple, "$5_color_format"),
    (BedrockColor::Gold, "$6_color_format"),
    (BedrockColor::Gray, "$7_color_format"),
    (BedrockColor::DarkGray, "$8_color_format"),
    (BedrockColor::Blue, "$9_color_format"),
    (BedrockColor::Green, "$a_color_format"),
    (BedrockColor::Aqua, "$b_color_format"),
    (BedrockColor::Red, "$c_color_format"),
    (BedrockColor::LightPurple, "$d_color_format"),
    (BedrockColor::Yellow, "$e_color_format"),
    (BedrockColor::White, "$f_color_format"),
    (BedrockColor::MinecoinGold, "$coin_color"),
    (BedrockColor::MaterialQuartz, "$material_quartz_color"),
    (BedrockColor::MaterialIron, "$material_iron_color"),
    (BedrockColor::MaterialNetherite, "$material_netherite_color"),
    (BedrockColor::MaterialRedstone, "$material_redstone_color"),
    (BedrockColor::MaterialCopper, "$material_copper_color"),
    (BedrockColor::MaterialGold, "$material_gold_color"),
    (BedrockColor::MaterialEmerald, "$material_emerald_color"),
    (BedrockColor::MaterialDiamond, "$material_diamond_color"),
    (BedrockColor::MaterialLapis, "$material_lapis_color"),
    (BedrockColor::MaterialAmethyst, "$material_amethyst_color"),
    (BedrockColor::MaterialResin, "$material_resin_color"),
    (BedrockColor::PartyBlue, "$party_blue_color"),
];

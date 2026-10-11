//! Shared formatting colour names, codes, globals and fallback RGB values.

use super::BedrockColor;

/// One semantic formatting colour; active packs may replace its fallback RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorDescriptor {
    pub color: BedrockColor,
    pub name: Option<&'static str>,
    pub code: char,
    pub global: &'static str,
    pub fallback_rgb: [u8; 3],
}

/// Declare all colour lookup paths from the same descriptor rows.
macro_rules! colors {
    ($($color:ident, $name:expr, $code:literal, $global:literal, $rgb:expr;)*) => {
        /// Shared semantic colours for component admission and text drawing.
        pub const FORMATTING_COLORS: &[ColorDescriptor] = &[$(ColorDescriptor {
            color: BedrockColor::$color, name: $name, code: $code,
            global: $global, fallback_rgb: $rgb,
        },)*];

        impl BedrockColor {
            /// The descriptor for a colour; unformatted text has none.
            #[must_use]
            pub const fn descriptor(self) -> Option<&'static ColorDescriptor> {
                match self {
                    $(Self::$color => Some(&ColorDescriptor {
                        color: Self::$color, name: $name, code: $code,
                        global: $global, fallback_rgb: $rgb,
                    }),)*
                    Self::Base => None,
                }
            }

            /// Resolve a case-sensitive formatting code without scanning names.
            #[must_use]
            pub const fn from_code(code: char) -> Option<Self> {
                match code { $($code => Some(Self::$color),)* _ => None }
            }

            /// Resolve a component colour name when admitting item definitions.
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                FORMATTING_COLORS.iter().find(|entry| entry.name.is_some_and(|candidate| candidate.eq_ignore_ascii_case(name)))
                    .map(|entry| entry.color)
            }

            /// The fallback RGB; unformatted text keeps the draw's own colour.
            #[must_use]
            pub const fn rgb(self) -> Option<[u8; 3]> {
                match self.descriptor() { Some(entry) => Some(entry.fallback_rgb), None => None }
            }
        }
    }
}

colors! {
    Black, Some("black"), '0', "$0_color_format", [0, 0, 0];
    DarkBlue, Some("dark_blue"), '1', "$1_color_format", [0, 0, 170];
    DarkGreen, Some("dark_green"), '2', "$2_color_format", [0, 170, 0];
    DarkAqua, Some("dark_aqua"), '3', "$3_color_format", [0, 170, 170];
    DarkRed, Some("dark_red"), '4', "$4_color_format", [170, 0, 0];
    DarkPurple, Some("dark_purple"), '5', "$5_color_format", [170, 0, 170];
    Gold, Some("gold"), '6', "$6_color_format", [255, 170, 0];
    Gray, Some("gray"), '7', "$7_color_format", [170, 170, 170];
    DarkGray, Some("dark_gray"), '8', "$8_color_format", [85, 85, 85];
    Blue, Some("blue"), '9', "$9_color_format", [85, 85, 255];
    Green, Some("green"), 'a', "$a_color_format", [85, 255, 85];
    Aqua, Some("aqua"), 'b', "$b_color_format", [85, 255, 255];
    Red, Some("red"), 'c', "$c_color_format", [255, 85, 85];
    LightPurple, Some("light_purple"), 'd', "$d_color_format", [255, 85, 255];
    Yellow, Some("yellow"), 'e', "$e_color_format", [255, 255, 85];
    White, Some("white"), 'f', "$f_color_format", [255, 255, 255];
    MinecoinGold, Some("minecoin_gold"), 'g', "$coin_color", [221, 214, 5];
    MaterialQuartz, Some("material_quartz"), 'h', "$material_quartz_color", [227, 212, 209];
    MaterialIron, Some("material_iron"), 'i', "$material_iron_color", [206, 202, 202];
    MaterialNetherite, Some("material_netherite"), 'j', "$material_netherite_color", [68, 58, 59];
    MaterialRedstone, Some("material_redstone"), 'm', "$material_redstone_color", [151, 22, 7];
    MaterialCopper, Some("material_copper"), 'n', "$material_copper_color", [180, 104, 77];
    MaterialGold, Some("material_gold"), 'p', "$material_gold_color", [222, 177, 45];
    MaterialEmerald, Some("material_emerald"), 'q', "$material_emerald_color", [17, 160, 54];
    MaterialDiamond, Some("material_diamond"), 's', "$material_diamond_color", [44, 186, 168];
    MaterialLapis, Some("material_lapis"), 't', "$material_lapis_color", [35, 98, 180];
    MaterialAmethyst, Some("material_amethyst"), 'u', "$material_amethyst_color", [154, 92, 198];
    MaterialResin, Some("material_resin"), 'v', "$material_resin_color", [237, 105, 52];
    PartyBlue, None, 'w', "$party_blue_color", [140, 179, 255];
}

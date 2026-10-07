//! Runtime faces used by OreUI's semantic text styles.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OreUiFont {
    Seven,
    Ten,
    SevenPixel,
    FivePixel,
    Five,
    FiveBold,
}

impl OreUiFont {
    pub const ALL: [Self; 6] = [
        Self::Seven,
        Self::Ten,
        Self::SevenPixel,
        Self::FivePixel,
        Self::Five,
        Self::FiveBold,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Seven => "Minecraft Seven v2",
            Self::Ten => "Minecraft Ten v2",
            Self::SevenPixel => "Minecraft Seven v4",
            Self::FivePixel => "Minecraft Five v3",
            Self::Five => "Minecraft Five v2",
            Self::FiveBold => "Minecraft Five v2 Bold",
        }
    }

    pub const fn source_prefix(self) -> &'static str {
        match self {
            Self::Seven => "Minecraft-Seven-",
            Self::Ten => "Minecraft-Ten-",
            Self::SevenPixel => "Minecraft-Seven-v4-",
            Self::FivePixel => "MinecraftFiveV3-",
            Self::Five => "Minecraft-Five-",
            Self::FiveBold => "Minecraft-Five-Bold-",
        }
    }

    pub const fn source_extension(self) -> &'static str {
        match self {
            Self::SevenPixel | Self::FivePixel => ".ttf",
            _ => ".otf",
        }
    }

    /// Physical em units per desktop GUI scale for the native pixel roles.
    pub const fn raster_gui_pixels(self) -> Option<u32> {
        match self {
            Self::SevenPixel => Some(8),
            Self::FivePixel => Some(5),
            _ => None,
        }
    }
}

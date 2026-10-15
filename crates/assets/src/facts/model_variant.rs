//! Typed artwork variants assigned by registry rules before model admission.

use serde::{Deserialize, Serialize};

/// Shared oxidation domain for copper block models.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CopperAge {
    Unaffected,
    Exposed,
    Weathered,
    Oxidized,
}

/// Ordinary chest models; ender chests have their own block-entity renderer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ChestKind {
    Normal,
    Trapped,
    Copper(CopperAge),
}

/// Model-specific facts retained at block admission.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BlockModelVariant {
    Skull(u8),
    Chest(ChestKind),
    Statue(CopperAge),
}

/// Legacy pack schema keys shared by block textures and sound materials.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LegacyBlockAlias {
    Grass,
    Chain,
    SeaLantern,
    TripWire,
    YellowFlower,
    RedFlower,
    Sapling,
}

impl LegacyBlockAlias {
    /// Returns the pack schema key after registry membership has been resolved.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Grass => "grass",
            Self::Chain => "chain",
            Self::SeaLantern => "seaLantern",
            Self::TripWire => "tripWire",
            Self::YellowFlower => "yellow_flower",
            Self::RedFlower => "red_flower",
            Self::Sapling => "sapling",
        }
    }
}

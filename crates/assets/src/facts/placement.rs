//! Placement admission and related block identities compiled by registryc.

use std::{collections::BTreeMap, sync::OnceLock};

use crate::AssetError;

pub const BLOCK_PLACEMENT_SOURCE_PATH: &str = "crates/assets/data/block-placement-v2193.tsv";

/// Declares placement names and their parser from one exhaustive list.
macro_rules! named_facts {
    ($(#[$doc:meta])* $name:ident { $($rule:ident),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub enum $name {
            #[default]
            Unresolved,
            $($rule),+
        }
        impl $name {
            /// Reads a compiler catalog value; unknown rules remain invalid.
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    "Unresolved" => Some(Self::Unresolved),
                    $(stringify!($rule) => Some(Self::$rule),)+
                    _ => None,
                }
            }
        }
    };
}
named_facts!(
    /// The placement transformation admitted for a block's state shape.
    PlacementRule {
    Identity,
    Axis,
    Loom,
    Glazed,
    Stair,
    Slab,
    DoubleSlab,
    Trapdoor,
    Hopper,
    Connections,
    Torch,
    Lever,
    Lantern,
    Snow,
    SeaPickle,
    Candle,
    Vine,
    Multiface,
    Door,
    StandingSign,
    WallSign,
    Dispenser,
    Piston,
    Button
});

named_facts!(
    /// Support classifications retained by provisional placement prediction.
    SupportKind { Stone, PowderSnow, Leaves, Water, Lava, Air, Fire, SnowLayer, Vanilla }
);

impl SupportKind {
    /// Classifies readable support metadata once at registry or custom-block admission.
    pub fn for_identifier(identifier: &str) -> Self {
        match identifier {
            "minecraft:stone" => Self::Stone,
            "minecraft:powder_snow" => Self::PowderSnow,
            "minecraft:air" => Self::Air,
            "minecraft:water" | "minecraft:flowing_water" => Self::Water,
            "minecraft:lava" | "minecraft:flowing_lava" => Self::Lava,
            "minecraft:fire" | "minecraft:soul_fire" => Self::Fire,
            "minecraft:snow_layer" => Self::SnowLayer,
            "minecraft:leaves" | "minecraft:leaves2" => Self::Leaves,
            name if name.ends_with("_leaves") => Self::Leaves,
            name if name.starts_with("minecraft:") => Self::Vanilla,
            _ => Self::Unresolved,
        }
    }
}

#[derive(Debug)]
pub(super) struct PlacementEntry {
    pub rule: PlacementRule,
    pub support: SupportKind,
    pub alternate: Option<Box<str>>,
}

/// Reads the generated catalog once, rejecting duplicate names and unknown rules.
pub(super) fn catalog() -> Result<&'static BTreeMap<Box<str>, PlacementEntry>, AssetError> {
    static CATALOG: OnceLock<Result<BTreeMap<Box<str>, PlacementEntry>, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let mut entries = BTreeMap::new();
            for row in include_str!("../../data/block-placement-v2193.tsv")
                .lines()
                .skip(1)
            {
                let fields: Vec<_> = row.split('\t').collect();
                if fields.len() != 4 {
                    return Err("invalid placement catalog row".into());
                }
                let entry = PlacementEntry {
                    rule: PlacementRule::from_name(fields[1]).ok_or("unknown placement rule")?,
                    support: SupportKind::from_name(fields[2])
                        .ok_or("unknown placement support")?,
                    alternate: (fields[3] != "-").then(|| fields[3].into()),
                };
                if entries.insert(fields[0].into(), entry).is_some() {
                    return Err("duplicate placement catalog entry".into());
                }
            }
            Ok(entries)
        })
        .as_ref()
        .map_err(|detail| AssetError::InvalidCompiledAssets {
            detail: detail.clone().into(),
        })
}

/// Joins a counterpart only when that block is present in the admitted palette.
pub(super) fn alternate(entry: &PlacementEntry, names: &BTreeMap<&str, u32>) -> Option<u32> {
    names.get(entry.alternate.as_deref()?).copied()
}

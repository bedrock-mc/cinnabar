//! Render identities admitted once from the compiled block expansion.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};

/// A dedicated render route; unknown content retains its ordinary family route.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum BlockRenderKind {
    #[default]
    Ordinary,
    StandingSign,
    WallSign,
    HangingSign,
    Lantern,
    FlowerPot,
    BambooFence,
    BambooGate,
    NetherBrickFence,
    StainedPane,
    Water,
    Lava,
    DeprecatedPillar,
    Tnt,
    Chain,
    Crystal,
    Ladder,
    Seagrass,
    Kelp,
    PaleMossCarpet,
    ClearGlass,
    Anvil,
    BrewingStand,
    BubbleColumn,
    Campfire,
    DragonEgg,
    EndRod,
    Fire,
    Grindstone,
    Lever,
    LilyPad,
    PitcherCrop,
    SeaPickle,
    Stem,
    TripwireHook,
    Barrel,
    CommandBlock,
    Composter,
    Cauldron,
    Cactus,
    Cake,
    Farmland,
    BeeHousing,
    ResinClump,
    ChiseledBookshelf,
    EnchantingTable,
    Azalea,
    Bamboo,
    Roots,
    Grass,
    Mycelium,
    GrassPath,
    SnowLayer,
    RedstoneBase,
    NamedCube,
    RespawnAnchor,
    EndPortalFrame,
    Portal,
    SelectorAliasCube,
    MineralCube,
    StainedGlass,
    CopperGrate,
    CandleCake,
    Shelf,
    LightningRod,
    CopperBulb,
    CoralWallFan,
    CoralFan,
    LiteralCube,
    Invisible,
    EntityDrawn,
    Ice,
    FrostedIce,
    Slime,
    Honey,
    TintedGlass,
    PowderSnow,
    Snow,
    BerryBush,
    DefaultRail,
    CropRows,
    Torchflower,
    AuthoredModel,
    GrassCross,
}

/// Cold identity admission for pack compilation; frame paths retain typed model handles.
pub fn block_render_fact(identifier: &str) -> BlockRenderFact {
    static KINDS: OnceLock<BTreeMap<&'static str, BlockRenderFact>> = OnceLock::new();
    KINDS
        .get_or_init(|| {
            let mut rows = include_str!("../../data/facts-expansion-v2193.tsv").lines();
            assert_eq!(
                rows.next(),
                Some("identifier\ttraits\trules\tprovisional\trender")
            );
            let mut kinds = BTreeMap::new();
            for row in rows {
                let columns: Vec<_> = row.split('\t').collect();
                assert_eq!(columns.len(), 5, "compiled render row");
                let fact = serde_json::from_str(columns[4]).expect("compiled render fact");
                assert!(
                    kinds.insert(columns[0], fact).is_none(),
                    "duplicate render identity"
                );
            }
            kinds
        })
        .get(identifier)
        .copied()
        .unwrap_or_default()
}

/// A registered portal axis; the unspecified form uses the Z mesh and neighbor override.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortalAxis {
    X,
    Z,
    Unspecified,
}
impl PortalAxis {
    /// Admits the portal-axis value domain before model compilation.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "x" => Some(Self::X),
            "z" => Some(Self::Z),
            "unknown" => Some(Self::Unspecified),
            _ => None,
        }
    }
}

/// A compiled biome tint domain, independent of geometry and alpha.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum BlockRenderTint {
    #[default]
    None,
    Grass,
    Foliage,
    Birch,
    Evergreen,
    Dry,
}

/// Render membership compiled from the pinned palette and reviewed native rules.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlockRenderFact {
    pub kind: BlockRenderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<super::BlockModelVariant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_alias: Option<super::LegacyBlockAlias>,
    pub tint: BlockRenderTint,
    /// The incomplete fire catch map; component names alone do not expose catch chances.
    pub fire_flammable: bool,
    /// Explicit additions to the current fire top-support fallback.
    pub fire_top: bool,
    #[serde(default)]
    pub flat_icon: bool,
    /// Membership of the provisional sword-speed override in the destroy carrier.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fast_sword: bool,
}

/// Shared route attributes used by descriptor and fallback selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlockRenderRoute {
    pub model: bool,
    pub cutout: bool,
    pub priority_cutout: bool,
    /// A dedicated rule draws every member, so the provisional envelope never applies.
    pub overrides_fallback: bool,
}

impl BlockRenderKind {
    /// Gives each dedicated shape one route policy, independent of its identifier.
    pub const fn route(self) -> BlockRenderRoute {
        use BlockRenderKind as R;
        let (model, cutout, priority_cutout, overrides_fallback) = match self {
            R::AuthoredModel
            | R::RespawnAnchor
            | R::Shelf
            | R::SeaPickle
            | R::BrewingStand
            | R::Campfire
            | R::Grindstone
            | R::TripwireHook
            | R::Lever
            | R::EndRod
            | R::LightningRod => (true, true, true, true),
            R::GrassCross
            | R::Chain
            | R::Ladder
            | R::Seagrass
            | R::Kelp
            | R::CoralFan
            | R::CoralWallFan
            | R::LilyPad => (true, true, false, true),
            R::Crystal => (true, true, false, true),
            R::CommandBlock => (true, false, false, true),
            R::EndPortalFrame | R::Lantern => (true, true, true, true),
            R::FlowerPot | R::Azalea | R::Bamboo => (false, false, true, false),
            R::StainedGlass
            | R::ClearGlass
            | R::CopperGrate
            | R::Ice
            | R::FrostedIce
            | R::Slime
            | R::Honey
            | R::TintedGlass
            | R::PowderSnow
            | R::GrassPath
            | R::SnowLayer
            | R::NamedCube
            | R::CopperBulb
            | R::RedstoneBase
            | R::CandleCake
            | R::Portal => (true, false, false, true),
            R::BubbleColumn
            | R::Barrel
            | R::Composter
            | R::Cauldron
            | R::LiteralCube
            | R::Invisible
            | R::EnchantingTable
            | R::EntityDrawn => (false, false, false, true),
            R::Ordinary
            | R::StandingSign
            | R::WallSign
            | R::HangingSign
            | R::BambooFence
            | R::BambooGate
            | R::NetherBrickFence
            | R::StainedPane
            | R::Water
            | R::Lava
            | R::Anvil
            | R::DragonEgg
            | R::Fire
            | R::PitcherCrop
            | R::Stem
            | R::Cactus
            | R::Cake
            | R::Farmland
            | R::BeeHousing
            | R::ResinClump
            | R::ChiseledBookshelf
            | R::Roots
            | R::Grass
            | R::Mycelium
            | R::SelectorAliasCube
            | R::MineralCube
            | R::Snow
            | R::BerryBush
            | R::DefaultRail
            | R::Tnt
            | R::DeprecatedPillar
            | R::CropRows
            | R::Torchflower
            | R::PaleMossCarpet => (false, false, false, false),
        };
        BlockRenderRoute {
            model,
            priority_cutout,
            cutout,
            overrides_fallback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_render_facts_cover_registered_variants_and_tint_domains() {
        let records = crate::read_registry_for_protocol(
            crate::pinned_block_registry_bytes(),
            crate::active_content_registry_protocol(),
        )
        .unwrap();
        let names: std::collections::BTreeSet<_> =
            records.iter().map(|record| record.name.as_ref()).collect();
        let mut counts = BTreeMap::new();
        for record in &records {
            let fact = block_render_fact(&record.name);
            if record.name.ends_with("_chain") {
                assert_eq!(fact.kind, BlockRenderKind::Chain);
                assert!(fact.kind.route().cutout);
            }
            if record.name.ends_with("_shelf") {
                assert_eq!(fact.kind, BlockRenderKind::Shelf);
                assert!(fact.kind.route().overrides_fallback);
            }
            if record.name.ends_with("candle_cake") {
                assert_eq!(fact.kind, BlockRenderKind::CandleCake);
                assert!(
                    names.contains(record.name.strip_suffix("_cake").unwrap()),
                    "missing candle companion for {}",
                    record.name
                );
            }
            *counts.entry(format!("{:?}", fact.kind)).or_insert(0usize) += 1;
        }
        for kind in [
            BlockRenderKind::Chain,
            BlockRenderKind::Shelf,
            BlockRenderKind::CandleCake,
            BlockRenderKind::Crystal,
            BlockRenderKind::LightningRod,
            BlockRenderKind::Lantern,
        ] {
            assert!(counts[&format!("{kind:?}")] > 0);
        }
        for (name, tint) in [
            ("minecraft:birch_leaves", BlockRenderTint::Birch),
            ("minecraft:spruce_leaves", BlockRenderTint::Evergreen),
            ("minecraft:leaf_litter", BlockRenderTint::Dry),
        ] {
            assert!(records.iter().any(|record| record.name.as_ref() == name));
            assert_eq!(block_render_fact(name).tint, tint);
        }
    }

    #[test]
    fn portal_axis_admission_covers_the_pinned_value_domain() {
        let super::super::PropertyDomain::String(values) =
            super::super::BlockProperty::PortalAxis.domain()
        else {
            panic!("portal axis domain");
        };
        for value in values {
            assert!(PortalAxis::from_name(value).is_some(), "{value}");
        }
    }

    #[test]
    fn compiled_model_metadata_covers_every_typed_backing() {
        use crate::facts::{BlockEntityFacts, BlockEntityRenderer as R, BlockModelVariant as V};
        for entry in BlockEntityFacts::pinned().entries() {
            for name in &entry.backing_blocks {
                let variant = block_render_fact(name).variant;
                assert!(
                    match entry.renderer {
                        R::Skull => matches!(variant, Some(V::Skull(_))),
                        R::Chest => matches!(variant, Some(V::Chest(_))),
                        R::Statue => matches!(variant, Some(V::Statue(_))),
                        _ => true,
                    },
                    "{name}: missing model metadata"
                );
            }
        }
    }

    #[test]
    fn ordinary_blocks_do_not_inherit_a_dedicated_visual_route() {
        let fact = block_render_fact("minecraft:stone");
        assert_eq!(fact.kind, BlockRenderKind::Ordinary);
        assert_eq!(fact.kind.route(), BlockRenderRoute::default());
        assert!(fact.fire_top);
    }
}

#[cfg(test)]
mod destroy_tests {
    #[test]
    fn compiled_fast_sword_membership_preserves_the_pinned_destroy_override() {
        for record in crate::read_registry_for_protocol(
            crate::pinned_block_registry_bytes(),
            crate::active_content_registry_protocol(),
        )
        .unwrap()
        {
            assert_eq!(
                super::block_render_fact(&record.name).fast_sword,
                matches!(
                    record.name.as_ref(),
                    "minecraft:bamboo" | "minecraft:bamboo_sapling"
                ),
                "{}",
                record.name
            );
        }
    }
}

//! Typed pack rules shared by registry validation and cold asset compilation.

use super::rules::BLOCK_POLICY_BYTES;
use crate::{EntityRenderMaterial, EntityRenderMaterialState};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

macro_rules! domain {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
        pub enum $name { $($variant),+ }
        impl $name {
            /// Every contract slot, used to reject an incomplete policy.
            pub const ALL: &[Self] = &[$(Self::$variant),+];
        }
    };
}

domain!(PackTexture {
    PaleMossCarpetSide,
    BambooFenceGate,
    BeaconCore,
    BeaconShell,
    BeeNestSide,
    BeeNestBottom,
    BeeNestTop,
    BeeNestFront,
    BeehiveSide,
    BeehiveTop,
    BeehiveFront,
    ChiseledBookshelfSide,
    ChiseledBookshelfTop,
    ChiseledBookshelfFront,
    CactusSide,
    CactusBottom,
    CactusTop,
    CakeSide,
    CakeBottom,
    CakeTop,
    CakeWest,
    FarmlandSide,
    Farmland,
    GrassSide,
    MyceliumSide,
    ResinClump,
    PowderSnow,
    StrawBed
});
domain!(PackBlock {
    Beacon,
    Obsidian,
    Glass,
    BeeNest,
    Beehive,
    ChiseledBookshelf,
    Cactus,
    Cake,
    Farmland,
    ResinClump,
    Dirt,
    CobblestoneWall,
    Air,
    Stone,
    StrawBed
});
domain!(TextureSelector {
    Door,
    Flower,
    Sapling,
    Powered,
    FrostedIce,
    Wheat,
    Stem,
    CropStages,
    Torchflower
});

/// Identifies a furnace-family cube (furnace, blast furnace, smoker) and whether its front
/// uses lit artwork, from the registry policy's render routes.
pub fn furnace_lit(identifier: &str) -> Option<bool> {
    match super::block_render_fact(identifier).kind {
        super::BlockRenderKind::Furnace => Some(false),
        super::BlockRenderKind::LitFurnace => Some(true),
        _ => None,
    }
}

/// Native material aliases whose names overlap actor and item identifiers.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum MaterialKind {
    UnmaskedArmor,
    AlphaTestNoCull,
    AlphaTest,
    AlphaBlend,
    Wind,
    Charged,
    Dragon,
    DissolveDepth,
    DissolveColor,
}

/// The precedence of a block item over its legacy sprite.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum ItemVisualKind {
    EmptyHand,
    BlockModel,
    Sprite,
}

/// A pinned mineral's native texture and wire identity.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MineralRule {
    pub network_hash: u32,
    pub identifier: String,
    pub key: String,
    pub path: String,
}

/// The exact blocks.json texture form admitted by a retained cube route.
#[derive(Debug, Deserialize, Eq, PartialEq)]
pub enum AliasCubeSource {
    Scalar(String),
    Pillar([String; 3]),
}

/// Pack-derived names and ordered native selectors with one checked policy owner.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackRules {
    alias_cubes: BTreeMap<String, AliasCubeSource>,
    alias_paths: BTreeMap<String, String>,
    textures: BTreeMap<PackTexture, String>,
    alpha_textures: Vec<PackTexture>,
    blocks: BTreeMap<PackBlock, String>,
    selectors: BTreeMap<String, TextureSelector>,
    variants: BTreeMap<TextureSelector, Vec<String>>,
    mushrooms: BTreeMap<String, String>,
    powered_prefix: String,
    materials: BTreeMap<String, MaterialKind>,
    egg_aliases: BTreeMap<String, String>,
    superseded_eggs: BTreeMap<String, String>,
    pub minerals: Vec<MineralRule>,
    item_visuals: BTreeMap<String, ItemVisualKind>,
    empty_hand: PackBlock,
}

/// Loads the compiler's embedded pack contracts once for asset admission.
pub fn rules() -> &'static PackRules {
    #[derive(Deserialize)]
    struct Policy {
        pack_rules: PackRules,
    }
    static RULES: OnceLock<PackRules> = OnceLock::new();
    RULES.get_or_init(|| {
        serde_json::from_slice::<Policy>(BLOCK_POLICY_BYTES)
            .expect("valid pack policy")
            .pack_rules
    })
}

impl PackTexture {
    /// Resolves a typed terrain slot through its checked pack rule.
    pub fn key(self) -> &'static str {
        &rules().textures[&self]
    }
}
impl PackBlock {
    /// Returns the canonical registry identity required by this pack contract.
    pub fn identifier(self) -> &'static str {
        &rules().blocks[&self]
    }
    /// Returns the unnamespaced blocks.json key for this contract.
    pub fn bare(self) -> &'static str {
        self.identifier()
            .strip_prefix("minecraft:")
            .expect("vanilla block namespace")
    }
}
impl PackRules {
    /// Resolves the exact source shape of a retained selector-alias cube.
    pub fn alias_cube(&self, identifier: &str) -> Option<&AliasCubeSource> {
        self.alias_cubes.get(identifier)
    }
    /// Resolves its validated terrain key to the pinned texture path.
    pub fn alias_path(&self, key: &str) -> Option<&str> {
        self.alias_paths.get(key).map(String::as_str)
    }
    /// Resolves a texture array's declared selector before any state is read.
    pub fn selector(&self, key: &str) -> Option<TextureSelector> {
        self.selectors.get(key).copied()
    }
    /// Gives a split registry identity its retained legacy array index.
    pub fn variant(&self, selector: TextureSelector, identifier: &str) -> Option<usize> {
        self.variants.get(&selector)?.iter().position(|name| {
            name.strip_prefix("minecraft:").unwrap_or(name)
                == identifier.strip_prefix("minecraft:").unwrap_or(identifier)
        })
    }
    /// Finds the huge-mushroom family bound to an authored face key.
    pub fn mushroom_block(&self, key: &str) -> Option<&str> {
        self.mushrooms.iter().find_map(|(prefix, block)| {
            key.strip_prefix(prefix)
                .filter(|face| {
                    matches!(
                        *face,
                        "west" | "east" | "bottom" | "top" | "north" | "south"
                    )
                })
                .map(|_| block.as_str())
        })
    }
    /// Retains the legacy powered-array prefix during cold state admission.
    pub fn is_powered(&self, identifier: &str) -> bool {
        identifier
            .strip_prefix("minecraft:")
            .unwrap_or(identifier)
            .starts_with(&self.powered_prefix)
    }
    /// Tests native alpha consumers beyond the material's authored flags.
    pub fn supports_alpha(&self, key: &str) -> bool {
        self.alpha_textures
            .iter()
            .any(|slot| self.textures[slot] == key)
    }
    /// Resolves a native material alias without reclassifying it as an actor.
    pub fn material(&self, name: &str) -> Option<MaterialKind> {
        self.materials.get(name).copied()
    }
    /// Selects the shader contract for exact authored native material targets.
    pub fn raster(&self, name: &str) -> EntityRenderMaterial {
        match self.material(name) {
            Some(MaterialKind::Dragon) => EntityRenderMaterial::Dragon,
            Some(MaterialKind::DissolveDepth) => EntityRenderMaterial::DissolveDepth,
            Some(MaterialKind::DissolveColor) => EntityRenderMaterial::DissolveColor,
            _ => EntityRenderMaterial::Default,
        }
    }
    /// Resolves built-in material aliases before reading their policy state.
    pub fn material_state(&self, name: &str) -> Option<EntityRenderMaterialState> {
        let name = match name {
            "harness" => "entity_alphatest",
            _ => name,
        };
        let mut state = EntityRenderMaterialState::default();
        match self.material(name)? {
            MaterialKind::AlphaTestNoCull => {
                state.alpha_test = true;
                state.cull = false;
            }
            MaterialKind::AlphaTest => state.alpha_test = true,
            MaterialKind::AlphaBlend => state.blend = true,
            MaterialKind::Wind => {
                state.alpha_test = true;
                state.blend = true;
                state.cull = false;
                state.depth_write = false;
                state.disable_overlay = true;
            }
            MaterialKind::Charged => {
                state.alpha_test = true;
                state.cull = false;
                state.blend = true;
                state.additive = true;
                state.disable_overlay = true;
            }
            _ => return None,
        }
        Some(state)
    }
    /// Resolves the actor-to-retail egg rename, retaining ordinary suffix routing.
    pub fn egg_identifier(&self, actor: &str) -> String {
        self.egg_aliases
            .get(actor)
            .cloned()
            .unwrap_or_else(|| format!("{actor}_spawn_egg"))
    }
    /// Returns the newer actor definition that supersedes a compatibility egg.
    pub fn superseding_egg(&self, actor: &str) -> Option<&str> {
        self.superseded_eggs.get(actor).map(String::as_str)
    }
    /// Resolves the exact legacy item presentation exceptions at compile time.
    pub fn item_visual(&self, identifier: &str) -> Option<ItemVisualKind> {
        if identifier == self.empty_hand() {
            Some(ItemVisualKind::EmptyHand)
        } else {
            self.item_visuals.get(identifier).copied()
        }
    }
    /// Returns the canonical empty-hand item identity for constructing its route.
    pub fn empty_hand(&self) -> &str {
        &self.blocks[&self.empty_hand]
    }
    /// Finds a pinned mineral contract without copying its identity list.
    pub fn mineral(&self, identifier: &str) -> Option<&MineralRule> {
        self.minerals
            .iter()
            .find(|rule| rule.identifier == identifier)
    }
}

/// Derives native leather sprite tinting from the admitted item material.
pub fn leather_sprite(path: &str) -> bool {
    let Some(stem) = path.strip_prefix("textures/items/").and_then(|name| {
        name.strip_suffix(".png")
            .or_else(|| name.strip_suffix(".tga"))
    }) else {
        return false;
    };
    let facts = super::ItemFacts::builtin().expect("valid item facts");
    facts
        .by_identifier(&format!("minecraft:{stem}"))
        .is_some_and(|fact| fact.is_leather())
}

#[cfg(test)]
mod tests;
/// Resolves the candle paired with a registered composite cake route.
pub fn candle_companion(identifier: &str) -> Option<&str> {
    (super::block_render_fact(identifier).kind == super::BlockRenderKind::CandleCake)
        .then(|| identifier.strip_suffix("_cake"))
        .flatten()
}

mod validation;

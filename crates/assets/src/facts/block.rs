//! Numeric block identity and interaction lookups.

use crate::{
    AssetError,
    registry::{BlockTraits, ModelFamily, RegistryRecord, SurvivalRule},
};

/// A compiled palette identity, or a named descriptor arriving at an admission boundary.
#[derive(Debug, Clone, Copy)]
pub enum BlockStateIdentity<'a> {
    Compiled(u32),
    Named(&'a str),
}
/// Canonical empty-block identity checked against the pinned air trait.
pub const AIR_IDENTIFIER: &str = "minecraft:air";

/// Placeholder identity for a block state the client has not admitted.
pub const RESERVED_BLOCK_NAME: &str = "cinnabar:reserved";

/// Resolves the ordinary digging tools a custom block's declared tags accelerate.
pub fn custom_block_effective_tools<'a>(tags: impl IntoIterator<Item = &'a str>) -> u8 {
    use crate::compiled::block_destroy::ToolKind;
    tags.into_iter().fold(0, |mask, tag| {
        mask | match tag {
            "minecraft:is_pickaxe_item_destructible" => ToolKind::Pickaxe.bit(),
            "minecraft:is_hatchet_item_destructible" => ToolKind::Axe.bit(),
            "minecraft:is_shovel_item_destructible" => ToolKind::Shovel.bit(),
            "minecraft:is_hoe_item_destructible" => ToolKind::Hoe.bit(),
            _ => 0,
        }
    })
}

/// Identity, placement and support metadata for one runtime ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockFact {
    /// Whether this runtime ID has readable block metadata, including custom and reserved entries.
    pub readable: bool,
    pub traits: BlockTraits,
    pub family: ModelFamily,
    pub survival: SurvivalRule,
    pub render: super::BlockRenderFact,
    pub ambient: super::presentation::AmbientBlock,
    /// Species-authored falling leaves; ordinary biome-tinted leaves use their world material.
    pub leaf_particle: Option<super::ParticleEffect>,
    pub placement: super::PlacementRule,
    pub support: super::SupportKind,
    /// Sequential identity of the admitted double slab or matching sign.
    pub alternate: Option<u32>,
    /// First sequential state of this block identity, shared by its variants.
    pub identity: Option<u32>,
}

impl Default for BlockFact {
    /// Unknown runtime ids retain unresolved placement support.
    fn default() -> Self {
        Self {
            readable: false,
            traits: BlockTraits::empty(),
            family: ModelFamily::Unknown,
            survival: SurvivalRule::Unresolved,
            render: super::BlockRenderFact::default(),
            ambient: super::presentation::AmbientBlock::Other,
            leaf_particle: None,
            placement: super::PlacementRule::Unresolved,
            support: super::SupportKind::Unresolved,
            alternate: None,
            identity: None,
        }
    }
}

impl BlockFact {
    /// Retains readable custom support without inventing a compiled placement identity.
    pub fn custom(identifier: &str) -> Self {
        Self {
            readable: true,
            support: super::SupportKind::for_identifier(identifier),
            ..Self::default()
        }
    }
}

/// A compact array indexed by the carrier's sequential runtime id.
#[derive(Debug, Clone)]
pub struct BlockFacts {
    records: Box<[BlockFact]>,
}

impl BlockFacts {
    /// Copies facts after requiring a dense, ordered runtime-id palette.
    pub fn new(records: &[RegistryRecord]) -> Result<Self, AssetError> {
        if records
            .iter()
            .enumerate()
            .any(|(id, r)| r.sequential_id as usize != id)
        {
            return Err(AssetError::InvalidRegistryFlags(0xff));
        }
        let catalog =
            super::placement::placement_catalog(records, &super::rules::policy().placement_rules)?;
        let mut names = std::collections::BTreeMap::new();
        for record in records
            .iter()
            .filter(|r| r.name.as_ref() != RESERVED_BLOCK_NAME)
        {
            names
                .entry(record.name.as_ref())
                .or_insert(record.sequential_id);
        }
        Ok(Self {
            records: records
                .iter()
                .map(|r| {
                    let mut fact = BlockFact {
                        readable: true,
                        traits: r.traits,
                        family: r.model_family,
                        survival: r.survival,
                        render: super::block_render_fact(&r.name),
                        ambient: super::presentation_generated::ambient_block(&r.name),
                        leaf_particle: (r.model_family == ModelFamily::Leaves)
                            .then(|| super::presentation::leaf_particle(&r.name))
                            .flatten(),
                        ..BlockFact::default()
                    };
                    fact.identity = names.get(r.name.as_ref()).copied();
                    if let Some(entry) = catalog.get(r.name.as_ref()) {
                        fact.placement = entry.rule;
                        fact.support = entry.support;
                        fact.alternate = entry
                            .alternate
                            .as_deref()
                            .and_then(|name| names.get(name).copied());
                    }
                    fact
                })
                .collect(),
        })
    }

    /// Returns one array entry, or unresolved facts for an unknown runtime id.
    pub fn get(&self, state: u32) -> BlockFact {
        self.records
            .get(state as usize)
            .copied()
            .unwrap_or_default()
    }

    /// Reports the number of admitted runtime ids.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Reports whether the palette contains no runtime ids.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

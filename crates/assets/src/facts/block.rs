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
/// Placeholder identity for a block state the client has not admitted.
pub const RESERVED_BLOCK_NAME: &str = "cinnabar:reserved";

/// Compiled facts for one admitted block state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockFact {
    pub traits: BlockTraits,
    pub family: ModelFamily,
    pub survival: SurvivalRule,
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
            traits: BlockTraits::empty(),
            family: ModelFamily::Unknown,
            survival: SurvivalRule::Unresolved,
            placement: super::PlacementRule::Unresolved,
            support: super::SupportKind::Unresolved,
            alternate: None,
            identity: None,
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
        let catalog = super::placement::catalog()?;
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
                        traits: r.traits,
                        family: r.model_family,
                        survival: r.survival,
                        ..BlockFact::default()
                    };
                    fact.identity = names.get(r.name.as_ref()).copied();
                    if let Some(entry) = catalog.get(r.name.as_ref()) {
                        fact.placement = entry.rule;
                        fact.support = entry.support;
                        fact.alternate = super::placement::alternate(entry, &names);
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

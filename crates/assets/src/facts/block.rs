//! Numeric block identity and interaction lookups.

use crate::{
    AssetError,
    registry::{BlockTraits, ModelFamily, RegistryRecord, SurvivalRule},
};

/// Compiled facts for one admitted block state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockFact {
    pub traits: BlockTraits,
    pub family: ModelFamily,
    pub survival: SurvivalRule,
}

impl Default for BlockFact {
    /// Unknown runtime ids retain unresolved placement support.
    fn default() -> Self {
        Self {
            traits: BlockTraits::empty(),
            family: ModelFamily::Unknown,
            survival: SurvivalRule::Unresolved,
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
        Ok(Self {
            records: records
                .iter()
                .map(|r| BlockFact {
                    traits: r.traits,
                    family: r.model_family,
                    survival: r.survival,
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

//! Pack ids for Cinnabar's Bedrock identities: the `mc_Entity` table of the Bedrock block
//! states, each state's Java block state from the pinned mapping looked up in the pack's
//! [`BlockIdMap`]; and the pack ids of Bedrock entities and items through their Java ids.

use super::{BlockIdMap, NamedIdMap};
use assets::facts::JavaBlockMapping;

/// The `mc_Entity` value of a block state no pack entry (or no Java state) matches.
pub const UNMAPPED_BLOCK_ID: i32 = -1;

impl BlockIdMap {
    /// The pack block id of every Bedrock block state, indexed by block-registry
    /// sequential id (`assets::RegistryRecord::sequential_id`, Cinnabar's dense block state
    /// index): `Vec<i32>` with one value per registry state, [`UNMAPPED_BLOCK_ID`] where the
    /// state has no Java counterpart or the pack assigns it no id. Ready to upload as the
    /// per-state `mc_Entity` lookup.
    pub fn bedrock_block_ids(&self, mapping: &JavaBlockMapping) -> Vec<i32> {
        let mut properties = Vec::new();
        mapping
            .block_states()
            .map(|state| {
                let Some(state) = state else {
                    return UNMAPPED_BLOCK_ID;
                };
                properties.clear();
                properties.extend(state.properties());
                self.block_id(state.name(), &properties, &|tag| state.in_tag(tag))
                    .unwrap_or(UNMAPPED_BLOCK_ID)
            })
            .collect()
    }
}

impl NamedIdMap {
    /// The entity.properties id of a Bedrock entity identifier (`minecraft:ender_crystal`):
    /// the id of the Java entity type the mapping gives it (`minecraft:end_crystal`), or, for
    /// an identifier the mapping lacks, the id of that same name.
    pub fn bedrock_entity_id(
        &self,
        mapping: &JavaBlockMapping,
        bedrock_identifier: &str,
    ) -> Option<i32> {
        self.id(mapping
            .java_entity(bedrock_identifier)
            .unwrap_or(bedrock_identifier))
    }

    /// The item.properties id of a Bedrock item identifier (`minecraft:web`): the id of the
    /// Java item the mapping gives it (`minecraft:cobweb`), or, for an identifier the mapping
    /// lacks, the id of that same name.
    pub fn bedrock_item_id(
        &self,
        mapping: &JavaBlockMapping,
        bedrock_identifier: &str,
    ) -> Option<i32> {
        self.id(mapping
            .java_item(bedrock_identifier)
            .unwrap_or(bedrock_identifier))
    }
}

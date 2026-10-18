//! Liquid transition facts derived from the pinned block registry.

use std::sync::OnceLock;

use crate::{
    NetworkIdMode,
    facts::{BlockProperty, StateValue},
    registry::BlockTraits,
};

/// Admitted primary-block properties needed to maintain the secondary water layer.
#[derive(Clone, Copy, Debug)]
pub struct BlockLiquidFacts {
    pub air: bool,
    pub water: bool,
    pub water_source: bool,
    pub can_contain_liquid_source: bool,
}

/// Looks up built-in facts in the session's internal ID space; custom states stay unresolved.
pub fn pinned_facts(mode: NetworkIdMode, runtime_id: u32) -> Option<BlockLiquidFacts> {
    static FACTS: OnceLock<Box<[Option<BlockLiquidFacts>]>> = OnceLock::new();
    let facts = FACTS.get_or_init(|| {
        crate::read_registry_for_protocol(
            crate::pinned_block_registry_bytes(),
            crate::active_content_registry_protocol(),
        )
        .expect("validated pinned block registry")
        .iter()
        .map(|record| {
            (record.name.as_ref() != crate::facts::block::RESERVED_BLOCK_NAME).then(|| {
                let water = record.traits.contains(BlockTraits::WATER);
                BlockLiquidFacts {
                    air: record.traits.contains(BlockTraits::AIR),
                    water,
                    water_source: water
                        && record.property(BlockProperty::LiquidDepth) == Some(&StateValue::Int(0)),
                    can_contain_liquid_source: record
                        .traits
                        .contains(BlockTraits::CAN_CONTAIN_LIQUID_SOURCE),
                }
            })
        })
        .collect()
    });
    let id = match mode {
        NetworkIdMode::Sequential => runtime_id,
        NetworkIdMode::Hashed => crate::pinned_block_sequential_id(runtime_id)?,
    };
    facts.get(id as usize).copied().flatten()
}

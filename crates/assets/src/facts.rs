//! Validated content facts compiled from the pinned registries.

mod item;
mod item_carrier;
pub use item::{
    ContainerRoles, ItemFact, ItemFactRecord, ItemFactsPayload, ItemRepairMaterial, ItemTag,
    ItemTagDefinition, ItemTagSet, ItemTier, ItemTraits, ItemUseAnimationFact,
};
pub use item_carrier::{
    ITEM_FACTS_EXPANSION_PATH, ITEM_FACTS_FILENAME, ITEM_FACTS_INPUTS, ITEM_FACTS_SOURCE_PATH,
    ItemFacts, ItemFactsError, MAX_ITEM_FACTS_BYTES, encode_item_facts,
};

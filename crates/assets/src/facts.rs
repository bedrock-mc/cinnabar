//! Validated block and item facts compiled from the pinned registries.

pub mod block;
pub mod shape;
pub use block::{BlockFact, BlockFacts};

pub(crate) mod family_routing;
mod properties;
mod properties_generated;
mod property_domains_generated;

pub(crate) use properties::decode;
pub use properties::{PropertyDomain, StateValue};
pub use properties_generated::BlockProperty;

mod item;
mod item_carrier;
pub use item::{
    ContainerRoles, FILLED_MAP_IDENTIFIER, ItemFact, ItemFactRecord, ItemFactsPayload,
    ItemRepairMaterial, ItemTag, ItemTagDefinition, ItemTagSet, ItemTier, ItemTraits,
    ItemUseAnimationFact, TOTEM_IDENTIFIER,
};
pub use item_carrier::{
    ITEM_FACTS_EXPANSION_PATH, ITEM_FACTS_FILENAME, ITEM_FACTS_INPUTS, ITEM_FACTS_SOURCE_PATH,
    ItemFacts, ItemFactsError, MAX_ITEM_FACTS_BYTES, encode_item_facts,
};

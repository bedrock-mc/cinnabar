//! Validated block and item facts compiled from the pinned registries.

pub mod block;
pub mod liquid;
pub mod rules;
pub use block::{BlockFact, BlockFacts, BlockStateIdentity};
mod placement;
pub use placement::{BLOCK_PLACEMENT_SOURCE_PATH, PlacementRule, SupportKind, placement_catalog};
pub mod shape;

pub(crate) mod family_routing;
mod properties;
mod properties_generated;
mod property_domains_generated;

pub(crate) use properties::decode;
pub use properties::{PropertyDomain, StateValue, block_property_domain};
pub use properties_generated::BlockProperty;

pub mod entity;
mod item;
mod item_carrier;
pub use item::{
    ARROW_IDENTIFIER, ArmourMaterial, BANNER_IDENTIFIER, ContainerRoles, EMPTY_BUCKET_IDENTIFIER,
    FILLED_MAP_IDENTIFIER, FISHING_ROD_IDENTIFIER, HARNESS_TAG, ItemFact, ItemFactRecord,
    ItemFactsPayload, ItemRepairMaterial, ItemTag, ItemTagDefinition, ItemTagSet, ItemTier,
    ItemTraits, ItemUseAnimationFact, PotionForm, TOTEM_IDENTIFIER, harness_color,
};
pub use item_carrier::{
    ITEM_FACTS_EXPANSION_PATH, ITEM_FACTS_FILENAME, ITEM_FACTS_INPUTS, ITEM_FACTS_SOURCE_PATH,
    ItemFacts, ItemFactsError, MAX_ITEM_FACTS_BYTES, encode_item_facts,
};

mod block_entity;
pub use block_entity::{
    BLOCK_ENTITY_FACTS_PATH, BlockEntityBacking, BlockEntityDiagnostic, BlockEntityEntry,
    BlockEntityFacts, BlockEntityFactsPayload, BlockEntityGeometry, BlockEntityMount,
    BlockEntityRenderer, BlockEntityState,
};

mod events;
mod events_generated;
pub use events::{
    legacy_particle_effect, level_event_sound, level_particle_effect, note_instrument,
};
pub use events_generated::ParticleEffect;

mod render;
pub use render::{
    BlockRenderFact, BlockRenderKind, BlockRenderRoute, BlockRenderTint, PortalAxis,
    block_render_fact,
};

mod model_variant;
pub use model_variant::{BlockModelVariant, ChestKind, CopperAge, LegacyBlockAlias};

mod identities_generated;
pub use identities_generated::{VanillaBlock, VanillaEntity, VanillaItem};
pub mod presentation;

mod presentation_generated;
pub use presentation_generated::{UiItemIcon, dimension_fallback_biome, dimension_id};

pub mod java_mapping;
pub use java_mapping::{JavaBlockMapping, JavaBlockState, java_biome, java_block_state};

/// Pack texture selectors and native presentation contracts checked against pinned inputs.
pub mod pack;

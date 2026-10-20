//! Runtime pack admission, compilation, and reload dependency tracking without Bevy.

#[cfg(test)]
#[global_allocator]
static TEST_ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

mod block_mining;
mod block_physics;
pub use block_mining::{CustomMiningFilters, custom_mining_seconds};
pub mod compile_cache;
pub mod entity_pack;
pub mod pack_flipbooks;
pub mod pack_language;
mod pack_preparation;
pub mod pack_skybox;
pub mod pack_textures;
pub mod reload_diff;

pub use pack_preparation::{
    BootstrapGenerationDisposition, PackInputs, PackPreparation, RequiredPackRejected,
    ResourcePackAdmissionState, classify_bootstrap_generation, custom_block_items,
    prepare_session_packs, required_packs_applied, stack_fingerprint,
};

#[cfg(test)]
mod test_support;

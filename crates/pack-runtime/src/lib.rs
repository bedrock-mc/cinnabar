//! Runtime pack admission, compilation, and reload dependency tracking without Bevy.

mod block_physics;
pub mod compile_cache;
pub mod entity_pack;
pub mod pack_language;
mod pack_preparation;
pub mod pack_skybox;
pub mod pack_textures;
pub mod reload_diff;

pub use pack_preparation::{
    BootstrapGenerationDisposition, PackInputs, PackPreparation, RequiredPackRejected,
    ResourcePackAdmissionState, StackFingerprint, classify_bootstrap_generation,
    custom_block_items, prepare_session_packs, required_packs_applied, stack_fingerprint,
};

#[cfg(test)]
mod test_support;

pub mod language_carrier;

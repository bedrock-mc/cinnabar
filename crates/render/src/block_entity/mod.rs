//! GPU passes for block models and selection overlays.
mod gpu;
use bevy::render::render_resource::{BlendComponent, BlendFactor, BlendOperation, BlendState};
pub use gpu::BlockEntityRenderPlugin;
#[cfg(feature = "shaderpacks")]
pub(crate) use gpu::{
    BlockEntityGpu, prepare_resources as prepare_block_entity_resources, shaderpack_block_entities,
};

/// Straight-alpha additive composition used by the untextured death effect.
pub const DRAGON_DEATH_BLEND: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::SrcAlpha,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::SrcAlpha,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
};

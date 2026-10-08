//! Engine-independent contracts shared by world publication and rendering.
//!
//! This crate owns publication authority, pacing bounds and shared skin layout
//! rules. It has no dependencies and must not acquire game state or GPU types.

mod actor_lighting;
pub mod primitive_shapes;
mod publication;
mod quality;
mod skin;

pub use actor_lighting::{ACTOR_SHADE_COEFFICIENTS, fancy_actor_shade};
pub use publication::{
    PublicationAllowance, PublicationPermit, PublicationPermitStage, PublicationServiceConfig,
};
pub use quality::EnhancedQuality;
pub use skin::{
    CLASSIC_SKIN_SIDE, MAX_CLASSIC_SKIN_SIDE, MAX_SKIN_ANIMATION_LAYERS, MAX_STANDARD_SKIN_SIDE,
    SkinRgba8, expand_legacy_skin_rgba8,
};

/// Maximum view radius supported by the initial world streaming pipeline.
pub const PHASE0_MAX_VIEW_RADIUS_CHUNKS: i32 = 16;

//! Engine-independent contracts shared by world publication and rendering.
//!
//! This crate owns publication authority, pacing bounds and shared skin layout
//! rules. It has no dependencies and must not acquire game state or GPU types.

mod actor_lighting;
mod frame_rate_limit;
pub mod primitive_shapes;
mod publication;
mod skin;
mod vrr;

pub use actor_lighting::{
    ACTOR_LIGHT_DIRECTIONAL, ACTOR_LIGHT_WORLD, ACTOR_SHADE_COEFFICIENTS, fancy_actor_shade,
};
pub use frame_rate_limit::FrameRateLimit;
pub use publication::{
    PublicationAllowance, PublicationPermit, PublicationPermitStage, PublicationServiceConfig,
};
pub use skin::{
    CLASSIC_SKIN_SIDE, MAX_CLASSIC_SKIN_SIDE, MAX_SKIN_ANIMATION_LAYERS, MAX_STANDARD_SKIN_SIDE,
    SkinRgba8, expand_legacy_skin_rgba8,
};
pub use vrr::VrrPreference;

/// Largest render distance the client offers and requests; the server grant still bounds streaming.
pub const PHASE0_MAX_VIEW_RADIUS_CHUNKS: i32 = 255;
/// Retention radius before the server grants one, so an unbounded maximum never pins old terrain.
pub const UNGRANTED_VIEW_RADIUS_CHUNKS: i32 = 16;

/// Near clipping distance shared by world and first-person camera projections.
pub const CAMERA_NEAR_PLANE_BLOCKS: f32 = 0.025;

/// Separation that keeps block selection and destroy overlays in front of their surfaces.
pub const BLOCK_OVERLAY_FACE_OFFSET: f32 = 0.002;

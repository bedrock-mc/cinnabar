//! Portable pixel normalization and publication capabilities shared by protocol and rendering.

mod publication;
mod skin;

pub use publication::{
    PublicationAllowance, PublicationPermit, PublicationPermitStage, PublicationServiceConfig,
};
pub use skin::{CLASSIC_SKIN_SIDE, MAX_CLASSIC_SKIN_SIDE, expand_legacy_skin_rgba8};

/// Local normalization ceiling for assembled persona skin rasters.
pub const MAX_STANDARD_SKIN_SIDE: u32 = 512;

/// Animated texture slots the vanilla player renderer adds to the base skin.
pub const MAX_SKIN_ANIMATION_LAYERS: usize = 3;

mod actor;
mod actor_status;
mod skin_model;
pub use actor::{
    ActorKind, ActorMetadataValue, PropertyDefinition, PropertyKind, actor_flag,
    actor_is_billboard, actor_render_scale, player_is_sleeping, target_rotation_is_absolute,
};
pub use actor_status::{
    ActorPickup, ActorStatus, DEATH_DURATION_TICKS, HURT_DURATION_TICKS, HURT_OVERLAY_ALPHA,
    PICKUP_DURATION_TICKS,
};
pub use skin_model::{SkinAnimation, SkinAnimationKind, SkinGeometrySource};

mod hand_phase;
pub use hand_phase::HandPhase;
pub mod item_use;

pub use actor::{
    ACTOR_FLAG_SNEAKING, ACTOR_FLAG_SPRINTING, ACTOR_FLAG_SWIMMING, ACTOR_FLAG_USING_ITEM,
};

pub use actor_status::HURT_OVERLAY_RGBA;

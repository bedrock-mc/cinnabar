//! Engine-independent Bedrock particle simulation: effect definitions, Molang-driven emitters,
//! world collision, triggers and camera-facing draw lists ready for GPU upload.

pub mod actor;
mod atlas;
mod def;
mod draw;
mod emitter;
mod library;
mod molang;
mod particle;
mod system;
mod triggers;
mod world;

pub mod ambient;
pub mod tiles;

pub use atlas::{ATLAS_SIDE, AtlasPatch};
pub use draw::{DrawLists, ParticleInstance, ParticleView};
pub use emitter::{ParticleSound, SpawnRequest, TileRequest};
pub use system::{MAX_LIVE_PARTICLES, ParticleSystem};
pub use triggers::{
    ITEM_ICON_PARTICLES, LevelParticle, block_break_request, block_crack_request, burst_requests,
    classify_level_event, crack_cadence_due, critical_hit_request, face_toward,
    is_particle_level_event, item_icon_request, named_request, parse_molang_variables,
    terrain_request,
};
pub use world::{EmptyWorld, Fluid, ParticleWorld};

#[cfg(test)]
mod explosion_tests;
#[cfg(test)]
mod snowball_tests;
#[cfg(test)]
mod terrain_tests;

//! Data-driven Bedrock particle engine: effect definitions, Molang-driven emitters, world
//! collision, and instanced billboard rendering.

mod atlas;
mod def;
mod draw;
mod emitter;
mod library;
mod molang;
mod particle;
mod render;
mod system;
mod tiles;
mod triggers;
mod world;

pub use atlas::ATLAS_SIDE;
pub use draw::{DrawLists, ParticleInstance, ParticleView};
pub use emitter::{ParticleSound, SpawnRequest, TileRequest};
pub use render::{ParticleGpuFrame, ParticleRenderPlugin, particle_view, update_particle_frame};
pub use system::{MAX_LIVE_PARTICLES, ParticleSystem};
pub use tiles::{block_particle_tile, item_particle_tile};
pub use triggers::{
    BLOCK_BREAK_EFFECT, ITEM_ICON_PIECES, LevelParticle, block_break_request, block_crack_request,
    classify_level_event, is_particle_level_event, item_icon_request, named_request,
    parse_molang_variables, terrain_request,
};
pub use world::{EmptyWorld, Fluid, ParticleWorld};

#[cfg(test)]
mod terrain_tests;

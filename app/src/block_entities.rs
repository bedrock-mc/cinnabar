//! Block-entity presentation: turns retained block-entity NBT, the backing block state,
//! server cues and break progress into renderer submissions each frame.
//!
//! The renderer carrier is optional; without it nothing is submitted and the world still
//! renders from block states alone.

mod containers;
mod cracks;
mod describe;
mod sign_text;
mod state;
mod system;

pub(crate) use system::{
    BlockEntityFont, BlockEntityRuntime, block_entity_scene, configure, load_block_entity_carrier,
};

//! Dropped-item model inputs built by presentation and drawn by the item renderer.
use std::sync::Arc;

mod build;
pub use build::{dropped_item_block_cube, dropped_item_block_model};

/// Packed RGBA8 multiplier that leaves a texel unchanged.
pub const OPAQUE_WHITE: u32 = 0xffff_ffff;

/// One item texture; identical sprites share a layer through their index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DroppedItemSprite {
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

/// A block as a unit cube: six square RGBA8 tiles in `West, East, Down, Up, North, South` order,
/// each multiplied by a packed RGBA8 tint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DroppedItemCube {
    pub tile: u32,
    pub faces: [Arc<[u8]>; 6],
    pub tints: [u32; 6],
}

/// A centered block mesh retaining its world templates and material tiles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DroppedItemBlock {
    pub materials: Arc<[(DroppedItemSprite, u32)]>,
    pub quads: Arc<[assets::ModelQuad]>,
    pub rotation: u32,
}

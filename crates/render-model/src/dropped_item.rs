//! Dropped-item scene data: extruded sprite meshes drawn as world-space instances.

use std::sync::Arc;

mod block;
mod mesh;
mod native;
mod rope;
pub use block::block_mesh;

pub use mesh::{
    ITEM_MESH_VERTEX_BYTES, ItemMeshVertex, cube_mesh, extruded_sprite_mesh,
    native_dropped_sprite_mesh,
};
pub use native::{DroppedItemShape, DroppedItemSpawnPose, native_dropped_item_transform};
pub use rope::{rope_color, rope_point, rope_ribbon};

/// Side length of every layer on the GPU; larger textures are rejected.
pub const MAX_ITEM_SPRITE_SIDE: u32 = 32;
/// Cap on GPU layers, including the reserved white layer.
pub const MAX_ITEM_LAYERS: usize = 512;
pub const MAX_DROPPED_ITEM_INSTANCES: usize = 1_024;
pub const MAX_DYNAMIC_ITEM_VERTICES: usize = 65_536;
/// Layer 0 is always opaque white so untextured dynamic geometry (lines) can use it.
pub const WHITE_LAYER: u32 = 0;

/// Geometry an instance can reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DroppedItemModel {
    /// Reclaimed atlas slot; live model indices on either side remain unchanged.
    Vacant,
    /// Legacy centered slab used by static placements, not native item actors.
    Sprite(DroppedItemSprite),
    /// Vanilla tessellated sprite frame after the ordinary dropped-item default transform.
    NativeSprite(DroppedItemSprite),
    Cube(DroppedItemCube),
    Block(DroppedItemBlock),
}

/// One drawn copy of a model: `world_from_item` maps the unit model into the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DroppedItemInstance {
    pub model: u32,
    pub world_from_item: [[f32; 4]; 3],
    pub block_level: u32,
    pub sky_level: u32,
    /// Packed RGBA8 colour blended over the lit result (see `pack_overlay_rgba8`); 0 disables it.
    pub overlay_rgba8: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainItemTransition {
    pub key: world::SubChunkKey,
    pub generation: u64,
    pub visible: bool,
}

/// A candidate whose visibility follows its ordered terrain publications.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainItemInstance {
    pub instance: DroppedItemInstance,
    pub visible: bool,
    pub transitions: Arc<[TerrainItemTransition]>,
}

/// Builds `world_from_item` from a centre, yaw about +Y (radians), and uniform scale.
#[must_use]
pub fn dropped_item_transform(center: [f32; 3], yaw_radians: f32, scale: f32) -> [[f32; 4]; 3] {
    let (sine, cosine) = yaw_radians.sin_cos();
    [
        [cosine * scale, 0.0, sine * scale, center[0]],
        [0.0, scale, 0.0, center[1]],
        [-sine * scale, 0.0, cosine * scale, center[2]],
    ]
}

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

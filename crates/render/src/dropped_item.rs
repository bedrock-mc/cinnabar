//! Dropped-item scene data: extruded sprite meshes drawn as world-space instances.
use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use render_model::{DroppedItemBlock, DroppedItemCube, DroppedItemSprite};
use std::sync::Arc;

mod block;
mod mesh;
mod native;
mod rope;
pub(crate) use block::block_mesh;

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

/// The frame's dropped items. `models_revision` must change whenever `models` changes.
#[derive(Clone, Debug, Default, Resource, ExtractResource)]
pub struct DroppedItemScene {
    pub(crate) models_revision: u64,
    pub(crate) models: Arc<[DroppedItemModel]>,
    pub(crate) instances: Arc<[DroppedItemInstance]>,
    pub(crate) terrain_session_id: Option<u64>,
    pub(crate) terrain_instances: Arc<[TerrainItemInstance]>,
    /// World-space geometry drawn as-is this frame (fishing line, leads).
    pub(crate) dynamic: Arc<[ItemMeshVertex]>,
    pub(crate) daylight: f32,
}

impl DroppedItemScene {
    /// Replaces the frame's contents; instances and dynamic vertices beyond their caps are dropped.
    pub fn publish(
        &mut self,
        models_revision: u64,
        models: Arc<[DroppedItemModel]>,
        instances: &[DroppedItemInstance],
        dynamic: &[ItemMeshVertex],
        daylight: f32,
    ) {
        if self.models_revision != models_revision {
            self.models_revision = models_revision;
            self.models = models;
        }
        let count = instances.len().min(MAX_DROPPED_ITEM_INSTANCES);
        self.instances = Arc::from(&instances[..count]);
        // Whole triangles only.
        let dynamic_count = dynamic.len().min(MAX_DYNAMIC_ITEM_VERTICES) / 3 * 3;
        self.dynamic = Arc::from(&dynamic[..dynamic_count]);
        self.daylight = if daylight.is_finite() {
            daylight.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    pub fn clear(&mut self) {
        self.instances = Arc::from([]);
        self.dynamic = Arc::from([]);
        self.terrain_instances = Arc::from([]);
        self.terrain_session_id = None;
    }

    /// Retains bounded candidates separately; transition lists come from bounded world admission.
    pub fn publish_terrain_instances(
        &mut self,
        session_id: u64,
        instances: &[TerrainItemInstance],
    ) {
        self.terrain_session_id = Some(session_id);
        let count = instances.len().min(MAX_DROPPED_ITEM_INSTANCES);
        self.terrain_instances = Arc::from(&instances[..count]);
    }

    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use render_model::OPAQUE_WHITE;

    #[test]
    fn publish_caps_instances_and_keeps_models_until_the_revision_changes() {
        let mut scene = DroppedItemScene::default();
        let model = DroppedItemModel::Sprite(DroppedItemSprite {
            width: 1,
            height: 1,
            rgba8: Arc::from([255_u8; 4]),
        });
        let instance = DroppedItemInstance {
            model: 0,
            world_from_item: dropped_item_transform([0.0; 3], 0.0, 1.0),
            block_level: 0,
            sky_level: 15,
            overlay_rgba8: 0,
        };
        let many = vec![instance; MAX_DROPPED_ITEM_INSTANCES + 5];
        let vertex = ItemMeshVertex {
            position: [0.0; 3],
            uv: [0.0; 2],
            normal: [0.0, 1.0, 0.0],
            layer: WHITE_LAYER,
            color: OPAQUE_WHITE,
        };
        let lines = vec![vertex; MAX_DYNAMIC_ITEM_VERTICES + 2];
        scene.publish(1, Arc::from([model]), &many, &lines, f32::NAN);
        assert_eq!(scene.instance_count(), MAX_DROPPED_ITEM_INSTANCES);
        assert_eq!(scene.dynamic.len() % 3, 0);
        assert!(scene.dynamic.len() <= MAX_DYNAMIC_ITEM_VERTICES);
        assert_eq!(scene.daylight, 1.0);
        scene.publish(1, Arc::from([]), &many[..1], &[], 0.5);
        assert_eq!(scene.models.len(), 1);
        scene.publish(2, Arc::from([]), &[], &[], 0.5);
        assert!(scene.models.is_empty());
    }

    #[test]
    fn transform_rotates_about_up_and_translates() {
        let rows = dropped_item_transform([1.0, 2.0, 3.0], std::f32::consts::FRAC_PI_2, 2.0);
        // Local +X maps to world -Z at a quarter turn.
        assert!(rows[0][0].abs() < 1e-6 && (rows[2][0] + 2.0).abs() < 1e-6);
        assert_eq!([rows[0][3], rows[1][3], rows[2][3]], [1.0, 2.0, 3.0]);
    }
}

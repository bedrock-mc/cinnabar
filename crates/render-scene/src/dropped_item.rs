use bevy::{prelude::Resource, render::extract_resource::ExtractResource};

use render_model::{
    DroppedItemInstance, DroppedItemModel, EquipmentRaster, ItemMeshVertex,
    MAX_DROPPED_ITEM_INSTANCES, MAX_DYNAMIC_ITEM_VERTICES, TerrainItemInstance,
};
use std::sync::Arc;
/// The frame's dropped items. `models_revision` must change whenever `models` changes.
#[derive(Clone, Debug, Default, Resource, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub struct DroppedItemScene {
    pub models_revision: u64,
    pub models: Arc<[DroppedItemModel]>,
    pub instances: Arc<[DroppedItemInstance]>,
    pub terrain_session_id: Option<u64>,
    pub terrain_instances: Arc<[TerrainItemInstance]>,
    /// World-space geometry drawn as-is this frame (fishing line, leads).
    pub dynamic: Arc<[ItemMeshVertex]>,
    pub daylight: f32,
    /// The item glint raster that glinting instances sample.
    pub glint: Option<EquipmentRaster>,
    /// This frame's glint scroll offsets at unit UV scale and strength.
    pub glint_scroll: [f32; 3],
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

    /// Supplies the item glint raster and this frame's scroll; a malformed raster is dropped.
    pub fn set_glint(&mut self, raster: Option<EquipmentRaster>, scroll: [f32; 3]) {
        self.glint = raster.filter(|raster| {
            raster.width != 0
                && raster.height != 0
                && raster.rgba8.len() == usize::from(raster.width) * usize::from(raster.height) * 4
        });
        self.glint_scroll = scroll.map(|value| if value.is_finite() { value } else { 0.0 });
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

#[cfg(test)]
mod tests;

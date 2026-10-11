use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use render_model::primitive_shapes::PrimitiveShapeStore;
use std::sync::{Arc, Mutex};
/// Shared retained state; extraction copies only the handle and the current clock/dimension.
#[derive(Resource, Clone, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub struct PrimitiveShapesScene {
    pub store: Arc<Mutex<PrimitiveShapeStore>>,
    pub clock: f32,
    pub dimension: i32,
    pub render_distance: f32,
}

impl Default for PrimitiveShapesScene {
    /// Starts with no geometry or GPU publications.
    fn default() -> Self {
        Self {
            store: Arc::new(Mutex::new(PrimitiveShapeStore::default())),
            clock: 0.0,
            dimension: 0,
            render_distance: 0.0,
        }
    }
}

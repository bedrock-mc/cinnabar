use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use mod_api::MAX_BLOCK_HIGHLIGHTS;
use mod_render::{RenderOutput, block_highlights, geometry::ModVertex, position_box};
use std::sync::Arc;
/// The current mod's render output, extracted whenever the mod commits a change.
#[derive(Resource, Clone, Debug, Default)]
pub struct ModRenderScene {
    generation: u64,
    pub passes: Vec<mod_render::Pass>,
    pub vertices: Arc<[ModVertex]>,
    primitives: Arc<mod_render::Primitives>,
    pub marker_vertices: Arc<[ModVertex]>,
    position_box: Option<[[f32; 3]; 2]>,
    pub block_vertices: Arc<[ModVertex]>,
    block_positions: Arc<[[i32; 3]]>,
    block_color: [f32; 4],
}

impl ExtractResource<bevy::render::RenderApp> for ModRenderScene {
    type Source = Self;

    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

impl ModRenderScene {
    /// Adopts `output` unless `generation` is already applied.
    pub fn apply(&mut self, output: &RenderOutput, generation: u64) {
        if generation == self.generation {
            return;
        }
        self.generation = generation;
        self.passes.clone_from(&output.passes);
        if !Arc::ptr_eq(&self.primitives, &output.primitives) {
            self.primitives = Arc::clone(&output.primitives);
            self.vertices = mod_render::geometry::build(&output.primitives).into();
        }
    }

    /// Publishes a host-owned own-position box independently of guest render output.
    pub fn set_position_box(&mut self, bounds: Option<[[f32; 3]; 2]>) {
        let bounds = bounds.filter(position_box::valid);
        if self.position_box == bounds {
            return;
        }
        self.position_box = bounds;
        self.marker_vertices = match bounds {
            Some(bounds) => {
                let mut vertices = Vec::with_capacity(position_box::VERTICES);
                position_box::append(&mut vertices, bounds);
                vertices.into()
            }
            None => Arc::from([]),
        };
    }

    /// Highlights loaded unit blocks through world geometry, independently of guest primitives.
    pub fn set_block_highlights(&mut self, positions: &[[i32; 3]], color: [f32; 4]) {
        let positions = &positions[..positions.len().min(MAX_BLOCK_HIGHLIGHTS)];
        let valid = color
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v));
        let positions = if valid { positions } else { &[] };
        if self.block_positions.as_ref() == positions && self.block_color == color {
            return;
        }
        self.block_positions = Arc::from(positions);
        self.block_color = color;
        self.block_vertices = block_highlights::build(positions, color).into();
    }

    /// Drops every pass and primitive, as when a mod traps, reloads or is revoked.
    pub fn clear(&mut self) {
        if self.generation != 0 || !self.passes.is_empty() || self.vertex_count() != 0 {
            *self = Self::default();
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len() + self.marker_vertices.len() + self.block_vertices.len()
    }
}

#[cfg(test)]
mod block_highlights_tests;
#[cfg(test)]
mod position_box_tests;

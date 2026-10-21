use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use mod_api::MAX_BLOCK_HIGHLIGHTS;
use mod_render::{RenderOutput, block_highlights, geometry::ModVertex, position_box};
use std::sync::Arc;

/// Publishes incarnation keys and the admitted body/equipment cost without model or pose data.
pub fn mask_targets(frame: &crate::ActorRenderFrame) -> Vec<(mod_api::drawing::MaskEntity, u64)> {
    let mut targets = std::collections::BTreeMap::<_, u64>::new();
    for (entry, instance) in frame.rig.manifest.iter().zip(frame.rig.instances.iter()) {
        let key = mod_api::drawing::MaskEntity {
            connection: entry.identity.session_id,
            dimension: entry.identity.dimension,
            runtime_id: entry.identity.runtime_id,
            spawn_revision: entry.identity.spawn_revision,
        };
        let vertices = frame
            .rig
            .geometry_spans
            .get(instance.geometry_id as usize)
            .map_or(0, |span| u64::from(span.vertex_count));
        let total = targets.entry(key).or_default();
        *total = total.saturating_add(vertices);
    }
    targets.into_iter().collect()
}
/// The current mod's render output, extracted whenever the mod commits a change.
#[derive(Resource, Clone, Debug, Default)]
pub struct ModRenderScene {
    generation: u64,
    pub passes: Vec<mod_render::Pass>,
    pub retained: Vec<Arc<mod_render::retained::Resources>>,
    pub masks: Arc<[mod_render::masks::Mask]>,
    pub vertices: Arc<[ModVertex]>,
    primitives: Arc<mod_render::Primitives>,
    pub marker_vertices: Arc<[ModVertex]>,
    position_box: Option<[[f32; 3]; 2]>,
    pub block_vertices: Arc<[ModVertex]>,
    block_positions: Arc<[[i32; 3]]>,
    block_color: [f32; 4],
    drawn_blocks: Arc<[mod_api::native_render::BlockHighlight]>,
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
        self.retained.clone_from(&output.retained);
        self.masks = Arc::clone(&output.masks);
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
        self.rebuild_block_highlights();
    }

    /// Adopts validated drawing highlights alongside host-discovered loaded-block selections.
    pub fn set_drawn_block_highlights(
        &mut self,
        blocks: &[mod_api::native_render::BlockHighlight],
    ) {
        let blocks = &blocks[..blocks.len().min(MAX_BLOCK_HIGHLIGHTS)];
        if self.drawn_blocks.as_ref() == blocks {
            return;
        }
        self.drawn_blocks = Arc::from(blocks);
        self.rebuild_block_highlights();
    }

    /// Both producers share geometry, depth and the existing combined vertex budget.
    fn rebuild_block_highlights(&mut self) {
        self.block_vertices = block_highlights::build_coloured(
            self.block_positions
                .iter()
                .map(|position| (*position, self.block_color))
                .chain(
                    self.drawn_blocks
                        .iter()
                        .map(|block| (block.position, block.colour)),
                )
                .take(MAX_BLOCK_HIGHLIGHTS),
        )
        .into();
    }

    /// Drops every pass and primitive, as when a mod traps, reloads or is revoked.
    pub fn clear(&mut self) {
        if self.generation != 0
            || !self.passes.is_empty()
            || !self.masks.is_empty()
            || self.vertex_count() != 0
        {
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

/// Same-frame placement and GPU inputs for retained world drawing.
pub mod retained;

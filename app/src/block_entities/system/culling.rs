//! Skips geometry preparation outside the view or in sections the cave graph hides.

use bevy::math::Mat4;
use render_model::BLOCK_ENTITY_REACH_BLOCKS;
use world::SubChunkKey;

use super::super::describe::Template;
use crate::runtime::visibility::CaveVisibilityCache;

/// One frame's block-entity preparation work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct BlockEntityWork {
    /// Described entities within the scan radius.
    pub(super) candidates: u32,
    /// Candidates prepared for drawing; the rest only advanced their state and canvases.
    pub(super) prepared: u32,
}

impl BlockEntityWork {
    /// Counts a candidate and returns whether it must be prepared this frame.
    pub(super) fn admit(
        &mut self,
        culling: Option<&CaveCulling<'_, impl Fn(SubChunkKey) -> bool>>,
        clip_from_world: Option<Mat4>,
        template: &Template,
        block: [i32; 3],
    ) -> bool {
        let prepare = bounds(template, block).is_none_or(|(low, high)| {
            clip_from_world.is_none_or(|matrix| {
                render_model::visibility_bounds::box_intersects_view(matrix, low, high)
            }) && !culling.is_some_and(|culling| culling.hides(low, high))
        });
        self.candidates += 1;
        self.prepared += u32::from(prepare);
        prepare
    }
}

/// Conservative bounds for locally drawing templates; global beams cannot use terrain culling.
pub(super) fn bounds(template: &Template, block: [i32; 3]) -> Option<([f32; 3], [f32; 3])> {
    let bounded = match template {
        Template::Beacon | Template::FlowerPot { .. } => false,
        Template::Static(kind) => kind.has_fixed_cull_reach(),
        Template::ItemFrame { item, .. } => item.is_none(),
        Template::Campfire { items, .. } => items.iter().all(Option::is_none),
        Template::Bell { .. }
        | Template::Conduit { .. }
        | Template::Chest(_)
        | Template::Shulker(_)
        | Template::Sign { .. }
        | Template::EnchantTable => true,
    };
    bounded.then(|| {
        (
            block.map(|axis| axis.saturating_sub(BLOCK_ENTITY_REACH_BLOCKS) as f32),
            block.map(|axis| axis.saturating_add(BLOCK_ENTITY_REACH_BLOCKS + 1) as f32),
        )
    })
}

/// The actor rule for block entities: hide only with a result for the current camera section
/// and connectivity generation, when every section within reach is known and unreached.
pub(super) struct CaveCulling<'a, K> {
    cave: &'a CaveVisibilityCache,
    camera: SubChunkKey,
    generation: u64,
    known: K,
}

impl<'a, K: Fn(SubChunkKey) -> bool> CaveCulling<'a, K> {
    pub(super) fn new(
        cave: &'a CaveVisibilityCache,
        camera: SubChunkKey,
        generation: u64,
        known: K,
    ) -> Self {
        Self {
            cave,
            camera,
            generation,
            known,
        }
    }

    /// Whether every section touched by conservative bounds is known and unreached.
    fn hides(&self, low: [f32; 3], high: [f32; 3]) -> bool {
        self.cave
            .hides_box(self.camera, self.generation, &self.known, low, high)
    }
}

#[cfg(test)]
#[path = "culling_tests.rs"]
mod tests;

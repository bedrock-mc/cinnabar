//! Skips geometry preparation for block entities the current cave graph hides.

use render_model::BLOCK_ENTITY_REACH_BLOCKS;
use world::{SUB_CHUNK_SIDE, SubChunkKey};

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
        template: &Template,
        block: [i32; 3],
    ) -> bool {
        let prepare = !culling.is_some_and(|culling| culling.hides(template, block));
        self.candidates += 1;
        self.prepared += u32::from(prepare);
        prepare
    }
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

    /// Whether `template` at `block` draws nothing visible; beacon beams never hide.
    fn hides(&self, template: &Template, block: [i32; 3]) -> bool {
        let global = match template {
            Template::Beacon => true,
            Template::Static(kind) => kind.renders_globally(),
            _ => false,
        };
        let section = |offset: i32| {
            block.map(|axis| {
                axis.saturating_add(offset)
                    .div_euclid(SUB_CHUNK_SIDE as i32)
            })
        };
        !global
            && self.cave.hides_sections(
                self.camera,
                self.generation,
                &self.known,
                section(-BLOCK_ENTITY_REACH_BLOCKS),
                section(BLOCK_ENTITY_REACH_BLOCKS),
            )
    }
}

#[cfg(test)]
#[path = "culling_tests.rs"]
mod tests;

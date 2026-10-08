//! Static mesh fragments, replayed wherever emitting them afresh would give the same vertices.

use bevy::platform::collections::HashMap;

use super::{BlockEntitySubmission, BlockEntityVertex, MeshBuilder};
use crate::block_entity::mesh::MAX_BLOCK_ENTITY_VERTICES;

/// Last frame's fragments: an unchanged scan finds each next in order without hashing, and a
/// reordered one (entities entering or leaving range) falls back to a lookup by block.
pub(super) struct PreviousFragments {
    list: Vec<Option<Box<CachedSubmission>>>,
    cursor: usize,
    by_block: Option<HashMap<[i32; 3], usize>>,
}

impl PreviousFragments {
    pub(super) fn new(list: Vec<Option<Box<CachedSubmission>>>) -> Self {
        Self {
            list,
            cursor: 0,
            by_block: None,
        }
    }

    /// Removes the fragment last built for `submission`'s block, if any.
    pub(super) fn take(
        &mut self,
        submission: &BlockEntitySubmission,
    ) -> Option<Box<CachedSubmission>> {
        if let Some(found) = self
            .list
            .get_mut(self.cursor)
            .and_then(|slot| slot.take_if(|fragment| fragment.submission == *submission))
        {
            self.cursor += 1;
            return Some(found);
        }
        let list = &self.list;
        let index = *self
            .by_block
            .get_or_insert_with(|| {
                list.iter()
                    .enumerate()
                    .filter_map(|(index, fragment)| {
                        fragment
                            .as_ref()
                            .map(|fragment| (fragment.submission.block, index))
                    })
                    .collect()
            })
            .get(&submission.block)?;
        let found = self.list[index].take()?;
        self.cursor = index + 1;
        Some(found)
    }
}

#[derive(Debug)]
pub(super) struct CachedSubmission {
    submission: BlockEntitySubmission,
    start: [usize; 5],
    solid: Vec<BlockEntityVertex>,
    overlay: Vec<BlockEntityVertex>,
    crack: Vec<BlockEntityVertex>,
    portal: Vec<BlockEntityVertex>,
    additive: Vec<BlockEntityVertex>,
    rejected_quads: u64,
}

/// Counts the vertices already accepted in each draw layer before a submission.
pub(super) fn vertex_counts(builder: &MeshBuilder) -> [usize; 5] {
    [
        builder.solid.len(),
        builder.overlay.len(),
        builder.crack.len(),
        builder.portal.len(),
        builder.additive.len(),
    ]
}

impl CachedSubmission {
    /// Copies only the vertices and rejected quads produced by this submission.
    pub(super) fn capture(
        submission: &BlockEntitySubmission,
        start: [usize; 5],
        rejected_before: u64,
        builder: &MeshBuilder,
    ) -> Self {
        Self {
            submission: submission.clone(),
            start,
            solid: builder.solid[start[0]..].to_vec(),
            overlay: builder.overlay[start[1]..].to_vec(),
            crack: builder.crack[start[2]..].to_vec(),
            portal: builder.portal[start[3]..].to_vec(),
            additive: builder.additive[start[4]..].to_vec(),
            rejected_quads: builder.rejected_quads.saturating_sub(rejected_before),
        }
    }

    /// Reuses a fragment for the same submission at its original layer offsets, or anywhere a
    /// fragment that lost no quads still fits every layer, since then no budget check differs.
    pub(super) fn matches(
        &self,
        submission: &BlockEntitySubmission,
        builder: &MeshBuilder,
    ) -> bool {
        if &self.submission != submission {
            return false;
        }
        let counts = vertex_counts(builder);
        counts == self.start
            || (self.rejected_quads == 0
                && counts
                    .iter()
                    .zip(self.lengths())
                    .all(|(count, length)| count + length <= MAX_BLOCK_ENTITY_VERTICES))
    }

    fn lengths(&self) -> [usize; 5] {
        [
            self.solid.len(),
            self.overlay.len(),
            self.crack.len(),
            self.portal.len(),
            self.additive.len(),
        ]
    }

    /// Appends the previously accepted geometry at its original position in every draw layer.
    pub(super) fn append_to(&self, builder: &mut MeshBuilder) {
        builder.solid.extend_from_slice(&self.solid);
        builder.overlay.extend_from_slice(&self.overlay);
        builder.crack.extend_from_slice(&self.crack);
        builder.portal.extend_from_slice(&self.portal);
        builder.additive.extend_from_slice(&self.additive);
        builder.rejected_quads = builder.rejected_quads.saturating_add(self.rejected_quads);
    }
}

use super::ChunkStore;
use crate::ShapeActor;
use crate::{BlockEntityKey, BlockEntityNbt, ChunkKey, CollisionRevisionError, SubChunkKey};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

impl ChunkStore {
    /// Reserves a revision when replacing a scope changes geometry-bearing actor facts.
    pub(super) fn reserve_actor_replacement<'a>(
        &self,
        chunk: ChunkKey,
        scope: Option<SubChunkKey>,
        replacement: impl Iterator<Item = (BlockEntityKey, &'a BlockEntityNbt)>,
    ) -> Result<Option<u64>, CollisionRevisionError> {
        let replacement: BTreeMap<_, _> = replacement
            .filter_map(|(key, nbt)| {
                (nbt.shape_actor() != &ShapeActor::None).then_some((key, nbt.shape_actor()))
            })
            .collect();
        let previous: BTreeMap<_, _> = self
            .chunks
            .get(&chunk)
            .into_iter()
            .flat_map(|chunk| chunk.block_entities.iter())
            .filter(|(key, _)| scope.is_none_or(|scope| key.sub_chunk() == scope))
            .filter_map(|(&key, nbt)| {
                (nbt.shape_actor() != &ShapeActor::None).then_some((key, nbt.shape_actor()))
            })
            .collect();
        self.reserve_actor_change(chunk, previous != replacement)
    }

    /// Allocates before mutation so exhausted revision space cannot publish stale geometry.
    pub(super) fn reserve_actor_change(
        &self,
        chunk: ChunkKey,
        changed: bool,
    ) -> Result<Option<u64>, CollisionRevisionError> {
        (changed && self.collision_revisions.contains_key(&chunk))
            .then(|| self.collision_revision_allocator.allocate())
            .transpose()
    }
}

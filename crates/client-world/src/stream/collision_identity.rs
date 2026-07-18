use world::{ChunkCollisionRevision, CollisionRevisionError};

use super::*;

impl WorldStream {
    pub(super) fn mark_collision_chunk_loaded(
        &mut self,
        key: ChunkKey,
    ) -> Result<bool, CollisionRevisionError> {
        let changed = self.store.mark_chunk_loaded(key)?;
        if changed {
            self.bump_collision_world_generation();
        }
        Ok(changed)
    }

    pub(super) fn observe_collision_revision_change(
        &mut self,
        key: ChunkKey,
        previous: Option<ChunkCollisionRevision>,
    ) {
        if self.store.collision_revision(key) != previous {
            self.bump_collision_world_generation();
        }
    }

    pub(super) fn bump_collision_world_generation(&mut self) {
        if self.collision_world_generation_exhausted {
            return;
        }
        match self.collision_world_generation.checked_add(1) {
            Some(generation) => self.collision_world_generation = generation,
            None => self.collision_world_generation_exhausted = true,
        }
    }
}

//! Columns whose published geometry or delivered emptiness hides cached far terrain.

use super::*;

#[derive(Default)]
pub(super) struct NearDrawn {
    columns: HashSet<ChunkKey>,
    revision: u64,
}

impl WorldStream {
    /// Columns covered by near geometry or authoritative air, and their membership revision.
    pub fn near_drawn_columns(&self) -> (u64, impl Iterator<Item = ChunkKey> + '_) {
        (
            self.near_drawn.revision,
            self.near_drawn.columns.iter().copied(),
        )
    }

    /// Covers delivered air immediately, including outside the full-detail radius. Other
    /// columns wait for every mesh to publish and retain coverage while those meshes change.
    pub(super) fn note_near_drawn(&mut self, column: ChunkKey) {
        // Only far terrain reads the set; with it off nothing is tracked.
        if !self.far_terrain_enabled() || !self.loaded_columns.contains(&column) {
            return;
        }
        if self.wants_mesh(column) && self.near_drawn.columns.contains(&column) {
            return;
        }
        let empty = self.authority.terrain().chunk(column).is_none_or(|chunk| {
            chunk
                .sub_chunks()
                .all(|(_, section)| self.classifier.is_sub_chunk_air(&section))
        });
        if !empty && !self.wants_mesh(column) {
            self.forget_near_drawn([&column]);
            return;
        }
        if self.near_drawn.columns.contains(&column) {
            return;
        }
        let mut keys = self
            .resident
            .column(column)
            .chain(self.known_air.column(column))
            .peekable();
        let uploaded = empty
            || (keys.peek().is_some()
                && keys.all(|key| {
                    self.applied_mesh_generations.contains_key(key)
                        && !self.withdrawn_meshes.contains(key)
                        && self.revisions.dirty(*key).is_none()
                }));
        if uploaded {
            self.near_drawn.columns.insert(column);
            self.near_drawn.revision += 1;
        }
    }

    pub(super) fn near_drawn_contains(&self, column: ChunkKey) -> bool {
        self.near_drawn.columns.contains(&column)
    }

    /// Forgets every drawn column, as when far terrain turns off.
    pub(super) fn clear_near_drawn(&mut self) {
        if !self.near_drawn.columns.is_empty() {
            self.near_drawn.columns = HashSet::new();
            self.near_drawn.revision += 1;
        }
    }

    /// Hands `columns` back to far terrain, as when they leave residency.
    pub(super) fn forget_near_drawn<'a>(
        &mut self,
        columns: impl IntoIterator<Item = &'a ChunkKey>,
    ) {
        let mut changed = false;
        for column in columns {
            changed |= self.near_drawn.columns.remove(column);
        }
        if changed {
            self.near_drawn.revision += 1;
        }
    }
}

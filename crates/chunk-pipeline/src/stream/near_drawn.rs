//! Columns near terrain has on screen. Far terrain hides under exactly these, so every
//! column is drawn by one of the two and never by both or neither.

use super::*;

#[derive(Default)]
pub(super) struct NearDrawn {
    columns: HashSet<ChunkKey>,
    /// Columns whose outgoing geometry still awaits its final removal acknowledgement.
    withdrawing: HashSet<ChunkKey>,
    revision: u64,
}

impl WorldStream {
    /// Columns near terrain draws, and a revision that changes whenever the set does.
    pub fn near_drawn_columns(&self) -> (u64, impl Iterator<Item = ChunkKey> + '_) {
        (
            self.near_drawn.revision,
            self.near_drawn.columns.iter().copied(),
        )
    }

    /// Columns that keep far terrain masked, including outgoing meshes awaiting publication.
    pub fn near_mask_columns(&self) -> (u64, impl Iterator<Item = ChunkKey> + '_) {
        (
            self.near_drawn.revision,
            self.near_drawn
                .columns
                .iter()
                .chain(&self.near_drawn.withdrawing)
                .copied(),
        )
    }

    /// Adds `column` once it is loaded, holds sections, and every one of them has a published
    /// mesh with no newer one pending. A column without sections stays with far terrain, whose
    /// summary of it is empty. A drawn column keeps its meshes on screen while they are redone.
    pub(super) fn note_near_drawn(&mut self, column: ChunkKey) {
        // Only far terrain reads the set; with it off nothing is tracked.
        if !self.far_terrain_enabled()
            || self.near_drawn.columns.contains(&column)
            || !self.loaded_columns.contains(&column)
            || !self.wants_mesh(column)
        {
            return;
        }
        let mut keys = self
            .resident
            .column(column)
            .chain(self.known_air.column(column))
            .peekable();
        let uploaded = keys.peek().is_some()
            && keys.all(|key| {
                self.applied_mesh_generations.contains_key(key)
                    && !self.withdrawn_meshes.contains(key)
                    && self.revisions.dirty(*key).is_none()
            });
        if uploaded {
            self.near_drawn.withdrawing.remove(&column);
            self.near_drawn.columns.insert(column);
            self.near_drawn.revision += 1;
        }
    }

    pub(super) fn near_drawn_contains(&self, column: ChunkKey) -> bool {
        self.near_drawn.columns.contains(&column)
    }

    /// Forgets every drawn column, as when far terrain turns off.
    pub(super) fn clear_near_drawn(&mut self) {
        if !self.near_drawn.columns.is_empty() || !self.near_drawn.withdrawing.is_empty() {
            self.near_drawn.columns = HashSet::new();
            self.near_drawn.withdrawing = HashSet::new();
            self.near_drawn.revision += 1;
        }
    }

    /// Ends full-detail membership while retaining the mask over geometry still on screen.
    pub(super) fn withdraw_near_drawn(&mut self, column: ChunkKey) {
        if self.near_drawn.columns.remove(&column) {
            self.near_drawn.withdrawing.insert(column);
            self.near_drawn.revision += 1;
        }
    }

    /// Releases an outgoing mask only after all published sections acknowledge removal.
    pub(super) fn note_near_withdrawal(&mut self, column: ChunkKey) {
        if !self.near_drawn.withdrawing.contains(&column) || self.wants_mesh(column) {
            return;
        }
        let still_published = self
            .resident
            .column(column)
            .chain(self.known_air.column(column))
            .any(|key| {
                self.applied_mesh_generations.contains_key(key)
                    && !self.withdrawn_meshes.contains(key)
            });
        if !still_published {
            self.near_drawn.withdrawing.remove(&column);
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
            changed |= self.near_drawn.withdrawing.remove(column);
        }
        if changed {
            self.near_drawn.revision += 1;
        }
    }
}

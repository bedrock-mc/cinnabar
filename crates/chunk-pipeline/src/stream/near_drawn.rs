//! Columns near terrain has on screen. Far terrain hides under exactly these, so every
//! column is drawn by one of the two and never by both or neither.

use super::*;

#[derive(Default)]
pub(super) struct NearDrawn {
    columns: HashSet<ChunkKey>,
    revision: u64,
}

impl WorldStream {
    /// Columns near terrain draws, and a revision that changes whenever the set does.
    #[must_use]
    pub fn near_drawn_columns(&self) -> (u64, impl Iterator<Item = ChunkKey> + '_) {
        (
            self.near_drawn.revision,
            self.near_drawn.columns.iter().copied(),
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
                    && self.revisions.dirty(*key).is_none()
            });
        if uploaded {
            self.near_drawn.columns.insert(column);
            self.near_drawn.revision += 1;
        }
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

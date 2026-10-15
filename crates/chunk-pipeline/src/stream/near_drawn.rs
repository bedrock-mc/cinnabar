//! Main-world completion of near publications. The render owner chooses visible coverage.

use super::*;

#[derive(Default)]
pub(super) struct NearDrawn {
    columns: HashSet<ChunkKey>,
    revision: u64,
}

impl WorldStream {
    /// Changes when section requirements or near membership can change the render target.
    pub fn terrain_publication_revision(&self) -> (u64, u64, u64, u64, usize, Option<i32>) {
        (
            self.revisions.next_revision,
            self.near_drawn.revision,
            self.resident.column_generation(),
            self.known_air.column_generation(),
            self.loaded_columns.len(),
            self.full_detail.radius,
        )
    }

    /// Describes loaded near columns from their owning source and dirty-revision indexes.
    pub fn terrain_publication_target(&self, horizon: i32) -> far_terrain::TerrainTarget {
        let near = self
            .loaded_columns
            .iter()
            .copied()
            .filter(|column| {
                column.dimension == self.current_dimension() && self.wants_mesh(*column)
            })
            .filter_map(|column| {
                let keys = self
                    .resident
                    .column(column)
                    .chain(self.known_air.column(column))
                    .map(|&key| {
                        let generation = self
                            .revisions
                            .dirty(key)
                            .map(|dirty| dirty.revision)
                            .or_else(|| self.applied_mesh_generations.get(&key).copied())
                            .unwrap_or(0);
                        (key, generation)
                    })
                    .collect::<std::collections::HashMap<_, _>>();
                (!keys.is_empty()).then_some((column, keys))
            })
            .collect();
        far_terrain::TerrainTarget {
            centre: self.far_terrain_centre(),
            horizon,
            near,
            retained: self
                .resident
                .iter()
                .chain(self.known_air.iter())
                .copied()
                .collect(),
            empty_far: Default::default(),
        }
    }

    /// Columns whose near meshes have all been acknowledged, with their membership revision.
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

//! Columns whose published geometry or delivered emptiness hides cached far terrain.

use super::*;

#[derive(Default)]
pub(super) struct NearDrawn {
    columns: HashMap<ChunkKey, Coverage>,
    revision: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Coverage {
    Geometry,
    Empty,
}

impl WorldStream {
    /// Covered columns, and a revision that changes with membership or coverage kind.
    pub fn near_drawn_columns(&self) -> (u64, impl Iterator<Item = ChunkKey> + '_) {
        (
            self.near_drawn.revision,
            self.near_drawn.columns.keys().copied(),
        )
    }

    /// Columns with published near geometry, which determine the far ring's finest LOD bound.
    pub fn near_geometry_columns(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        self.near_drawn
            .columns
            .iter()
            .filter_map(|(column, coverage)| (*coverage == Coverage::Geometry).then_some(*column))
    }

    /// Delivered air that hides cached terrain without extending the near geometry bound.
    pub fn near_empty_columns(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        self.near_drawn
            .columns
            .iter()
            .filter_map(|(column, coverage)| (*coverage == Coverage::Empty).then_some(*column))
    }

    /// Covers delivered air immediately, including outside the full-detail radius. Other
    /// columns wait for every mesh to publish and retain coverage while those meshes change.
    pub(super) fn note_near_drawn(&mut self, column: ChunkKey) {
        // Only far terrain reads coverage; with it off nothing is tracked.
        if !self.far_terrain_enabled() || !self.loaded_columns.contains(&column) {
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
        if !empty && self.near_drawn.columns.get(&column) == Some(&Coverage::Geometry) {
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
            let coverage = if empty {
                Coverage::Empty
            } else {
                Coverage::Geometry
            };
            if self.near_drawn.columns.insert(column, coverage) != Some(coverage) {
                self.near_drawn.revision += 1;
            }
        }
    }

    /// Whether near geometry or delivered air currently hides this column's far terrain.
    pub(super) fn near_drawn_contains(&self, column: ChunkKey) -> bool {
        self.near_drawn.columns.contains_key(&column)
    }

    /// Forgets every drawn column, as when far terrain turns off.
    pub(super) fn clear_near_drawn(&mut self) {
        if !self.near_drawn.columns.is_empty() {
            self.near_drawn.columns = HashMap::new();
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
            changed |= self.near_drawn.columns.remove(column).is_some();
        }
        if changed {
            self.near_drawn.revision += 1;
        }
    }
}

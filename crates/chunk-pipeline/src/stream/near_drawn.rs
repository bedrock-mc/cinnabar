//! Main-world completion of near publications. The render owner chooses visible coverage.

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

/// Where [`WorldStream::terrain_publication_changes`] resumes replaying section membership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainPublicationCursor([u64; 2]);

impl WorldStream {
    /// Identifies the stream and dimension visit independently of rendering feature toggles.
    pub fn terrain_publication_source(&self) -> ((u64, u64), u64, u64) {
        (
            (
                self.authority.actor_session_id(),
                self.form_dimension_epoch(),
            ),
            self.resident.membership_revision(),
            self.known_air.membership_revision(),
        )
    }

    /// Sections whose latest publication receipts may still be used by this stream.
    pub fn terrain_publication_sections(&self) -> impl Iterator<Item = SubChunkKey> + '_ {
        self.resident.iter().chain(self.known_air.iter()).copied()
    }

    /// The current end of the membership history [`Self::terrain_publication_changes`] replays.
    pub fn terrain_publication_cursor(&self) -> TerrainPublicationCursor {
        TerrainPublicationCursor([
            self.resident.journal_position(),
            self.known_air.journal_position(),
        ])
    }

    /// Each section whose membership in [`Self::terrain_publication_sections`] may have changed
    /// since `cursor`, possibly repeated, with whether it belongs now. Applying them to the
    /// sections as of `cursor` yields the current sections. `None` when the bounded history no
    /// longer reaches back to `cursor`, so the caller reconciles against every section instead.
    pub fn terrain_publication_changes(
        &self,
        cursor: TerrainPublicationCursor,
    ) -> Option<impl Iterator<Item = (SubChunkKey, bool)> + '_> {
        let [resident, known_air] = cursor.0;
        let resident = self.resident.changed_since(resident)?;
        let known_air = self.known_air.changed_since(known_air)?;
        Some(resident.chain(known_air).map(|&key| {
            (
                key,
                self.resident.contains(&key) || self.known_air.contains(&key),
            )
        }))
    }

    /// Changes when section requirements or near membership can change the render target.
    pub fn terrain_publication_revision(&self) -> (u64, u64, u64, u64, usize, Option<i32>) {
        (
            self.revisions.next_revision,
            self.near_drawn.revision,
            self.resident.membership_revision(),
            self.known_air.membership_revision(),
            self.loaded_columns.len(),
            self.full_detail_radius(),
        )
    }

    /// Describes near geometry and delivered air from source and dirty-revision indexes.
    pub fn terrain_publication_target(&self, horizon: i32) -> far_terrain::TerrainTarget {
        let near = self
            .loaded_columns
            .iter()
            .copied()
            .filter(|column| {
                column.dimension == self.current_dimension()
                    && (self.wants_mesh(*column) || self.column_is_delivered_air(*column))
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
            .collect::<std::collections::HashMap<_, _>>();
        far_terrain::TerrainTarget {
            centre: self.far_terrain_centre(),
            horizon,
            near: near.into(),
            retained: std::sync::Arc::new(self.terrain_publication_sections().collect()),
            empty_far: Default::default(),
        }
    }

    /// Columns whose near meshes have all been acknowledged, with their membership revision.
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
        let empty = self.column_is_delivered_air(column);
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

    /// A completely delivered column proves emptiness from its source, independent of meshing.
    fn column_is_delivered_air(&self, column: ChunkKey) -> bool {
        self.loaded_columns.contains(&column)
            && self.authority.terrain().chunk(column).is_none_or(|chunk| {
                chunk
                    .sub_chunks()
                    .all(|(_, section)| self.classifier.is_sub_chunk_air(&section))
            })
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

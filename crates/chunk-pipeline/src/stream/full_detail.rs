//! Full-detail radius: only columns near the player are lit, meshed and uploaded; the rest
//! keep their block data and are drawn by far terrain from their summaries.

use super::*;

/// Columns stay full-detail this many chunks past the radius before they are demoted, so
/// walking along the boundary does not remesh the same columns back and forth.
pub const FULL_DETAIL_HYSTERESIS_CHUNKS: i32 = 2;

#[derive(Default)]
pub(super) struct FullDetail {
    /// `None` meshes every column, as without far terrain.
    radius: Option<i32>,
    centre: Option<ChunkKey>,
    /// Columns within the radius, plus those not yet past the hysteresis band.
    columns: HashSet<ChunkKey>,
    /// Full-detail columns whose every section mesh is uploaded, which far terrain hides under.
    drawn: HashSet<ChunkKey>,
    drawn_revision: u64,
}

impl FullDetail {
    /// True when `column` is meshed and uploaded; every column is without a radius.
    pub(super) fn wants_mesh(&self, column: ChunkKey) -> bool {
        self.radius.is_none() || self.columns.contains(&column)
    }

    fn within(&self, column: ChunkKey, extra: i32) -> bool {
        let (Some(radius), Some(centre)) = (self.radius, self.centre) else {
            return false;
        };
        let (dx, dz) = (
            i64::from(column.x) - i64::from(centre.x),
            i64::from(column.z) - i64::from(centre.z),
        );
        let reach = i64::from(radius + extra);
        column.dimension == centre.dimension && dx * dx + dz * dz <= reach * reach
    }
}

impl WorldStream {
    /// Limits lighting, meshing and uploads to columns within `radius` chunks of the player;
    /// `None` restores full detail everywhere.
    pub fn set_full_detail_radius(&mut self, radius: Option<i32>) {
        let radius = radius.map(|radius| radius.max(1));
        if self.full_detail.radius == radius {
            return;
        }
        let was_limited = self.full_detail.radius.is_some();
        self.full_detail.radius = radius;
        self.full_detail.centre = None;
        if radius.is_none() && was_limited {
            // Everything is full detail again: relight and remesh what was left out.
            let demoted: Vec<ChunkKey> = self
                .loaded_columns
                .iter()
                .copied()
                .filter(|column| !self.full_detail.columns.contains(column))
                .collect();
            self.full_detail.columns.clear();
            self.full_detail.drawn.clear();
            self.full_detail.drawn_revision += 1;
            for column in demoted {
                self.refresh_full_detail_column(column);
            }
            return;
        }
        self.reevaluate_full_detail();
    }

    /// The full-detail radius in effect, if any.
    #[must_use]
    pub const fn full_detail_radius(&self) -> Option<i32> {
        self.full_detail.radius
    }

    /// Columns drawn in full detail, and a revision that changes whenever the set does.
    #[must_use]
    pub fn drawn_full_detail_columns(&self) -> (u64, impl Iterator<Item = ChunkKey> + '_) {
        (
            self.full_detail.drawn_revision,
            self.full_detail.drawn.iter().copied(),
        )
    }

    /// True when `column` is meshed and uploaded; every column is without a radius.
    pub(super) fn wants_mesh(&self, column: ChunkKey) -> bool {
        self.full_detail.wants_mesh(column)
    }

    /// Light is solved one column past full detail, so edge meshes sample current light.
    pub(super) fn wants_light(&self, column: ChunkKey) -> bool {
        self.full_detail.radius.is_none()
            || (-1..=1).any(|dx| {
                (-1..=1).any(|dz| {
                    self.full_detail.columns.contains(&ChunkKey::new(
                        column.dimension,
                        column.x.saturating_add(dx),
                        column.z.saturating_add(dz),
                    ))
                })
            })
    }

    /// Promotes and demotes columns when the player's column or the radius changes.
    pub(super) fn reevaluate_full_detail(&mut self) {
        let Some(radius) = self.full_detail.radius else {
            return;
        };
        let centre = self.player_chunk();
        if self.full_detail.centre == Some(centre) {
            return;
        }
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("stream.full_detail").entered();
        self.full_detail.centre = Some(centre);
        let demoted: Vec<ChunkKey> = self
            .full_detail
            .columns
            .iter()
            .copied()
            .filter(|column| {
                !self
                    .full_detail
                    .within(*column, FULL_DETAIL_HYSTERESIS_CHUNKS)
            })
            .collect();
        for column in &demoted {
            self.full_detail.columns.remove(column);
        }
        let mut promoted = Vec::new();
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let column = ChunkKey::new(centre.dimension, centre.x + dx, centre.z + dz);
                if self.full_detail.within(column, 0) && !self.full_detail.columns.contains(&column)
                {
                    promoted.push(column);
                }
            }
        }
        let unlit_before: HashSet<ChunkKey> = promoted
            .iter()
            .flat_map(|column| neighbourhood(*column))
            .filter(|column| !self.wants_light(*column))
            .collect();
        self.full_detail.columns.extend(promoted.iter().copied());
        for column in demoted {
            self.demote_full_detail_column(column);
        }
        for column in promoted {
            self.refresh_full_detail_column(column);
        }
        // Halo columns now beside full detail need light for their neighbours' meshes.
        for column in unlit_before {
            if self.wants_light(column) && !self.wants_mesh(column) {
                let keys: Vec<SubChunkKey> = self.resident.column(column).copied().collect();
                for key in keys {
                    self.mark_light_dirty_exact_with_priority(key, false);
                }
            }
        }
    }

    /// Relights and remeshes every section a column already holds.
    fn refresh_full_detail_column(&mut self, column: ChunkKey) {
        let keys: Vec<SubChunkKey> = self
            .resident
            .column(column)
            .chain(self.known_air.column(column))
            .copied()
            .collect();
        if keys.is_empty() {
            self.note_full_detail_drawn(column);
            return;
        }
        self.mark_changed_sources(keys, Instant::now());
    }

    /// Withdraws a column's meshes and summarises it, so far terrain draws its latest state.
    fn demote_full_detail_column(&mut self, column: ChunkKey) {
        if self.full_detail.drawn.remove(&column) {
            self.full_detail.drawn_revision += 1;
        }
        let keys: Vec<SubChunkKey> = self
            .resident
            .column(column)
            .chain(self.known_air.column(column))
            .copied()
            .collect();
        let now = Instant::now();
        for key in keys {
            self.mark_dirty_exact_ungated(key, now, false);
        }
        if self.loaded_columns.contains(&column) {
            self.note_far_column_complete(column);
        }
    }

    /// Marks a full-detail column drawn once it is loaded and none of its sections await a mesh.
    pub(super) fn note_full_detail_drawn(&mut self, column: ChunkKey) {
        if self.full_detail.radius.is_none()
            || !self.full_detail.columns.contains(&column)
            || self.full_detail.drawn.contains(&column)
            || !self.loaded_columns.contains(&column)
        {
            return;
        }
        let pending = self
            .resident
            .column(column)
            .chain(self.known_air.column(column))
            .any(|key| self.revisions.dirty(*key).is_some());
        if !pending {
            self.full_detail.drawn.insert(column);
            self.full_detail.drawn_revision += 1;
        }
    }

    /// Drops retired columns from the drawn set.
    pub(super) fn forget_full_detail_columns(&mut self, columns: &BTreeSet<ChunkKey>) {
        let before = self.full_detail.drawn.len();
        self.full_detail
            .drawn
            .retain(|column| !columns.contains(column));
        if self.full_detail.drawn.len() != before {
            self.full_detail.drawn_revision += 1;
        }
    }
}

/// The column and its eight neighbours.
fn neighbourhood(column: ChunkKey) -> impl Iterator<Item = ChunkKey> {
    (-1..=1).flat_map(move |dx| {
        (-1..=1).map(move |dz| {
            ChunkKey::new(
                column.dimension,
                column.x.saturating_add(dx),
                column.z.saturating_add(dz),
            )
        })
    })
}

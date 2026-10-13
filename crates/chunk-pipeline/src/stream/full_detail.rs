//! Full-detail radius: only columns near the player are lit, meshed and uploaded; the rest
//! keep their block data and are drawn by far terrain from their summaries.
//!
//! A column with resident data is promoted when it comes within the radius and demoted once
//! it is more than [`FULL_DETAIL_HYSTERESIS_CHUNKS`] past it. Promotion remeshes it and near
//! terrain takes it over only when every mesh is uploaded; demotion hands it back to far
//! terrain at once, re-summarised from its current blocks, before its meshes are withdrawn.

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
        if radius.is_none() {
            // Everything is full detail again: relight and remesh what was left out, including
            // columns still arriving.
            let demoted: Vec<ChunkKey> = self
                .resident
                .columns()
                .filter(|column| !self.full_detail.columns.contains(column))
                .collect();
            self.full_detail.columns.clear();
            for column in demoted {
                self.refresh_full_detail_column(column);
            }
            return;
        }
        if !was_limited {
            // Every column was full detail; those past the new radius are demoted below.
            let columns: Vec<ChunkKey> = self.resident.columns().collect();
            self.full_detail.columns.extend(columns);
        }
        self.reevaluate_full_detail();
    }

    /// The full-detail radius in effect, if any.
    #[must_use]
    pub const fn full_detail_radius(&self) -> Option<i32> {
        self.full_detail.radius
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
        self.mark_changed_sources(keys, Instant::now());
        self.note_near_drawn(column);
    }

    /// Withdraws a column's meshes and summarises it, so far terrain draws its latest state.
    fn demote_full_detail_column(&mut self, column: ChunkKey) {
        self.forget_near_drawn([&column]);
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

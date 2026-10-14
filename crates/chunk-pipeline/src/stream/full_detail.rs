//! Full-detail radius: only columns near the player are lit, meshed and uploaded; the rest
//! keep their block data and are drawn by far terrain from their summaries.
//!
//! A column with resident data is promoted when it comes within the radius and demoted once
//! it is more than [`FULL_DETAIL_HYSTERESIS_CHUNKS`] past it. Both hand-overs are two-phase:
//! promotion remeshes the column and near terrain takes it only when every mesh is published;
//! demotion re-summarises it and keeps its meshes until far terrain reports its tile built
//! ([`WorldStream::finish_far_handoff`]), and only then withdraws them.

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
    /// Derived from membership by `insert_column` and `remove_column` only.
    ordered_columns: BTreeSet<ChunkKey>,
    /// Near-drawn columns past the band, still meshed until far terrain covers them.
    handoffs: HashMap<ChunkKey, Handoff>,
    scan: Option<DetailScan>,
    /// The first limit must also visit columns that were drawn without one.
    scan_residents: bool,
    /// Columns drawn without a limit remain active until the scan reaches them.
    initial_generation: Option<u64>,
    initial_cursor: Option<ChunkKey>,
}

struct DetailScan {
    demotion_cursor: Option<ChunkKey>,
    demoting: bool,
    entering: Option<column_scan::EnteringColumns>,
}

#[derive(Clone, Copy)]
struct Handoff {
    /// Summaries issued after this generation saw the column's blocks since it left or last
    /// changed.
    after: u64,
    delivered: bool,
}

impl FullDetail {
    /// Adds membership and its resumable scan index together.
    fn insert_column(&mut self, column: ChunkKey) {
        if self.columns.insert(column) {
            self.ordered_columns.insert(column);
        }
    }

    /// Removes membership and its resumable scan index together.
    fn remove_column(&mut self, column: ChunkKey) {
        if self.columns.remove(&column) {
            self.ordered_columns.remove(&column);
        }
    }

    /// True when `column` is meshed and uploaded; every column is without a radius.
    pub(super) fn wants_mesh(&self, column: ChunkKey, resident: &ColumnSubChunkSet) -> bool {
        self.radius.is_none()
            || self.contains_column(column, resident)
            || self.handoffs.contains_key(&column)
    }

    /// Includes unvisited columns that were full detail when the first limit was requested.
    fn contains_column(&self, column: ChunkKey, resident: &ColumnSubChunkSet) -> bool {
        self.columns.contains(&column)
            || (self.initial_cursor.is_none_or(|cursor| column > cursor)
                && self
                    .initial_generation
                    .is_some_and(|generation| resident.existed_at(column, generation)))
    }
}

impl WorldStream {
    /// Reports maintenance that needs another slice even without worker completions.
    pub(super) fn has_pending_full_detail(&self) -> bool {
        self.full_detail.scan.is_some()
            || self.full_detail.scan_residents
            || (self.full_detail.radius.is_some()
                && self.full_detail.centre != Some(self.player_chunk()))
    }

    /// Limits lighting, meshing and uploads to columns within `radius` chunks of the player;
    /// `None` restores full detail everywhere.
    pub fn set_full_detail_radius(&mut self, radius: Option<i32>) {
        let radius = radius.map(|radius| radius.max(1));
        if self.full_detail.radius == radius {
            return;
        }
        if self.full_detail.radius.is_none() {
            let retired = std::mem::take(&mut self.full_detail);
            if !retired.columns.is_empty() || !retired.handoffs.is_empty() {
                rayon::spawn(move || drop(retired));
            }
            self.full_detail.initial_generation = Some(self.resident.column_generation());
        }
        if radius.is_none() {
            self.full_detail.initial_generation = None;
        }
        self.full_detail.initial_cursor = None;
        self.full_detail.scan_residents |= radius.is_none() || self.full_detail.radius.is_none();
        self.full_detail.radius = radius;
        self.full_detail.centre = None;
        self.full_detail.scan = None;
        if !self.between_frames_service {
            self.reevaluate_full_detail();
        }
    }

    /// The full-detail radius in effect, if any.
    #[must_use]
    pub const fn full_detail_radius(&self) -> Option<i32> {
        self.full_detail.radius
    }

    /// True when `column` is meshed and uploaded; every column is without a radius.
    pub(super) fn wants_mesh(&self, column: ChunkKey) -> bool {
        self.full_detail.wants_mesh(column, &self.resident)
    }

    /// Light is solved one column past full detail, so edge meshes sample current light.
    pub(super) fn wants_light(&self, column: ChunkKey) -> bool {
        self.full_detail.radius.is_none()
            || (-1..=1).any(|dx| {
                (-1..=1).any(|dz| {
                    self.full_detail.contains_column(
                        ChunkKey::new(
                            column.dimension,
                            column.x.saturating_add(dx),
                            column.z.saturating_add(dz),
                        ),
                        &self.resident,
                    )
                })
            })
    }

    /// Promotes and demotes columns when the player's column or the radius changes.
    pub(super) fn reevaluate_full_detail(&mut self) {
        if self.full_detail.radius.is_none()
            && !self.full_detail.scan_residents
            && self.full_detail.scan.is_none()
        {
            return;
        }
        let centre = self.player_chunk();
        if self.full_detail.centre.is_none()
            || (self.full_detail.radius.is_some() && self.full_detail.centre != Some(centre))
        {
            let previous = self
                .full_detail
                .scan
                .is_none()
                .then(|| {
                    self.full_detail
                        .centre
                        .zip(self.full_detail.radius)
                        .map(|(centre, radius)| column_scan::Disk { centre, radius })
                })
                .flatten();
            self.full_detail.centre = Some(centre);
            self.full_detail.initial_cursor = None;
            self.full_detail.scan = Some(DetailScan {
                demotion_cursor: None,
                demoting: true,
                entering: self.full_detail.radius.map(|radius| {
                    column_scan::EnteringColumns::new(
                        column_scan::Disk { centre, radius },
                        previous,
                    )
                }),
            });
        }
        let Some(mut scan) = self.full_detail.scan.take() else {
            return;
        };
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("stream.full_detail").entered();
        let mut items = 0;
        while !self.maintenance_should_yield(items) {
            let started = Instant::now();
            #[cfg(feature = "tracy")]
            let _item =
                tracing::info_span!("stream.service_item_max", kind = "full_detail").entered();
            if scan.demoting {
                let column = if self.full_detail.scan_residents || self.full_detail.radius.is_none()
                {
                    self.resident.next_column(scan.demotion_cursor)
                } else {
                    self.full_detail
                        .ordered_columns
                        .range(column_scan::after(scan.demotion_cursor))
                        .next()
                        .copied()
                };
                let Some(column) = column else {
                    scan.demoting = false;
                    self.full_detail.scan_residents = false;
                    self.full_detail.initial_generation = None;
                    self.full_detail.initial_cursor = None;
                    continue;
                };
                scan.demotion_cursor = Some(column);
                self.full_detail.initial_cursor = Some(column);
                if let Some(radius) = self.full_detail.radius {
                    let disk = column_scan::Disk {
                        centre,
                        radius: radius.saturating_add(FULL_DETAIL_HYSTERESIS_CHUNKS),
                    };
                    let initial_member = self
                        .full_detail
                        .initial_generation
                        .is_none_or(|generation| self.resident.existed_at(column, generation));
                    if initial_member && disk.contains(column) {
                        if self.full_detail.scan_residents {
                            self.full_detail.insert_column(column);
                        } else {
                            let (_, last) = disk.row(column.x).expect("a kept column has a row");
                            scan.demotion_cursor =
                                Some(ChunkKey::new(column.dimension, column.x, last));
                        }
                    } else if initial_member {
                        self.full_detail.remove_column(column);
                        self.demote_full_detail_column(column);
                    }
                } else if !self.full_detail.columns.contains(&column)
                    || self.full_detail.handoffs.contains_key(&column)
                {
                    self.refresh_full_detail_column(column);
                }
            } else if let Some(column) = scan.entering.as_mut().and_then(Iterator::next) {
                if !self.full_detail.columns.contains(&column) {
                    let unlit: Vec<_> = neighbourhood(column)
                        .filter(|neighbour| !self.wants_light(*neighbour))
                        .collect();
                    self.full_detail.insert_column(column);
                    self.full_detail.handoffs.remove(&column);
                    self.refresh_full_detail_column(column);
                    for neighbour in unlit {
                        if !self.wants_mesh(neighbour) {
                            let keys: Vec<_> = self.resident.column(neighbour).copied().collect();
                            for key in keys {
                                self.mark_light_dirty_exact_with_priority(key, false);
                            }
                        }
                    }
                }
            } else {
                if self.full_detail.radius.is_none() {
                    let retired = std::mem::take(&mut self.full_detail);
                    rayon::spawn(move || drop(retired));
                    self.full_detail.centre = Some(centre);
                }
                return;
            }
            self.record_service_item(started);
            self.maintenance_guarantee = false;
            self.maintenance_items += 1;
            items += 1;
        }
        self.full_detail.scan = Some(scan);
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

    /// Starts handing a column to far terrain: one with any mesh on screen is re-summarised
    /// and keeps its meshes until [`WorldStream::finish_far_handoff`]; one with none is
    /// withdrawn now, as nothing near draws it.
    fn demote_full_detail_column(&mut self, column: ChunkKey) {
        let on_screen = self.near_drawn_contains(column)
            || self
                .resident
                .column(column)
                .chain(self.known_air.column(column))
                .any(|key| {
                    self.applied_mesh_generations.contains_key(key)
                        && !self.withdrawn_meshes.contains(key)
                });
        if on_screen {
            let handoff = self.pending_handoff();
            self.full_detail.handoffs.insert(column, handoff);
            self.note_far_column_complete(column);
        } else {
            self.withdraw_full_detail_column(column);
        }
    }

    /// Queues removal of the column's near meshes after its far coverage is ready.
    fn withdraw_full_detail_column(&mut self, column: ChunkKey) {
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

    /// Columns leaving full detail whose current summary has been delivered; each is
    /// withdrawn by [`WorldStream::finish_far_handoff`] once far terrain draws it.
    pub fn far_handoffs(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        self.full_detail
            .handoffs
            .iter()
            .filter(|(_, handoff)| handoff.delivered)
            .map(|(column, _)| *column)
    }

    /// Withdraws a column's meshes now that far terrain draws it from a delivered summary.
    pub fn finish_far_handoff(&mut self, column: ChunkKey) {
        if self
            .full_detail
            .handoffs
            .get(&column)
            .is_some_and(|handoff| handoff.delivered)
        {
            self.full_detail.handoffs.remove(&column);
            self.withdraw_full_detail_column(column);
        }
    }

    /// True once every loaded column holding sections is drawn by near terrain.
    #[must_use]
    pub fn all_loaded_near_drawn(&self) -> bool {
        self.loaded_columns.iter().all(|column| {
            self.near_drawn_contains(*column)
                || self
                    .resident
                    .column(*column)
                    .chain(self.known_air.column(*column))
                    .next()
                    .is_none()
        })
    }

    /// A change in a column far terrain draws, or is about to, refreshes its summary.
    pub(super) fn note_far_change(&mut self, column: ChunkKey) {
        if !self.loaded_columns.contains(&column) {
            return;
        }
        let pending = self.pending_handoff();
        if let Some(handoff) = self.full_detail.handoffs.get_mut(&column) {
            *handoff = pending;
        } else if self.wants_mesh(column) {
            return;
        }
        self.note_far_column_complete(column);
    }

    /// Marks handing-over columns whose summary was just delivered.
    pub(super) fn note_far_summaries_delivered(&mut self, delivered: &[FarColumnSummary]) {
        for summary in delivered {
            if let Some(handoff) = self.full_detail.handoffs.get_mut(&summary.column)
                && summary.generation > handoff.after
            {
                handoff.delivered = true;
            }
        }
    }

    /// A hand-over waiting for a summary issued from now on.
    fn pending_handoff(&self) -> Handoff {
        Handoff {
            after: self.far_terrain_issued_generation(),
            delivered: false,
        }
    }

    /// Drops evicted columns from any hand-over.
    pub(super) fn forget_full_detail_handoffs(&mut self, columns: &BTreeSet<ChunkKey>) {
        for column in columns {
            self.full_detail.handoffs.remove(column);
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

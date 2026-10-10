use {super::*, render_api::MAX_VIEW_RADIUS_CHUNKS};

/// Tracks new data in one publisher cohort without treating re-sends as progress.
pub(super) struct ArrivalCohort {
    epoch: u64,
    view: ViewCohort,
    seen: HashSet<(ChunkKey, Option<i32>)>,
    deadline: Instant,
}

impl WorldStream {
    pub(super) fn provisionally_rebase_for_local_teleport(&mut self, position: [f32; 3]) {
        let center = position.map(floor_to_i32);
        let destination = ChunkKey::new(
            self.authority.current_dimension(),
            center[0].div_euclid(16),
            center[2].div_euclid(16),
        );
        if self.column_is_active(destination) {
            return;
        }

        // Vanilla keeps chunk data across a teleport and drops only what the moved view no
        // longer covers, so overlap stays presented.
        self.arrival_cohort = None;
        self.requests.transport_pending = 0;
        self.publisher.center = Some(center);
        self.prune_column_deadlines();
        let stale = self
            .tracked_columns()
            .into_iter()
            .chain(self.requests.collision_failures.iter().copied())
            .filter(|column| !self.column_is_data_interesting(*column))
            .collect::<BTreeSet<_>>();
        self.evict_columns(stale.into_iter().collect());
        self.publisher.begin_local_rebase();
    }

    pub(super) fn sync_resident(&mut self, key: SubChunkKey) {
        if self.authority.terrain().sub_chunk(key).is_some() {
            self.resident.insert(key);
            self.known_air.remove(&key);
        } else {
            self.record_known_air(key);
        }
    }
    pub(super) fn record_known_air(&mut self, key: SubChunkKey) -> bool {
        let became_resident = self.resident.insert(key);
        let became_known_air = self.known_air.insert(key);
        if became_known_air {
            self.mesh_dependency_masks.remove(&key);
        }
        self.set_connectivity(key, Some(FaceConnectivity::all()));
        became_resident || became_known_air
    }
    pub(super) fn evict_column(&mut self, key: ChunkKey) {
        self.evict_columns(BTreeSet::from([key]));
    }

    /// Retires authority in one pass, then releases packed column allocations off-thread.
    /// Work scales with the retired columns and their neighbours, not with the whole view.
    pub(super) fn evict_columns(&mut self, columns: BTreeSet<ChunkKey>) {
        if columns.is_empty() {
            return;
        }
        self.actor_block_syncs.remove_columns(&columns);
        for &column in &columns {
            self.light_diagnostics.remove_column(column);
            self.evict_block_crack_column(column);
            self.loaded_columns.remove(&column);
            self.requests.collision_failures.remove(&column);
        }
        self.block_entity_visuals.remove_chunks(&columns);
        self.requests.purge_columns(&columns);
        let mut changed = self.resident_keys_in_columns(&columns);
        let removing_all = changed.len() == self.resident.len();
        for &column in &columns {
            if let Some(range) = self.authority.dimension_range(column.dimension) {
                for offset in 0..range.sub_chunk_count {
                    let key =
                        SubChunkKey::from_chunk(column, range.base_sub_chunk_y + offset as i32);
                    if self.authority.terrain().biome_storage(key).is_some() {
                        changed.insert(key);
                    }
                }
            }
        }
        let (removed, retired) = self.authority.detach_chunks(&columns);
        changed.extend(removed);
        let old_connectivity_len = self.connectivity.len();
        let retired_indexes = removing_all.then(|| {
            (
                std::mem::take(&mut self.resident),
                std::mem::take(&mut self.known_air),
                std::mem::take(&mut self.applied_mesh_generations),
                std::mem::take(&mut self.mesh_dependency_masks),
                std::mem::take(&mut self.connectivity),
            )
        });
        if !changed.is_empty() {
            // Records exist only for resident or known-air keys; keys that left residency
            // earlier retire their own records when their removal publishes.
            let retiring = columns
                .iter()
                .flat_map(|&column| self.known_air.column(column).copied())
                .chain(changed.iter().copied())
                .collect::<Vec<_>>();
            for key in &retiring {
                self.resident.remove(key);
                self.known_air.remove(key);
                self.applied_mesh_generations.remove(key);
                self.mesh_dependency_masks.remove(key);
            }
            // One cell sweep beats per-key removal, which rescans overflow keys each time.
            self.connectivity
                .retain(|key| !columns.contains(&key.chunk()));
        }
        if self.connectivity.len() != old_connectivity_len {
            self.bump_connectivity_generation();
        }
        let (light_dirty, mesh_dirty) = if removing_all {
            self.lighting.retire_all();
            Default::default()
        } else {
            self.surviving_neighbours_of(&changed)
        };
        let now = Instant::now();
        for key in changed {
            if !removing_all {
                self.lighting.remove_key(key);
            }
            self.mark_dirty_exact(key, now);
        }
        for key in light_dirty {
            self.mark_light_dirty_exact_with_priority(key, false);
        }
        for key in mesh_dirty {
            self.mark_dirty_exact(key, now);
        }
        if !retired.is_empty() || retired_indexes.is_some() {
            rayon::spawn(move || drop((retired, retired_indexes)));
        }
    }

    /// Resident keys outside the removed columns that sample a removed key: face neighbours
    /// for light, and the full 3×3×3 neighbourhood for meshing. Every removed key belongs to
    /// a retired column, so only the eight surrounding columns can hold survivors.
    fn surviving_neighbours_of(
        &self,
        removed: &BTreeSet<SubChunkKey>,
    ) -> (BTreeSet<SubChunkKey>, BTreeSet<SubChunkKey>) {
        let mut heights = BTreeMap::<ChunkKey, BTreeSet<i32>>::new();
        for key in removed {
            heights.entry(key.chunk()).or_default().insert(key.y);
        }
        let (mut light, mut mesh) = (BTreeSet::new(), BTreeSet::new());
        for (column, removed_heights) in &heights {
            for dx in -1_i32..=1 {
                for dz in -1_i32..=1 {
                    let (Some(x), Some(z)) = (column.x.checked_add(dx), column.z.checked_add(dz))
                    else {
                        continue;
                    };
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    let face = dx == 0 || dz == 0;
                    for &key in self.resident.column(ChunkKey::new(column.dimension, x, z)) {
                        if face && removed_heights.contains(&key.y) {
                            light.insert(key);
                        }
                        let below = key.y.saturating_sub(1);
                        let above = key.y.saturating_add(1);
                        if removed_heights.range(below..=above).next().is_some() {
                            mesh.insert(key);
                        }
                    }
                }
            }
        }
        (light, mesh)
    }

    /// Collects only the sections belonging to the columns being retired.
    fn resident_keys_in_columns(&self, columns: &BTreeSet<ChunkKey>) -> BTreeSet<SubChunkKey> {
        columns
            .iter()
            .flat_map(|column| self.resident.column(*column).copied())
            .collect()
    }
    pub(super) fn evict_all_resident(&mut self) {
        self.light_diagnostics.columns.clear();
        self.unsent_column_deadlines.clear();
        self.arrival_cohort = None;
        let mut columns = self.tracked_columns();
        columns.extend(self.requests.collision_failures.iter().copied());
        self.evict_columns(columns);
    }
    pub(super) fn tracked_columns(&self) -> BTreeSet<ChunkKey> {
        let mut columns = self.loaded_columns.clone();
        columns.extend(self.requests.requested.keys().copied());
        columns.extend(self.resident.columns());
        columns.extend(self.known_air.columns());
        columns
    }
    /// Re-evaluates chunk-grid retention against the local player's current
    /// chunk and the server-confirmed radius, evicting every tracked column and
    /// pruning every announced requirement the grid no longer keeps. Cheap to
    /// call on every player move: it only rescans when the player's chunk or
    /// the confirmed radius changes.
    pub(super) fn reevaluate_chunk_retention(&mut self) -> bool {
        let Some(radius) = self.chunk_radius else {
            return false;
        };
        let center = self.player_chunk();
        if self.last_retention_center == Some(center) && self.last_retention_radius == Some(radius)
        {
            return false;
        }
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("stream.retention").entered();
        self.last_retention_center = Some(center);
        self.last_retention_radius = Some(radius);
        let center_xz = [center.x, center.z];
        let current_dimension = self.authority.current_dimension();
        let is_retained = |key: &ChunkKey| {
            key.dimension == current_dimension && chunk_in_view(radius, [key.x, key.z], center_xz)
        };
        // Failed requests can leave diagnostic evidence without any resident terrain.
        self.light_diagnostics
            .columns
            .retain(|key, _| is_retained(key));
        self.publisher.required_columns.retain(is_retained);
        self.unsent_column_deadlines
            .retain(|key, _| is_retained(key));
        let stale = self
            .tracked_columns()
            .into_iter()
            .filter(|key| !is_retained(key))
            .collect::<Vec<_>>();
        self.evict_columns(stale.into_iter().collect());
        true
    }
    /// Retains terrain around completed local physics without changing the last server position.
    /// Rejects stale owners and any physics that has not yet applied a committed spatial control.
    pub fn retain_for_local_player(
        &mut self,
        actor_session_id: u64,
        dimension: i32,
        dimension_epoch: u64,
        position: [f32; 3],
    ) -> bool {
        if actor_session_id != self.authority.actor_session_id()
            || dimension != self.authority.current_dimension()
            || dimension_epoch != self.authority.form_dimension_epoch()
            || !position.into_iter().all(f32::is_finite)
            || self.authority.has_pending_spatial_control()
        {
            return false;
        }
        self.local_player_chunk = Some(ChunkKey::new(
            dimension,
            floor_to_i32(position[0]).div_euclid(16),
            floor_to_i32(position[2]).div_euclid(16),
        ));
        self.request_chunk_retention()
    }

    /// Retains terrain around the committed server position while no local physics owns the player.
    pub fn retain_for_server_position(&mut self) -> bool {
        self.local_player_chunk = None;
        self.request_chunk_retention()
    }

    /// Re-evaluates retention now, or, for a stream its between-frames service polls, marks it
    /// due so the terrain eviction runs before the next commit or poll, usually on the service.
    /// Returns whether the retained grid changed.
    fn request_chunk_retention(&mut self) -> bool {
        if !self.between_frames_service {
            return self.reevaluate_chunk_retention();
        }
        let Some(radius) = self.chunk_radius else {
            return false;
        };
        let center = self.player_chunk();
        if self.last_retention_center == Some(center) && self.last_retention_radius == Some(radius)
        {
            return false;
        }
        self.retention_due = true;
        self.retire_dropped_requests(center, radius);
        true
    }

    /// The frame flushes requests before a deferred eviction runs, so requests the new grid
    /// drops retire now. A column holding only requests evicts whole, as no terrain work is
    /// deferred for it; one holding terrain loses only its requests until the eviction.
    fn retire_dropped_requests(&mut self, center: ChunkKey, radius: i32) {
        let dimension = self.authority.current_dimension();
        let (request_only, with_terrain): (BTreeSet<_>, BTreeSet<_>) = self
            .requests
            .requested
            .keys()
            .copied()
            .filter(|key| {
                key.dimension != dimension
                    || !chunk_in_view(radius, [key.x, key.z], [center.x, center.z])
            })
            .partition(|column| {
                !self.loaded_columns.contains(column)
                    && self.resident.column(*column).next().is_none()
                    && self.known_air.column(*column).next().is_none()
            });
        self.evict_columns(request_only);
        self.requests.purge_columns(&with_terrain);
    }

    /// Runs a retention re-evaluation deferred by [`Self::request_chunk_retention`].
    pub(super) fn apply_due_chunk_retention(&mut self) {
        if std::mem::take(&mut self.retention_due) {
            self.reevaluate_chunk_retention();
        }
    }

    /// Local physics advances the player grid between server corrections.
    fn player_chunk(&self) -> ChunkKey {
        if let Some(chunk) = self.local_player_chunk {
            return chunk;
        }
        let position = self.authority.resolved_server_position().position;
        ChunkKey::new(
            self.authority.current_dimension(),
            floor_to_i32(position[0]).div_euclid(16),
            floor_to_i32(position[2]).div_euclid(16),
        )
    }
    pub(super) fn active_radius_chunks(&self) -> i32 {
        match (self.publisher.radius_chunks, self.chunk_radius) {
            (Some(publisher), Some(chunk)) => publisher.min(chunk),
            (Some(radius), None) | (None, Some(radius)) => radius,
            (None, None) => render_api::UNGRANTED_VIEW_RADIUS_CHUNKS,
        }
        .clamp(0, MAX_VIEW_RADIUS_CHUNKS)
    }
    pub(super) fn column_is_active(&self, key: ChunkKey) -> bool {
        if key.dimension != self.authority.current_dimension() {
            return false;
        }
        let Some(center) = self.publisher.center else {
            return true;
        };
        let radius = u64::try_from(self.active_radius_chunks()).unwrap_or(0);
        let center_x = center[0].div_euclid(16);
        let center_z = center[2].div_euclid(16);
        i64::from(key.x).abs_diff(i64::from(center_x)) <= radius
            && i64::from(key.z).abs_diff(i64::from(center_z)) <= radius
    }
    /// Reports whether inbound world data belongs to either server-established
    /// interest scope. Publisher scope remains the control/rebase authority;
    /// the confirmed player grid independently retains ordinary world data.
    pub(super) fn column_is_data_interesting(&self, key: ChunkKey) -> bool {
        if key.dimension != self.authority.current_dimension() {
            return false;
        }
        self.column_is_active(key)
            || self.chunk_radius.is_some_and(|radius| {
                let player = self.player_chunk();
                chunk_in_view(
                    radius.clamp(0, MAX_VIEW_RADIUS_CHUNKS),
                    [key.x, key.z],
                    [player.x, player.z],
                )
            })
    }
    /// Drops deadlines outside the current retained view and publisher scope.
    pub(super) fn prune_column_deadlines(&mut self) {
        self.unsent_column_deadlines = std::mem::take(&mut self.unsent_column_deadlines)
            .into_iter()
            .filter(|(key, _)| self.column_is_data_interesting(*key))
            .collect();
    }

    /// Starts a missing column's deadline only when adjacent data first arrives.
    pub(super) fn record_column_arrival(&mut self, column: ChunkKey, now: Instant) {
        self.record_cohort_progress(column, None, now);
        for dx in -1..=1 {
            for dz in -1..=1 {
                let Some((x, z)) = column.x.checked_add(dx).zip(column.z.checked_add(dz)) else {
                    continue;
                };
                let neighbour = ChunkKey::new(column.dimension, x, z);
                if self.column_is_data_interesting(neighbour) {
                    self.unsent_column_deadlines
                        .entry(neighbour)
                        .or_insert(now + UNSENT_COLUMN_GRACE);
                }
            }
        }
    }

    /// Counts each delivered slot once in the current cohort's quiet period.
    pub(super) fn record_sub_chunk_arrival(&mut self, key: SubChunkKey, now: Instant) {
        self.record_column_arrival(key.chunk(), now);
        self.record_cohort_progress(key.chunk(), Some(key.y), now);
    }

    /// Renews only for new data in the current publisher epoch and bounds.
    fn record_cohort_progress(&mut self, column: ChunkKey, y: Option<i32>, now: Instant) {
        let Some(view) = self.publisher.cohort else {
            return;
        };
        if !view.contains_column(column.dimension, [column.x, column.z]) {
            return;
        }
        if self
            .arrival_cohort
            .as_ref()
            .is_none_or(|cohort| cohort.epoch != self.publisher.epoch || cohort.view != view)
        {
            self.arrival_cohort = Some(ArrivalCohort {
                epoch: self.publisher.epoch,
                view,
                seen: HashSet::new(),
                deadline: now + UNSENT_COLUMN_GRACE,
            });
        }
        let cohort = self.arrival_cohort.as_mut().expect("cohort initialized");
        if cohort.seen.insert((column, y)) {
            cohort.deadline = now + UNSENT_COLUMN_GRACE;
        }
    }

    /// Whether the server still owes `key`: it is in range and unknown, and either requested or
    /// in an unsent column whose local deadline has not elapsed.
    /// This retained timeout fallback is provisional; vanilla requires eligible columns.
    #[cfg(test)]
    pub(super) fn sub_chunk_is_due(&self, key: SubChunkKey, now: Instant) -> bool {
        self.sub_chunk_is_due_in(key, || self.column_due(key.chunk(), now))
    }

    /// [`Self::sub_chunk_is_due`] with the column's facts supplied, so keys sharing a column
    /// read them once; `column` runs only when the key's own state does not decide.
    pub(super) fn sub_chunk_is_due_in(
        &self,
        key: SubChunkKey,
        column: impl FnOnce() -> ColumnDue,
    ) -> bool {
        if self.light_source_is_known(key) {
            return false;
        }
        let Some(range) = self.authority.dimension_range(key.dimension) else {
            return false;
        };
        let end = range
            .base_sub_chunk_y
            .saturating_add(i32::try_from(range.sub_chunk_count).unwrap_or(i32::MAX));
        if key.y < range.base_sub_chunk_y || key.y >= end {
            return false;
        }
        self.requests.is_expected(key) || column().unsent_owes
    }

    /// The column facts [`Self::sub_chunk_is_due`] reads once a key's own state is undecided.
    pub(super) fn column_due(&self, column: ChunkKey, now: Instant) -> ColumnDue {
        let loaded = self.loaded_columns.contains(&column);
        let requested = self.requests.requested.contains_key(&column);
        let unsent_owes = !loaded
            && !requested
            && (self
                .unsent_column_deadlines
                .get(&column)
                .is_some_and(|deadline| now < *deadline)
                || self.arrival_cohort.as_ref().is_some_and(|cohort| {
                    cohort.epoch == self.publisher.epoch
                        && Some(cohort.view) == self.publisher.cohort
                        && cohort
                            .view
                            .contains_column(column.dimension, [column.x, column.z])
                        && now < cohort.deadline
                }))
            && self.column_is_data_interesting(column)
            && self.publisher.cohort.is_none_or(|cohort| {
                let dx = i64::from(column.x) - i64::from(cohort.center[0]);
                let dz = i64::from(column.z) - i64::from(cohort.center[1]);
                let radius = i64::from(cohort.radius);
                dx * dx + dz * dz <= radius * radius
            });
        ColumnDue {
            loaded,
            requested,
            unsent_owes,
        }
    }
}

/// One column's residency facts for deciding whether its sections are still owed.
#[derive(Debug, Clone, Copy)]
pub(super) struct ColumnDue {
    loaded: bool,
    requested: bool,
    /// Unloaded and unrequested, yet inside an unsent-column or cohort grace window.
    unsent_owes: bool,
}

impl ColumnDue {
    /// Loaded with nothing outstanding: none of its sections can be owed.
    pub(super) const fn settled(self) -> bool {
        self.loaded && !self.requested
    }
}

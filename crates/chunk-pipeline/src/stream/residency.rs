use super::*;

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
        // longer covers (`NetworkChunkSubscriber::moveRegion`), so overlap stays presented.
        self.arrival_cohort = None;
        self.transport_pending_requests = 0;
        self.publisher_center = Some(center);
        self.prune_column_deadlines();
        let stale = self
            .tracked_columns()
            .into_iter()
            .chain(self.request_collision_failures.iter().copied())
            .filter(|column| !self.column_is_data_interesting(*column))
            .collect::<BTreeSet<_>>();
        self.evict_columns(stale.into_iter().collect());
        self.committed_view_cohort = None;
        self.required_columns.clear();
        self.provisional_publisher_rebase = true;
        self.local_resets_armed = self.local_resets_armed.saturating_add(1);
        self.local_reset_dispatch_count = 0;
        self.local_reset_dispatch_total = 0;
        self.local_reset_dispatch_active = true;
        self.local_reset_dispatch_classes = [None; MAX_LOCAL_RESET_DISPATCH_EVIDENCE];
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
    pub(super) fn evict_columns(&mut self, columns: BTreeSet<ChunkKey>) {
        if columns.is_empty() {
            return;
        }
        for &column in &columns {
            self.evict_block_crack_column(column);
            self.loaded_columns.remove(&column);
            self.request_collision_failures.remove(&column);
        }
        self.block_entity_visuals.remove_chunks(&columns);
        self.purge_sub_chunk_columns_state(&columns);
        let mut changed = self.resident_keys_in_columns(&columns);
        let removing_all = changed.len() == self.resident.len();
        let mut biome_dirty = BTreeSet::new();
        for &column in &columns {
            if let Some(range) = vanilla_dimension_range(column.dimension) {
                for offset in 0..range.sub_chunk_count {
                    let key =
                        SubChunkKey::from_chunk(column, range.base_sub_chunk_y + offset as i32);
                    if self.authority.terrain().biome_storage(key).is_some() {
                        changed.insert(key);
                        if !removing_all {
                            biome_dirty.extend(key.biome_mesh_dependents());
                        }
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
            self.resident.retain(|key| !columns.contains(&key.chunk()));
            self.known_air.retain(|key| !columns.contains(&key.chunk()));
            self.applied_mesh_generations
                .retain(|key, _| !columns.contains(&key.chunk()));
            self.mesh_dependency_masks
                .retain(|key, _| !columns.contains(&key.chunk()));
            self.connectivity
                .retain(|key, _| !columns.contains(&key.chunk()));
        }
        if self.connectivity.len() != old_connectivity_len {
            self.bump_connectivity_generation();
        }
        let now = Instant::now();
        let mut light_dirty = BTreeSet::new();
        if removing_all {
            self.retire_all_lighting();
        }
        for key in changed {
            if !removing_all {
                light_dirty.extend(
                    key.mesh_dependents()
                        .filter(|key| self.resident.contains(key)),
                );
                biome_dirty.extend(
                    key.mesh_neighbourhood_dependents()
                        .filter(|key| self.resident.contains(key)),
                );
                self.remove_light_key_without_invalidation(key);
            }
            self.mark_dirty_exact(key, now);
        }
        for key in light_dirty {
            self.mark_light_dirty_exact_with_priority(key, false);
        }
        for key in biome_dirty {
            if self.resident.contains(&key) {
                self.mark_dirty_exact(key, now);
            }
        }
        if !retired.is_empty() || retired_indexes.is_some() {
            rayon::spawn(move || drop((retired, retired_indexes)));
        }
    }
    /// Fresh column arrivals search one ordered X range instead of every resident slot.
    fn resident_keys_in_columns(&self, columns: &BTreeSet<ChunkKey>) -> BTreeSet<SubChunkKey> {
        if columns.len() == 1 {
            let column = *columns.first().unwrap();
            let first = SubChunkKey::new(column.dimension, column.x, i32::MIN, i32::MIN);
            let last = SubChunkKey::new(column.dimension, column.x, i32::MAX, i32::MAX);
            self.resident
                .range(first..=last)
                .filter(|key| key.z == column.z)
                .copied()
                .collect()
        } else {
            self.resident
                .iter()
                .copied()
                .filter(|key| columns.contains(&key.chunk()))
                .collect()
        }
    }
    pub(super) fn evict_all_resident(&mut self) {
        self.unsent_column_deadlines.clear();
        self.arrival_cohort = None;
        let mut columns = self
            .resident
            .iter()
            .map(|key| key.chunk())
            .collect::<BTreeSet<_>>();
        columns.extend(self.known_air.iter().map(|key| key.chunk()));
        columns.extend(self.loaded_columns.iter().copied());
        columns.extend(self.requested_sub_chunks.keys().copied());
        columns.extend(self.request_collision_failures.iter().copied());
        self.evict_columns(columns);
    }
    pub(super) fn tracked_columns(&self) -> BTreeSet<ChunkKey> {
        let mut columns = self.loaded_columns.clone();
        columns.extend(self.requested_sub_chunks.keys().copied());
        columns.extend(self.resident.iter().map(|key| key.chunk()));
        columns.extend(self.known_air.iter().map(|key| key.chunk()));
        columns
    }
    /// Re-evaluates chunk-grid retention against the local player's current
    /// chunk and the server-confirmed radius, evicting every tracked column and
    /// pruning every announced requirement the grid no longer keeps. Cheap to
    /// call on every player move: it only rescans when the player's chunk or
    /// the confirmed radius changes.
    pub(super) fn reevaluate_chunk_retention(&mut self) {
        let Some(radius) = self.chunk_radius else {
            return;
        };
        let center = self.player_chunk();
        if self.last_retention_center == Some(center) && self.last_retention_radius == Some(radius)
        {
            return;
        }
        self.last_retention_center = Some(center);
        self.last_retention_radius = Some(radius);
        let center_xz = [center.x, center.z];
        let current_dimension = self.authority.current_dimension();
        let is_retained = |key: &ChunkKey| {
            key.dimension == current_dimension && chunk_in_view(radius, [key.x, key.z], center_xz)
        };
        self.required_columns.retain(is_retained);
        self.unsent_column_deadlines
            .retain(|key, _| is_retained(key));
        let stale = self
            .tracked_columns()
            .into_iter()
            .filter(|key| !is_retained(key))
            .collect::<Vec<_>>();
        self.evict_columns(stale.into_iter().collect());
    }
    /// The local player's current chunk column, floored from the resolved
    /// server-authoritative position so negative coordinates land in the
    /// correct column.
    fn player_chunk(&self) -> ChunkKey {
        let position = self.authority.resolved_server_position().position;
        ChunkKey::new(
            self.authority.current_dimension(),
            floor_to_i32(position[0]).div_euclid(16),
            floor_to_i32(position[2]).div_euclid(16),
        )
    }
    pub(super) fn active_radius_chunks(&self) -> i32 {
        match (self.publisher_radius_chunks, self.chunk_radius) {
            (Some(publisher), Some(chunk)) => publisher.min(chunk),
            (Some(radius), None) | (None, Some(radius)) => radius,
            (None, None) => PHASE0_MAX_VIEW_RADIUS_CHUNKS,
        }
        .clamp(0, PHASE0_MAX_VIEW_RADIUS_CHUNKS)
    }
    pub(super) fn column_is_active(&self, key: ChunkKey) -> bool {
        if key.dimension != self.authority.current_dimension() {
            return false;
        }
        let Some(center) = self.publisher_center else {
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
                    radius.clamp(0, PHASE0_MAX_VIEW_RADIUS_CHUNKS),
                    [key.x, key.z],
                    [player.x, player.z],
                )
            })
    }
    pub(super) fn is_expected_sub_chunk(&self, key: SubChunkKey) -> bool {
        self.requested_sub_chunks
            .get(&key.chunk())
            .is_some_and(|expected| expected.contains_key(&key.y))
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
        let Some(view) = self.committed_view_cohort else {
            return;
        };
        if !view.contains_column(column.dimension, [column.x, column.z]) {
            return;
        }
        if self
            .arrival_cohort
            .as_ref()
            .is_none_or(|cohort| cohort.epoch != self.publisher_epoch || cohort.view != view)
        {
            self.arrival_cohort = Some(ArrivalCohort {
                epoch: self.publisher_epoch,
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
    pub(super) fn sub_chunk_is_due(&self, key: SubChunkKey, now: Instant) -> bool {
        if self.light_source_is_known(key) {
            return false;
        }
        let Some(range) = vanilla_dimension_range(key.dimension) else {
            return false;
        };
        let end = range
            .base_sub_chunk_y
            .saturating_add(i32::try_from(range.sub_chunk_count).unwrap_or(i32::MAX));
        if key.y < range.base_sub_chunk_y || key.y >= end {
            return false;
        }
        if self.is_expected_sub_chunk(key) {
            return true;
        }
        let column = key.chunk();
        if self.loaded_columns.contains(&column) || self.requested_sub_chunks.contains_key(&column)
        {
            return false;
        }
        (self
            .unsent_column_deadlines
            .get(&column)
            .is_some_and(|deadline| now < *deadline)
            || self.arrival_cohort.as_ref().is_some_and(|cohort| {
                cohort.epoch == self.publisher_epoch
                    && Some(cohort.view) == self.committed_view_cohort
                    && cohort
                        .view
                        .contains_column(column.dimension, [column.x, column.z])
                    && now < cohort.deadline
            }))
            && self.column_is_data_interesting(column)
            && self.committed_view_cohort.is_none_or(|cohort| {
                let dx = i64::from(column.x) - i64::from(cohort.center[0]);
                let dz = i64::from(column.z) - i64::from(cohort.center[1]);
                let radius = i64::from(cohort.radius);
                dx * dx + dz * dz <= radius * radius
            })
    }
}

use super::*;

impl WorldStream {
    pub(super) fn provisionally_rebase_for_local_teleport(&mut self, position: [f32; 3]) {
        let center = position.map(floor_to_i32);
        let destination = ChunkKey::new(
            self.current_dimension,
            center[0].div_euclid(16),
            center[2].div_euclid(16),
        );
        if self.column_is_active(destination) {
            return;
        }

        self.arm_local_reset(center);
    }

    pub(super) fn apply_same_location_reset(&mut self, sequence: u64) {
        let center = self
            .publisher_center
            .unwrap_or_else(|| self.resolved_server_position.position.map(floor_to_i32));
        let mut removal_keys = self
            .applied_mesh_generations
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        removal_keys.extend(self.revisions.entries.keys().copied());
        removal_keys.extend(self.pending_mesh.keys().copied());
        removal_keys.extend(self.in_flight.keys().copied());
        removal_keys.extend(self.mesh_changes.iter().map(|change| match change {
            WorldMeshChange::Upsert { key, .. } | WorldMeshChange::Remove { key, .. } => *key,
        }));

        // Disconnect old workers before clearing their identities. Their
        // completions may still be produced, but can no longer enter this
        // stream after the reset barrier commits.
        let (mesh_tx, mesh_rx) = bounded(WORK_RESULT_CAPACITY);
        self.mesh_tx = mesh_tx;
        self.mesh_rx = mesh_rx;

        self.evict_all_resident();
        self.store = ChunkStore::new();
        self.block_entity_visuals.clear();
        self.resident.clear();
        self.known_air.clear();
        self.loaded_columns.clear();
        self.connectivity.clear();
        self.bump_connectivity_generation();

        self.requests = RequestQueue::default();
        self.requested_sub_chunks.clear();
        self.request_collision_failures.clear();
        self.sub_chunk_deadlines.clear();
        self.correlated_sub_chunk_attempts.clear();
        self.admitted_sub_chunk_replies.clear();
        self.deferred_retries.clear();
        self.deferred_retry_set.clear();
        self.transport_pending_requests = 0;

        self.block_generations.clear();
        self.light_store = LightStore::default();
        self.light_ownership.clear();
        self.direct_sky.clear();
        self.light_failures.clear();
        self.light_revisions.entries.clear();
        self.pending_light.clear();
        self.in_flight_light.clear();
        self.light_waiters.clear();
        self.fatal_light_failure = false;
        let (light_tx, light_rx) = bounded(LIGHT_RESULT_CAPACITY);
        self.light_tx = light_tx;
        self.light_rx = light_rx;

        self.pending_mesh.clear();
        self.in_flight.clear();
        self.revisions.entries.clear();
        self.applied_mesh_generations.clear();
        self.mesh_dependency_masks.clear();
        self.mesh_changes.clear();
        // Publish fresh, monotonically newer removals for old renderer keys.
        // If replacement data reaches a key first, its still-newer revision
        // supersedes the removal without allowing stale geometry to return.
        let now = Instant::now();
        for key in removal_keys {
            self.mark_dirty_exact(key, now);
        }

        self.source_columns.clear();
        self.source_capture_sequence = None;
        let _ =
            self.actors
                .reset_dimension(self.actor_session_id, sequence, self.current_dimension);
        self.pending_same_location_reset = false;
        self.arm_local_reset(center);
    }

    fn arm_local_reset(&mut self, center: [i32; 3]) {
        self.evict_all_resident();
        self.transport_pending_requests = 0;
        self.publisher_center = Some(center);
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
        if self.store.sub_chunk(key).is_some() {
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
        self.block_entity_visuals.remove_chunk(key);
        self.loaded_columns.remove(&key);
        self.request_collision_failures.remove(&key);
        self.purge_sub_chunk_column_state(key);
        let mut changed = self
            .resident
            .iter()
            .copied()
            .filter(|resident| resident.chunk() == key)
            .collect::<BTreeSet<_>>();
        let biome_sources =
            vanilla_dimension_range(key.dimension).map_or_else(BTreeSet::new, |range| {
                (0..range.sub_chunk_count)
                    .filter_map(|offset| {
                        let y = range
                            .base_sub_chunk_y
                            .checked_add(i32::try_from(offset).ok()?)?;
                        let biome_key = SubChunkKey::from_chunk(key, y);
                        self.store.biome_storage(biome_key).map(|_| biome_key)
                    })
                    .collect::<BTreeSet<_>>()
            });
        let biome_dirty = biome_sources
            .iter()
            .copied()
            .flat_map(SubChunkKey::biome_mesh_dependents)
            .collect::<BTreeSet<_>>();
        changed.extend(biome_sources);
        changed.extend(self.store.evict_chunk(key));
        self.resident.retain(|resident| resident.chunk() != key);
        self.known_air.retain(|resident| resident.chunk() != key);
        self.applied_mesh_generations
            .retain(|resident, _| resident.chunk() != key);
        self.mesh_dependency_masks
            .retain(|resident, _| resident.chunk() != key);
        let old_connectivity_len = self.connectivity.len();
        self.connectivity
            .retain(|resident, _| resident.chunk() != key);
        if self.connectivity.len() != old_connectivity_len {
            self.bump_connectivity_generation();
        }
        let now = Instant::now();
        self.mark_changed_sources_with_mesh_dirty(changed, biome_dirty, now);
    }
    pub(super) fn evict_all_resident(&mut self) {
        let mut columns = self
            .resident
            .iter()
            .map(|key| key.chunk())
            .collect::<BTreeSet<_>>();
        columns.extend(self.known_air.iter().map(|key| key.chunk()));
        columns.extend(self.loaded_columns.iter().copied());
        columns.extend(self.requested_sub_chunks.keys().copied());
        columns.extend(self.request_collision_failures.iter().copied());
        for column in columns {
            self.evict_column(column);
        }
    }
    pub(super) fn tracked_columns(&self) -> BTreeSet<ChunkKey> {
        let mut columns = self.loaded_columns.clone();
        columns.extend(self.requested_sub_chunks.keys().copied());
        columns.extend(self.resident.iter().map(|key| key.chunk()));
        columns.extend(self.known_air.iter().map(|key| key.chunk()));
        columns
    }
    pub(super) fn evict_outside_active_radius(&mut self) {
        let Some(center) = self.publisher_center else {
            return;
        };
        let radius = self.active_radius_chunks();
        let center_x = center[0].div_euclid(16);
        let center_z = center[2].div_euclid(16);
        let mut columns = self
            .resident
            .iter()
            .map(|key| key.chunk())
            .filter(|key| {
                key.dimension != self.current_dimension
                    || i64::from(key.x).abs_diff(i64::from(center_x)) > radius as u64
                    || i64::from(key.z).abs_diff(i64::from(center_z)) > radius as u64
            })
            .collect::<BTreeSet<_>>();
        columns.extend(self.loaded_columns.iter().copied().filter(|key| {
            key.dimension != self.current_dimension
                || i64::from(key.x).abs_diff(i64::from(center_x)) > radius as u64
                || i64::from(key.z).abs_diff(i64::from(center_z)) > radius as u64
        }));
        columns.extend(self.requested_sub_chunks.keys().copied().filter(|key| {
            key.dimension != self.current_dimension
                || i64::from(key.x).abs_diff(i64::from(center_x)) > radius as u64
                || i64::from(key.z).abs_diff(i64::from(center_z)) > radius as u64
        }));
        for column in columns {
            self.evict_column(column);
        }
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
        if key.dimension != self.current_dimension {
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
    pub(super) fn is_expected_sub_chunk(&self, key: SubChunkKey) -> bool {
        self.requested_sub_chunks
            .get(&key.chunk())
            .is_some_and(|expected| expected.contains_key(&key.y))
    }
}

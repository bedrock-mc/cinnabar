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
    /// Re-evaluates chunk-grid retention against the local player's current
    /// chunk and the server-confirmed radius, evicting every tracked column the
    /// vanilla client's grid no longer keeps. Cheap to call on every player
    /// move: it only rescans when the player's chunk or the confirmed radius
    /// changes.
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
        let stale = self
            .tracked_columns()
            .into_iter()
            .filter(|key| {
                key.dimension != self.current_dimension
                    || !chunk_in_view(radius, [key.x, key.z], center_xz)
            })
            .collect::<Vec<_>>();
        for column in stale {
            self.evict_column(column);
        }
    }
    /// The local player's current chunk column, floored from the resolved
    /// server-authoritative position so negative coordinates land in the
    /// correct column.
    fn player_chunk(&self) -> ChunkKey {
        let position = self.resolved_server_position.position;
        ChunkKey::new(
            self.current_dimension,
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

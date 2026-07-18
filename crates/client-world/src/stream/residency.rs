use super::*;

impl WorldStream {
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
        let previous_collision_revision = self.store.collision_revision(key);
        changed.extend(self.store.evict_chunk(key));
        self.observe_collision_revision_change(key, previous_collision_revision);
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
        columns.extend(self.loaded_columns.iter().copied());
        columns.extend(self.requested_sub_chunks.keys().copied());
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

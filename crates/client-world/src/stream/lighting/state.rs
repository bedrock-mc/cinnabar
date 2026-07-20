use super::super::*;

impl WorldStream {
    pub(in crate::stream) fn mark_light_changed_sources(
        &mut self,
        sources: impl IntoIterator<Item = SubChunkKey>,
    ) {
        let sources = sources.into_iter().collect::<BTreeSet<_>>();
        for key in &sources {
            if self.resident.contains(key) {
                self.next_block_generation = self.next_block_generation.wrapping_add(1).max(1);
                self.block_generations
                    .insert(*key, self.next_block_generation);
                let expected_kind = if self.known_air.contains(key) {
                    LightSubChunkKind::KnownAir
                } else if self.store.sub_chunk(*key).is_some() {
                    LightSubChunkKind::Resident
                } else {
                    LightSubChunkKind::Unknown
                };
                if expected_kind == LightSubChunkKind::Unknown {
                    self.remove_light_key(*key);
                    continue;
                }
                if self.light_store.kind(*key) != expected_kind {
                    let retained = self
                        .light_store
                        .light(*key)
                        .map_or_else(|| SubChunkLight::dark(0), |light| light.as_ref().clone());
                    match expected_kind {
                        LightSubChunkKind::KnownAir => {
                            self.light_store.insert_known_air(*key, retained);
                        }
                        LightSubChunkKind::Resident => {
                            self.light_store.insert_resident(*key, retained);
                        }
                        LightSubChunkKind::Unknown => unreachable!(),
                    }
                }
            } else {
                self.remove_light_key(*key);
            }
        }
        let dependents = sources
            .into_iter()
            .flat_map(SubChunkKey::mesh_dependents)
            .filter(|key| self.resident.contains(key))
            .collect::<BTreeSet<_>>();
        for dependent in dependents {
            self.mark_light_dirty_exact(dependent);
        }
    }
    pub(in crate::stream) fn remove_light_key(&mut self, key: SubChunkKey) {
        let invalidates_mesh_halo = self.light_store.light(key).is_some()
            || self.light_ownership.contains_key(&key)
            || self.direct_sky.contains_key(&key);
        if invalidates_mesh_halo {
            self.mark_mesh_neighbourhood_dirty(key, Instant::now());
        }
        self.block_generations.remove(&key);
        self.light_store.remove(key);
        self.light_ownership.remove(&key);
        self.direct_sky.remove(&key);
        self.light_failures.remove(&key);
        self.light_revisions.entries.remove(&key);
        self.pending_light.remove(&key);
        self.in_flight_light.remove(&key);
        self.remove_light_waiters_for(key);
    }
    pub(in crate::stream) fn remove_light_waiters_for(&mut self, key: SubChunkKey) {
        self.light_waiters.remove(&key);
        self.remove_light_waiter_target(key);
    }
    pub(in crate::stream) fn remove_light_waiter_target(&mut self, key: SubChunkKey) {
        self.light_waiters.retain(|_, waiters| {
            waiters.remove(&key);
            !waiters.is_empty()
        });
    }
    pub(in crate::stream) fn mark_light_dirty_exact(&mut self, key: SubChunkKey) -> Option<u64> {
        if !self.resident.contains(&key) || !self.block_generations.contains_key(&key) {
            return None;
        }
        self.light_failures.remove(&key);
        self.remove_light_waiter_target(key);
        let queued_at = Instant::now();
        let revision = self.light_revisions.mark_dirty(key, queued_at);
        self.pending_light.insert(
            key,
            PendingLight {
                revision,
                queued_at,
            },
        );
        Some(revision)
    }
    pub(in crate::stream) fn light_is_current(&self, key: SubChunkKey) -> bool {
        if !self.light_source_is_known(key) || self.light_revisions.dirty(key).is_some() {
            return false;
        }
        let Some(block_generation) = self.block_generations.get(&key).copied() else {
            return false;
        };
        let Some(ownership) = self.light_ownership.get(&key).copied() else {
            return false;
        };
        let expected_kind = if self.known_air.contains(&key) {
            LightSubChunkKind::KnownAir
        } else if self.store.sub_chunk(key).is_some() {
            LightSubChunkKind::Resident
        } else {
            LightSubChunkKind::Unknown
        };
        ownership.block_generation == block_generation
            && expected_kind != LightSubChunkKind::Unknown
            && self.light_store.kind(key) == expected_kind
            && self
                .light_store
                .light(key)
                .is_some_and(|light| light.generation() == ownership.light_revision)
            && self
                .direct_sky
                .get(&key)
                .is_some_and(|direct| direct.light_revision == ownership.light_revision)
    }
    pub(in crate::stream) fn light_source_is_known(&self, key: SubChunkKey) -> bool {
        self.resident.contains(&key)
            && (self.known_air.contains(&key) || self.store.sub_chunk(key).is_some())
    }
    pub(in crate::stream) fn mesh_light_halo(&self, center: SubChunkKey) -> Option<MeshLightHalo> {
        let mut slots = std::array::from_fn(|_| None);
        for dx in -1_i8..=1 {
            for dy in -1_i8..=1 {
                for dz in -1_i8..=1 {
                    let offset = [dx, dy, dz];
                    let Some(key) = center
                        .x
                        .checked_add(i32::from(dx))
                        .zip(center.y.checked_add(i32::from(dy)))
                        .zip(center.z.checked_add(i32::from(dz)))
                        .map(|((x, y), z)| SubChunkKey::new(center.dimension, x, y, z))
                    else {
                        continue;
                    };
                    if !self.light_source_is_known(key) {
                        continue;
                    }
                    if !self.light_is_current(key) {
                        return None;
                    }
                    let ownership = self.light_ownership.get(&key).copied()?;
                    let light = Arc::clone(self.light_store.light(key)?);
                    slots[mesh_offset_index(offset)] = Some(MeshLightSlot {
                        key,
                        block_generation: ownership.block_generation,
                        light_revision: ownership.light_revision,
                        light,
                    });
                }
            }
        }
        Some(MeshLightHalo {
            center: Some(center),
            slots,
        })
    }
    pub(in crate::stream) fn light_block_snapshot(&self, key: SubChunkKey) -> LightBlockSnapshot {
        let mut blocks = BTreeMap::new();
        for sample_key in key.mesh_dependents() {
            if !self.light_source_is_known(sample_key) {
                continue;
            }
            if self.known_air.contains(&sample_key) {
                blocks.insert(sample_key, SnapshotBlock::KnownAir);
            } else if let Some(sub_chunk) = self.store.sub_chunk(sample_key) {
                blocks.insert(sample_key, SnapshotBlock::Resident(sub_chunk));
            }
        }
        let profile = match key.dimension {
            0 => DimensionLightProfile::Overworld {
                direct_sky_down: true,
            },
            1 => DimensionLightProfile::Nether,
            _ => DimensionLightProfile::End,
        };
        let overworld_top_y = (key.dimension == 0)
            .then(|| vanilla_dimension_range(0))
            .flatten()
            .and_then(|range| {
                range
                    .base_sub_chunk_y
                    .checked_add(i32::try_from(range.sub_chunk_count).ok()?)?
                    .checked_mul(16)?
                    .checked_sub(1)
            });
        LightBlockSnapshot {
            dimension: key.dimension,
            blocks,
            classifier: self.classifier,
            network_id_mode: self.network_id_mode,
            runtime_assets: self.runtime_assets.clone(),
            resolved_light: HashMap::new(),
            profile,
            overworld_top_y,
        }
    }
    pub(in crate::stream) fn light_prior_snapshot(&self, key: SubChunkKey) -> LightPriorSnapshot {
        let keys = key.mesh_dependents().collect::<BTreeSet<_>>();
        let direct_sky = keys
            .iter()
            .filter_map(|sample_key| {
                self.direct_sky
                    .get(sample_key)
                    .cloned()
                    .map(|direct| (*sample_key, direct))
            })
            .collect();
        let trusted_boundaries = keys
            .iter()
            .copied()
            .filter(|sample_key| *sample_key != key && self.light_is_current(*sample_key))
            .collect();
        LightPriorSnapshot {
            light: self.light_store.snapshot_keys(keys),
            direct_sky,
            trusted_boundaries,
        }
    }
    pub(in crate::stream) fn register_untrusted_light_waiters(&mut self, target: SubChunkKey) {
        for neighbour in target.mesh_dependents().filter(|key| *key != target) {
            if self.light_source_is_known(neighbour)
                && !self.light_is_current(neighbour)
                && self.light_store.light(neighbour).is_some()
                && self.prior_light_may_seed(target, neighbour)
            {
                self.light_waiters
                    .entry(neighbour)
                    .or_default()
                    .insert(target);
            }
        }
    }
    pub(in crate::stream) fn prior_light_may_seed(
        &self,
        target: SubChunkKey,
        neighbour: SubChunkKey,
    ) -> bool {
        let Some(light) = self.light_store.light(neighbour) else {
            return false;
        };
        let block_may_seed = !light.channel(LightChannel::Block).is_uniform()
            || light.get(LightChannel::Block, 0, 0, 0) != Some(0);
        if block_may_seed {
            return true;
        }
        if target.dimension != 0 || self.known_air_has_vertical_direct_sky(target) {
            return false;
        }
        !light.channel(LightChannel::Sky).is_uniform()
            || light.get(LightChannel::Sky, 0, 0, 0) != Some(0)
    }
    pub(in crate::stream) fn light_dispatch_ready(&self, key: SubChunkKey) -> bool {
        if key.dimension != 0 {
            return true;
        }
        let Some(above) = offset_sub_chunk_key(key, [0, 1, 0]) else {
            return true;
        };
        !self.light_source_is_known(above) || self.light_is_current(above)
    }
}

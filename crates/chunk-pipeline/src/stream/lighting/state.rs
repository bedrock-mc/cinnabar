use super::super::*;

/// Light indexes, job scheduling and solver plumbing for resident sub-chunks.
pub(in crate::stream) struct Lighting {
    pub(in crate::stream) tx: Sender<LightCompletion>,
    pub(in crate::stream) rx: Receiver<LightCompletion>,
    /// Solves still executing, including ones whose keys were evicted meanwhile.
    pub(in crate::stream) running_jobs: Arc<AtomicUsize>,
    pub(in crate::stream) next_batch_id: u64,
    pub(in crate::stream) next_block_generation: u64,
    pub(in crate::stream) fatal_failure: bool,
    pub(in crate::stream) revisions: RevisionTracker,
    pub(in crate::stream) block_generations: HashMap<SubChunkKey, u64>,
    pub(in crate::stream) store: LightStore,
    pub(in crate::stream) ownership: HashMap<SubChunkKey, LightOwnership>,
    pub(in crate::stream) direct_sky: BTreeMap<SubChunkKey, StoredDirectSky>,
    pub(in crate::stream) failures: HashMap<SubChunkKey, LightFailure>,
    pub(in crate::stream) jobs: scheduler::KeyedJobs<PendingLight, LightJobIdentity, 1>,
    pub(in crate::stream) priority_wakeups: HashMap<SubChunkKey, u64>,
    pub(in crate::stream) in_flight_batches: HashMap<u64, usize>,
    pub(in crate::stream) last_dispatched_batch: HashMap<SubChunkKey, u64>,
    pub(in crate::stream) waiters: HashMap<SubChunkKey, BTreeSet<SubChunkKey>>,
}

impl Lighting {
    pub(in crate::stream) fn new() -> Self {
        let (tx, rx) = bounded(LIGHT_RESULT_CAPACITY);
        Self {
            tx,
            rx,
            running_jobs: Arc::new(AtomicUsize::new(0)),
            next_batch_id: 0,
            next_block_generation: 0,
            fatal_failure: false,
            revisions: RevisionTracker::default(),
            block_generations: HashMap::new(),
            store: LightStore::default(),
            ownership: HashMap::new(),
            direct_sky: BTreeMap::new(),
            failures: HashMap::new(),
            jobs: Default::default(),
            priority_wakeups: HashMap::new(),
            in_flight_batches: HashMap::new(),
            last_dispatched_batch: HashMap::new(),
            waiters: HashMap::new(),
        }
    }

    /// Releases every per-key index off the frame thread, as a disjoint retirement leaves no
    /// retained neighbours; plumbing, counters, running solves and the fatal latch survive.
    pub(in crate::stream) fn retire_all(&mut self) {
        let Self {
            tx: _,
            rx: _,
            running_jobs: _,
            next_batch_id: _,
            next_block_generation: _,
            fatal_failure: _,
            revisions,
            block_generations,
            store,
            ownership,
            direct_sky,
            failures,
            jobs,
            priority_wakeups,
            in_flight_batches,
            last_dispatched_batch,
            waiters,
        } = self;
        let retired = (
            std::mem::take(&mut revisions.entries),
            std::mem::take(block_generations),
            std::mem::take(store),
            std::mem::take(ownership),
            std::mem::take(direct_sky),
            std::mem::take(failures),
            std::mem::take(jobs),
            std::mem::take(priority_wakeups),
            std::mem::take(in_flight_batches),
            std::mem::take(last_dispatched_batch),
            std::mem::take(waiters),
        );
        rayon::spawn(move || drop(retired));
    }

    /// Forgets one source in every index without invalidating dependent meshes; stale scan
    /// and lane entries fall away against the pending map.
    pub(in crate::stream) fn remove_key(&mut self, key: SubChunkKey) {
        self.block_generations.remove(&key);
        self.store.remove(key);
        self.ownership.remove(&key);
        self.direct_sky.remove(&key);
        self.failures.remove(&key);
        self.revisions.entries.remove(&key);
        self.jobs.pending.remove(&key);
        self.priority_wakeups.remove(&key);
        self.remove_in_flight(key, None);
        self.last_dispatched_batch.remove(&key);
        self.remove_waiters_for(key);
    }

    /// Drops queued work and wake-up edges after a fatal solve; running jobs drain normally.
    pub(in crate::stream) fn clear_after_fatal(&mut self) {
        self.jobs.clear_queued();
        self.priority_wakeups.clear();
        self.waiters.clear();
    }

    pub(in crate::stream) fn remove_in_flight(
        &mut self,
        key: SubChunkKey,
        expected: Option<LightJobIdentity>,
    ) -> bool {
        let Some(identity) = self.jobs.in_flight.get(&key).copied() else {
            return false;
        };
        if expected.is_some_and(|expected| expected != identity) {
            return false;
        }
        self.jobs.in_flight.remove(&key);
        if let std::collections::hash_map::Entry::Occupied(mut entry) =
            self.in_flight_batches.entry(identity.batch_id)
        {
            if *entry.get() <= 1 {
                entry.remove();
            } else {
                *entry.get_mut() -= 1;
            }
        }
        true
    }

    pub(in crate::stream) fn remove_waiters_for(&mut self, key: SubChunkKey) {
        self.waiters.remove(&key);
        self.remove_waiter_target(key);
    }

    pub(in crate::stream) fn remove_waiter_target(&mut self, key: SubChunkKey) -> usize {
        // Waiter edges are registered only for face-adjacent light dependencies:
        // the upper skylight dependency and `register_untrusted_light_waiters`.
        // Therefore `key` can occur only in a face neighbour's waiter set.
        let mut probes = 0;
        for source in key.mesh_dependents().filter(|source| *source != key) {
            probes += 1;
            if let std::collections::hash_map::Entry::Occupied(mut entry) =
                self.waiters.entry(source)
            {
                entry.get_mut().remove(&key);
                if entry.get().is_empty() {
                    entry.remove();
                }
            }
        }
        probes
    }
}

impl WorldStream {
    pub(in crate::stream) fn block_light_semantics_changed(
        &self,
        previous: Option<&SubChunk>,
        replacement: Option<&SubChunk>,
    ) -> bool {
        Self::light_semantics_changed(
            self.classifier,
            self.authority.runtime_assets(),
            self.authority.network_id_mode(),
            previous,
            replacement,
        )
    }

    /// Compares the exact block-light inputs against an immutable registry snapshot.
    pub(in crate::stream) fn light_semantics_changed(
        classifier: BlockClassifier,
        assets: &RuntimeAssets,
        mode: NetworkIdMode,
        previous: Option<&SubChunk>,
        replacement: Option<&SubChunk>,
    ) -> bool {
        client_world::ingestion::light_semantics_changed(
            classifier.air_network_id(),
            assets,
            mode,
            previous,
            replacement,
        )
    }

    #[cfg(test)]
    pub(in crate::stream) fn mark_light_changed_sources(
        &mut self,
        sources: impl IntoIterator<Item = SubChunkKey>,
    ) {
        self.mark_light_changed_sources_with_priority(sources, false);
    }
    pub(in crate::stream) fn mark_light_changed_sources_with_priority(
        &mut self,
        sources: impl IntoIterator<Item = SubChunkKey>,
        urgent: bool,
    ) {
        let sources = sources.into_iter().collect::<BTreeSet<_>>();
        for key in &sources {
            if self.resident.contains(key) {
                self.lighting.next_block_generation =
                    self.lighting.next_block_generation.wrapping_add(1).max(1);
                self.lighting
                    .block_generations
                    .insert(*key, self.lighting.next_block_generation);
                let expected_kind = if self.known_air.contains(key) {
                    LightSubChunkKind::KnownAir
                } else if self.authority.terrain().sub_chunk(*key).is_some() {
                    LightSubChunkKind::Resident
                } else {
                    LightSubChunkKind::Unknown
                };
                if expected_kind == LightSubChunkKind::Unknown {
                    self.remove_light_key(*key);
                    continue;
                }
                if self.lighting.store.kind(*key) != expected_kind {
                    let retained = self
                        .lighting
                        .store
                        .light(*key)
                        .map_or_else(|| SubChunkLight::dark(0), |light| light.as_ref().clone());
                    match expected_kind {
                        LightSubChunkKind::KnownAir => {
                            self.lighting.store.insert_known_air(*key, retained);
                        }
                        LightSubChunkKind::Resident => {
                            self.lighting.store.insert_resident(*key, retained);
                        }
                        LightSubChunkKind::Unknown => unreachable!(),
                    }
                }
            } else {
                self.remove_light_key(*key);
            }
        }
        let dependents = sources
            .iter()
            .copied()
            .flat_map(SubChunkKey::mesh_dependents)
            .filter(|key| self.resident.contains(key))
            .filter(|key| !self.air_fixed_point_survives_sources(*key, &sources))
            .collect::<BTreeSet<_>>();
        for dependent in dependents {
            self.mark_light_dirty_exact_with_priority(dependent, urgent);
        }
    }
    pub(in crate::stream) fn remove_light_key(&mut self, key: SubChunkKey) {
        let invalidates_mesh_halo = self.lighting.store.light(key).is_some()
            || self.lighting.ownership.contains_key(&key)
            || self.lighting.direct_sky.contains_key(&key);
        if invalidates_mesh_halo {
            self.mark_mesh_neighbourhood_dirty(key, Instant::now());
        }
        self.lighting.remove_key(key);
    }

    #[cfg(test)]
    pub(in crate::stream) fn mark_light_dirty_exact(&mut self, key: SubChunkKey) -> Option<u64> {
        self.mark_light_dirty_exact_with_priority(key, false)
    }
    pub(in crate::stream) fn mark_light_dirty_exact_with_priority(
        &mut self,
        key: SubChunkKey,
        urgent: bool,
    ) -> Option<u64> {
        if !self.resident.contains(&key) || !self.lighting.block_generations.contains_key(&key) {
            return None;
        }
        self.lighting.failures.remove(&key);
        // Pending jobs capture their inputs at dispatch, so later changes share one successor.
        if let Some(pending) = self.lighting.jobs.pending.get(&key).copied()
            && self.lighting.revisions.is_current(key, pending.revision)
        {
            let urgent = urgent
                || self
                    .lighting
                    .jobs
                    .in_flight
                    .get(&key)
                    .is_some_and(|identity| identity.urgent);
            if urgent && !pending.urgent {
                self.lighting.jobs.pending.get_mut(&key).unwrap().urgent = true;
                self.lighting.jobs.rescan(key, pending.revision, true);
                self.lighting.priority_wakeups.insert(key, pending.revision);
            }
            return Some(pending.revision);
        }
        self.lighting.priority_wakeups.remove(&key);
        self.lighting.remove_waiter_target(key);
        let urgent = urgent
            || self
                .lighting
                .jobs
                .pending
                .get(&key)
                .is_some_and(|pending| pending.urgent)
            || self
                .lighting
                .jobs
                .in_flight
                .get(&key)
                .is_some_and(|identity| identity.urgent);
        let queued_at = Instant::now();
        let revision = self.lighting.revisions.mark_dirty(key, queued_at);
        let startup = self.is_startup_dependency(key);
        self.lighting.jobs.enqueue_prioritized(
            key,
            PendingLight {
                revision,
                queued_at,
                urgent,
            },
            startup,
        );
        Some(revision)
    }
    pub(in crate::stream) fn light_is_current(&self, key: SubChunkKey) -> bool {
        if !self.light_source_is_known(key) || self.lighting.revisions.dirty(key).is_some() {
            return false;
        }
        let Some(block_generation) = self.lighting.block_generations.get(&key).copied() else {
            return false;
        };
        let Some(ownership) = self.lighting.ownership.get(&key).copied() else {
            return false;
        };
        let expected_kind = if self.known_air.contains(&key) {
            LightSubChunkKind::KnownAir
        } else if self.authority.terrain().contains_sub_chunk(key) {
            LightSubChunkKind::Resident
        } else {
            LightSubChunkKind::Unknown
        };
        ownership.block_generation == block_generation
            && expected_kind != LightSubChunkKind::Unknown
            && self.lighting.store.kind(key) == expected_kind
            && self
                .lighting
                .store
                .light(key)
                .is_some_and(|light| light.generation() == ownership.light_revision)
            && self
                .lighting
                .direct_sky
                .get(&key)
                .is_some_and(|direct| direct.light_revision == ownership.light_revision)
    }
    pub(in crate::stream) fn light_source_is_known(&self, key: SubChunkKey) -> bool {
        self.resident.contains(&key)
            && (self.known_air.contains(&key) || self.authority.terrain().contains_sub_chunk(key))
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
                    let ownership = self.lighting.ownership.get(&key).copied()?;
                    let light = Arc::clone(self.lighting.store.light(key)?);
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
        let mut blocks = SectionSnapshot::default();
        for sample_key in key.mesh_dependents() {
            if !self.light_source_is_known(sample_key) {
                continue;
            }
            if self.known_air.contains(&sample_key) {
                blocks.insert(sample_key, SnapshotBlock::KnownAir);
            } else if let Some(sub_chunk) = self.authority.terrain().sub_chunk(sample_key) {
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
            .then(|| self.light_column_top_sub_chunk_y(key))
            .flatten()
            .and_then(|y| y.checked_mul(16)?.checked_add(15));
        LightBlockSnapshot {
            dimension: key.dimension,
            blocks,
            classifier: self.classifier,
            network_id_mode: self.authority.network_id_mode(),
            runtime_assets: self.authority.runtime_assets().clone(),
            resolved_light: HashMap::new(),
            profile,
            overworld_top_y,
        }
    }
    pub(in crate::stream) fn light_prior_snapshot(&self, key: SubChunkKey) -> LightPriorSnapshot {
        let keys = || key.mesh_dependents();
        let direct_sky = keys()
            .filter_map(|sample_key| {
                self.lighting
                    .direct_sky
                    .get(&sample_key)
                    .cloned()
                    .map(|direct| (sample_key, direct))
            })
            .collect();
        let trusted_boundaries = keys()
            .filter(|sample_key| *sample_key != key && self.light_is_current(*sample_key))
            .map(|key| (key, ()))
            .collect();
        LightPriorSnapshot {
            light: self.lighting.store.snapshot_keys(keys()),

            direct_sky,
            trusted_boundaries,
        }
    }
    pub(in crate::stream) fn original_light_column_context_ready(&self, key: SubChunkKey) -> bool {
        let center = key.chunk();
        if !self.publisher.required_columns.contains(&center) {
            return true;
        }
        for dx in -1_i32..=1 {
            for dz in -1_i32..=1 {
                let Some(x) = center.x.checked_add(dx) else {
                    continue;
                };
                let Some(z) = center.z.checked_add(dz) else {
                    continue;
                };
                let neighbour = ChunkKey::new(center.dimension, x, z);
                // Requests that ended without data still settle the column: its empty slots are air.
                let settled = self.loaded_columns.contains(&neighbour)
                    || (self.requests.collision_failures.contains(&neighbour)
                        && !self.requests.requested.contains_key(&neighbour));
                if self.publisher.required_columns.contains(&neighbour) && !settled {
                    return false;
                }
            }
        }
        true
    }
    pub(in crate::stream) fn register_untrusted_light_waiters(
        &mut self,
        target: SubChunkKey,
        retained_batch: &HashSet<SubChunkKey>,
    ) {
        for neighbour in target.mesh_dependents().filter(|key| *key != target) {
            if !retained_batch.contains(&neighbour)
                && self.light_source_is_known(neighbour)
                && !self.light_is_current(neighbour)
                && self.lighting.store.light(neighbour).is_some()
                && self.prior_light_may_seed(target, neighbour)
            {
                self.lighting
                    .waiters
                    .entry(neighbour)
                    .or_default()
                    .insert(target);
            }
        }
    }
    pub(in crate::stream) fn highest_pending_light_in_column(
        &self,
        key: SubChunkKey,
    ) -> Option<(SubChunkKey, PendingLight)> {
        self.light_column_sources(key)
            .filter_map(|candidate| {
                self.lighting
                    .jobs
                    .pending
                    .get(&candidate)
                    .copied()
                    .map(|pending| (candidate, pending))
            })
            .max_by_key(|(candidate, _)| candidate.y)
    }

    /// Iterates loaded sources in one column without visiting unrelated sections.
    pub(in crate::stream) fn light_column_sources(
        &self,
        key: SubChunkKey,
    ) -> impl Iterator<Item = SubChunkKey> + '_ {
        self.resident.column(key.chunk()).copied()
    }

    /// Extends the admitted sky ceiling to include taller loaded columns.
    pub(in crate::stream) fn light_column_top_sub_chunk_y(&self, key: SubChunkKey) -> Option<i32> {
        let declared_top = self
            .authority
            .dimension_range(key.dimension)
            .and_then(|range| {
                range
                    .base_sub_chunk_y
                    .checked_add(i32::try_from(range.sub_chunk_count).ok()?)?
                    .checked_sub(1)
            });
        declared_top
            .into_iter()
            .chain(self.light_column_sources(key).map(|source| source.y))
            .max()
    }

    pub(in crate::stream) fn prior_light_may_seed(
        &self,
        target: SubChunkKey,
        neighbour: SubChunkKey,
    ) -> bool {
        let Some(light) = self.lighting.store.light(neighbour) else {
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
        if self.light_source_is_known(above) {
            self.light_is_current(above)
        } else {
            !self.requests.is_expected(above)
        }
    }
}

use super::super::*;

impl WorldStream {
    pub(in crate::stream) fn dispatch_mesh_jobs(
        &mut self,
        camera_position: [f32; 3],
        budget: usize,
    ) -> usize {
        let worker_budget = budget.min(WORK_RESULT_CAPACITY.saturating_sub(self.in_flight.len()));
        let mut candidates = self
            .pending_mesh
            .iter()
            .map(|(&key, &pending)| {
                (
                    distance_squared(key, camera_position),
                    key,
                    pending.revision,
                    pending.since,
                    pending.queued_at,
                )
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
        });

        let mut dispatched = 0;
        for (_, key, revision, dirty_since, queued_at) in candidates {
            if self.mesh_changes.len() >= MAX_PENDING_MESH_CHANGES {
                break;
            }
            if !self.revisions.is_current(key, revision) {
                continue;
            }
            let Some(center) = self.store.sub_chunk(key) else {
                self.pending_mesh.remove(&key);
                if self.known_air.contains(&key) {
                    self.set_connectivity(key, Some(FaceConnectivity::all()));
                    let registered = self.register_mesh_dependency_mask(
                        key,
                        revision,
                        MeshDependencyMask::default(),
                    );
                    debug_assert!(registered);
                } else {
                    self.set_connectivity(key, None);
                    self.mesh_dependency_masks.remove(&key);
                }
                self.mesh_changes.push_back(WorldMeshChange::Remove {
                    key,
                    generation: revision,
                    dirty_since,
                });
                continue;
            };
            if dispatched >= worker_budget || self.in_flight.contains_key(&key) {
                continue;
            }
            let Some(light_halo) = self.mesh_light_halo(key) else {
                continue;
            };
            let snapshot = self.mesh_snapshot(key, center, light_halo);
            self.pending_mesh.remove(&key);
            self.in_flight.insert(key, revision);
            let tx = self.mesh_tx.clone();
            let classifier = self.classifier;
            let network_id_mode = self.network_id_mode;
            let runtime_assets = Arc::clone(&self.runtime_assets);
            let resolved_biome_tints = Arc::clone(&self.resolved_biome_tints);
            let tint_identity = self.biome_tint_identity();
            rayon::spawn(move || {
                let started = Instant::now();
                let queue_wait = queue_wait(queued_at, started);
                let source = Arc::clone(&snapshot.center);
                let biome_sources = snapshot.biomes.clone();
                let light_halo = snapshot.light_halo.clone();
                let biome = pack_biome_record(&biome_sources, &resolved_biome_tints);
                let mesh = snapshot.mesh(classifier, &runtime_assets, network_id_mode);
                let dependency_mask =
                    snapshot.dependency_mask(classifier, &runtime_assets, network_id_mode);
                let _ = tx.send(MeshCompletion {
                    key,
                    revision,
                    source,
                    biome_sources,
                    biome,
                    tint_identity,
                    mesh,
                    dependency_mask,
                    light_halo,
                    queue_wait,
                    duration: started.elapsed(),
                });
            });
            self.stats.last_mesh_dispatch_at = Some(Instant::now());
            dispatched += 1;
        }
        dispatched
    }
    pub(in crate::stream) fn mesh_snapshot(
        &self,
        key: SubChunkKey,
        center: Arc<SubChunk>,
        light_halo: MeshLightHalo,
    ) -> MeshSnapshot {
        let mut adjacent = std::array::from_fn(|_| None);
        for offset @ [dx, dy, dz] in MeshNeighbourhood::adjacent_offsets() {
            let neighbour = key
                .x
                .checked_add(i32::from(dx))
                .zip(key.y.checked_add(i32::from(dy)))
                .zip(key.z.checked_add(i32::from(dz)))
                .and_then(|((x, y), z)| {
                    self.store
                        .sub_chunk(SubChunkKey::new(key.dimension, x, y, z))
                });
            adjacent[mesh_offset_index(offset)] = neighbour;
        }
        MeshSnapshot {
            center,
            biomes: self.biome_neighbourhood(key),
            adjacent,
            light_halo,
        }
    }
    pub(in crate::stream) fn biome_neighbourhood(&self, key: SubChunkKey) -> BiomeNeighbourhood {
        let mut biomes = std::array::from_fn(|_| None);
        for dz in -1_i8..=1 {
            for dx in -1_i8..=1 {
                let Some(x) = key.x.checked_add(i32::from(dx)) else {
                    continue;
                };
                let Some(z) = key.z.checked_add(i32::from(dz)) else {
                    continue;
                };
                let slot = biome_neighbour_index(dx, dz)
                    .expect("bounded biome-neighbour offsets have descriptor slots");
                biomes[slot] =
                    self.store
                        .biome_storage(SubChunkKey::new(key.dimension, x, key.y, z));
            }
        }
        biomes
    }
    pub(in crate::stream) fn mesh_light_halo_is_current(&self, halo: &MeshLightHalo) -> bool {
        let Some(center) = halo.center else {
            return halo.slots.iter().all(Option::is_none);
        };
        for dx in -1_i8..=1 {
            for dy in -1_i8..=1 {
                for dz in -1_i8..=1 {
                    let offset = [dx, dy, dz];
                    let key =
                        offset_sub_chunk_key(center, [i32::from(dx), i32::from(dy), i32::from(dz)]);
                    let slot = halo.slots[mesh_offset_index(offset)].as_ref();
                    match (key, slot) {
                        (Some(key), None) if self.light_source_is_known(key) => return false,
                        (None, Some(_)) => return false,
                        (Some(key), Some(slot)) if !self.mesh_light_slot_is_current(key, slot) => {
                            return false;
                        }
                        _ => {}
                    }
                }
            }
        }
        true
    }
    pub(in crate::stream) fn mesh_light_slot_is_current(
        &self,
        key: SubChunkKey,
        slot: &MeshLightSlot,
    ) -> bool {
        slot.key == key
            && self.light_is_current(key)
            && self.block_generations.get(&key).copied() == Some(slot.block_generation)
            && self.light_ownership.get(&key).is_some_and(|ownership| {
                ownership.block_generation == slot.block_generation
                    && ownership.light_revision == slot.light_revision
            })
            && self
                .light_store
                .light(key)
                .is_some_and(|light| Arc::ptr_eq(light, &slot.light))
    }
    pub(in crate::stream) fn requeue_current_mesh_completion(
        &mut self,
        key: SubChunkKey,
        revision: u64,
    ) {
        let Some(dirty) = self
            .revisions
            .dirty(key)
            .filter(|dirty| dirty.revision == revision)
        else {
            return;
        };
        self.pending_mesh.entry(key).or_insert(PendingMesh {
            revision,
            since: dirty.since,
            queued_at: Instant::now(),
        });
    }
    pub(in crate::stream) fn accept_mesh_completion(&mut self, completion: MeshCompletion) {
        self.stats.observe_mesh_queue_wait(completion.queue_wait);
        if self.in_flight.get(&completion.key) == Some(&completion.revision) {
            self.in_flight.remove(&completion.key);
        }
        let source_is_current = self
            .store
            .sub_chunk(completion.key)
            .is_some_and(|current| Arc::ptr_eq(&current, &completion.source));
        let current_biomes = self.biome_neighbourhood(completion.key);
        let biome_sources_are_current = completion.biome_sources.iter().zip(&current_biomes).all(
            |(completed, current)| match (completed, current) {
                (Some(completed), Some(current)) => Arc::ptr_eq(completed, current),
                (None, None) => true,
                _ => false,
            },
        );
        if !self
            .revisions
            .is_current(completion.key, completion.revision)
            || !source_is_current
            || !biome_sources_are_current
            || completion.tint_identity != self.biome_tint_identity()
            || !self.mesh_light_halo_is_current(&completion.light_halo)
        {
            self.stats.stale_mesh_jobs = self.stats.stale_mesh_jobs.saturating_add(1);
            self.requeue_current_mesh_completion(completion.key, completion.revision);
            return;
        }
        self.stats.max_mesh_duration = self.stats.max_mesh_duration.max(completion.duration);
        self.stats.last_mesh_completion_at = Some(Instant::now());
        let dirty = self
            .revisions
            .dirty(completion.key)
            .expect("current mesh completion has a dirty revision");
        self.set_connectivity(completion.key, Some(completion.mesh.connectivity()));
        if self.resident.contains(&completion.key) {
            let registered = self.register_mesh_dependency_mask(
                completion.key,
                completion.revision,
                completion.dependency_mask,
            );
            debug_assert!(registered);
        }
        self.mesh_changes.push_back(WorldMeshChange::Upsert {
            key: completion.key,
            mesh: completion.mesh,
            biome: completion.biome,
            tint_identity: completion.tint_identity,
            generation: completion.revision,
            dirty_since: dirty.since,
        });
    }
}

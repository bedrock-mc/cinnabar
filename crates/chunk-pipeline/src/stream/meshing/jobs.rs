use super::super::*;

impl WorldStream {
    /// Keeps zero-byte removals independent of the initial geometry preparation limit.
    pub(in crate::stream) fn dispatch_mesh_jobs_with_limits(
        &mut self,
        camera_position: [f32; 3],
        budget: usize,
        removal_budget: usize,
    ) -> usize {
        if (budget == 0 && removal_budget == 0) || self.mesh_jobs.pending.is_empty() {
            return 0;
        }

        let view = SchedulerView {
            position: camera_position,
            forward: self.view_forward,
        };
        let (resident, known_air) = (&self.resident, &self.known_air);
        let probe_near = self
            .mesh_jobs
            .ingress(view, self.poll_deadline, |key, _, pending| {
                let lane = if resident.contains(&key) && !known_air.contains(&key) {
                    RESIDENT_MESH_LANE
                } else {
                    MESH_REMOVAL_LANE
                };
                (lane, pending.urgent)
            });

        let occupied = self.admitted_mesh_jobs.load(Ordering::Acquire);
        let worker_budget = budget.min(
            super::admission::mesh_job_cap(rayon::current_num_threads()).saturating_sub(occupied),
        );
        let mut resident_candidates = if probe_near {
            scheduler::near_camera_keys(view, self.authority.current_dimension())
                .filter_map(|key| {
                    let pending = self.mesh_jobs.pending.get(&key).copied()?;
                    (self.resident.contains(&key)
                        && !self.known_air.contains(&key)
                        && !self.mesh_jobs.in_flight.contains_key(&key)
                        && self.revisions.is_current(key, pending.revision))
                    .then_some((
                        PendingSchedulerCandidate::new(key, pending.revision, view, pending.urgent),
                        pending,
                        false,
                    ))
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        resident_candidates.sort_unstable_by(|left, right| right.0.cmp(&left.0));
        resident_candidates.truncate(MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
        let mut removal_candidates = Vec::new();
        for _ in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL {
            if worker_budget == 0 && self.poll_budget_exhausted() {
                break;
            }
            let Some(mut candidate) = self.mesh_jobs.lanes[RESIDENT_MESH_LANE].ready.pop() else {
                break;
            };
            let key = candidate.key;
            candidate.distance_squared = view.rank(key);
            let Some(pending) = self.mesh_jobs.pending.get(&key).copied() else {
                continue;
            };
            if pending.revision != candidate.revision {
                continue;
            }
            if !self.revisions.is_current(key, pending.revision)
                || self.mesh_jobs.in_flight.contains_key(&key)
            {
                self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                    .deferred
                    .push(candidate);
            } else if self.resident.contains(&key) && !self.known_air.contains(&key) {
                if let Some((_, _, queued)) = resident_candidates
                    .iter_mut()
                    .find(|(entry, _, _)| entry.key == key)
                {
                    *queued = true;
                } else {
                    resident_candidates.push((candidate, pending, true));
                }
            } else {
                self.mesh_jobs.lanes[MESH_REMOVAL_LANE]
                    .ready
                    .push(candidate);
            }
        }
        let removal_authority = self
            .publication_allowance
            .as_ref()
            .map_or(removal_budget, |allowance| {
                allowance.zero_byte_admission_capacity_with_priority(true)
            })
            .min(removal_budget)
            .min(MAX_PENDING_MESH_CHANGES.saturating_sub(self.mesh_changes.len()));
        if removal_authority != 0 {
            for index in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL {
                if index != 0 && self.poll_budget_exhausted() {
                    break;
                }
                let Some(candidate) = self.mesh_jobs.lanes[MESH_REMOVAL_LANE].deferred.pop() else {
                    break;
                };
                self.mesh_jobs.lanes[MESH_REMOVAL_LANE]
                    .ready
                    .push(candidate);
            }
        }
        for index in 0..MAX_PENDING_MESH_QUEUE_WORK_PER_POLL {
            if self.poll_budget_exhausted()
                && (!removal_candidates.is_empty() || index >= MAX_PENDING_SCHEDULER_SCANS_PER_POLL)
            {
                break;
            }
            let Some(candidate) = self.mesh_jobs.lanes[MESH_REMOVAL_LANE].ready.pop() else {
                break;
            };
            let key = candidate.key;
            let Some(pending) = self.mesh_jobs.pending.get(&key).copied() else {
                continue;
            };
            if pending.revision != candidate.revision {
                continue;
            }
            if !self.revisions.is_current(key, pending.revision) {
                self.mesh_jobs.lanes[MESH_REMOVAL_LANE]
                    .deferred
                    .push(candidate);
            } else if self.resident.contains(&key) && !self.known_air.contains(&key) {
                self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                    .ready
                    .push(candidate);
            } else if removal_candidates.len() >= removal_authority {
                self.mesh_jobs.lanes[MESH_REMOVAL_LANE]
                    .deferred
                    .push(candidate);
                break;
            } else {
                removal_candidates.push((candidate, pending));
            }
        }

        let mut dispatched = 0;
        let mut examined = false;
        let now = Instant::now();
        self.prioritize_transfer_mesh_candidates(&mut resident_candidates);
        resident_candidates.sort_unstable_by(|left, right| right.0.cmp(&left.0));
        for (candidate, pending, queued) in resident_candidates {
            let key = candidate.key;
            if self.mesh_changes.len() >= MAX_PENDING_MESH_CHANGES
                || dispatched >= worker_budget
                || (examined && self.poll_budget_exhausted())
            {
                if queued {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .ready
                        .push(candidate);
                }
                continue;
            }
            if !self.revisions.is_current(key, pending.revision)
                || self.mesh_jobs.in_flight.contains_key(&key)
            {
                if queued {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .deferred
                        .push(candidate);
                }
                continue;
            }
            examined = true;
            if self.mesh_neighbour_is_due(key, now) {
                if queued {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .deferred
                        .push(candidate);
                }
                continue;
            }
            let Some(center) = self.authority.terrain().sub_chunk(key) else {
                if queued {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .deferred
                        .push(candidate);
                }
                continue;
            };
            let Some(light_halo) = self.mesh_light_halo(key) else {
                if queued {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .deferred
                        .push(candidate);
                }
                continue;
            };
            let Some(mut output_permit) = self.mesh_memory.try_admit(
                &center,
                self.authority.runtime_assets(),
                self.authority.network_id_mode(),
            ) else {
                if queued {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .deferred
                        .push(candidate);
                }
                continue;
            };
            let snapshot = self.mesh_snapshot(key, center, light_halo);
            self.mesh_jobs.pending.remove(&key);
            self.mesh_jobs.in_flight.insert(key, pending.revision);
            if pending.urgent {
                self.urgent_mesh_in_flight.insert(key);
            }
            let job_permit = super::admission::MeshJobPermit::new(&self.admitted_mesh_jobs);
            let cancelled = Arc::new(AtomicBool::new(false));
            self.mesh_cancellations.insert(key, Arc::clone(&cancelled));
            let tx = self.mesh_tx.clone();
            let classifier = self.classifier;
            let network_id_mode = self.authority.network_id_mode();
            let runtime_assets = Arc::clone(self.authority.runtime_assets());
            let resolved_biome_tints = Arc::clone(self.authority.resolved_biome_tints());
            let tint_identity = self.biome_tint_identity();
            workers::WORKERS.mesh.spawn(move || {
                let started = Instant::now();
                let queue_wait = queue_wait(pending.queued_at, started);
                let source = Arc::clone(&snapshot.center);
                let biome_sources = snapshot.biomes.clone();
                let light_halo = snapshot.light_halo.clone();
                let biome = pack_biome_record(&biome_sources, &resolved_biome_tints);
                let mesh = if cancelled.load(Ordering::Acquire) {
                    ChunkMesh::default()
                } else {
                    snapshot.mesh(classifier, &runtime_assets, network_id_mode)
                };
                let dependency_mask = if cancelled.load(Ordering::Acquire) {
                    MeshDependencyMask::default()
                } else {
                    snapshot.dependency_mask(classifier, &runtime_assets, network_id_mode)
                };
                output_permit.reconcile(&mesh, &biome);
                let _ = tx.send(MeshCompletion {
                    output_permit: Some(output_permit),
                    _job_permit: Some(job_permit),
                    key,
                    revision: pending.revision,
                    source,
                    biome_sources,
                    biome,
                    tint_identity,
                    mesh,
                    dependency_mask,
                    light_halo,
                    queue_wait,
                    duration: started.elapsed(),
                    urgent: pending.urgent,
                });
            });
            self.stats.last_mesh_dispatch_at = Some(Instant::now());
            self.stats.phase2_stages.mesh_jobs_dispatched = self
                .stats
                .phase2_stages
                .mesh_jobs_dispatched
                .saturating_add(1);
            dispatched += 1;
        }

        let mut removal_candidates = removal_candidates.into_iter();
        let mut removed = false;
        while let Some((candidate, pending)) = removal_candidates.next() {
            if (removed || dispatched != 0) && self.poll_budget_exhausted() {
                self.mesh_jobs.lanes[MESH_REMOVAL_LANE]
                    .deferred
                    .push(candidate);
                self.mesh_jobs.lanes[MESH_REMOVAL_LANE]
                    .deferred
                    .extend(removal_candidates.map(|(candidate, _)| candidate));
                break;
            }
            let key = candidate.key;
            if !self.revisions.is_current(key, pending.revision) {
                self.mesh_jobs.lanes[MESH_REMOVAL_LANE]
                    .deferred
                    .push(candidate);
                continue;
            }
            let permit = match &self.publication_allowance {
                Some(allowance) => {
                    let Some(permit) = allowance.try_admit_zero_byte_with_priority(pending.urgent)
                    else {
                        self.mesh_jobs.lanes[MESH_REMOVAL_LANE]
                            .deferred
                            .push(candidate);
                        self.mesh_jobs.lanes[MESH_REMOVAL_LANE]
                            .deferred
                            .extend(removal_candidates.map(|(candidate, _)| candidate));
                        break;
                    };
                    Some(permit)
                }
                None => None,
            };
            self.mesh_jobs.pending.remove(&key);
            removed = true;
            if self.known_air.contains(&key) {
                self.set_connectivity(key, Some(FaceConnectivity::all()));
                let registered = self.register_mesh_dependency_mask(
                    key,
                    pending.revision,
                    MeshDependencyMask::default(),
                );
                debug_assert!(registered);
            } else {
                self.set_connectivity(key, None);
                self.mesh_dependency_masks.remove(&key);
            }
            let change = WorldMeshChange::Remove {
                key,
                generation: pending.revision,
                dirty_since: pending.since,
                urgent: pending.urgent,
                permit,
            };
            if pending.urgent {
                self.mesh_changes.push_front(change);
            } else {
                self.mesh_changes.push_back(change);
            }
            self.stats.phase2_stages.mesh_changes_queued = self
                .stats
                .phase2_stages
                .mesh_changes_queued
                .saturating_add(1);
        }
        dispatched
    }
    /// Faces, AO and smooth light sample all 26 neighbours; meshing before one the server still
    /// owes arrives bakes a hole or dark corner, so the mesh waits for it.
    pub(in crate::stream) fn mesh_neighbour_is_due(
        &mut self,
        key: SubChunkKey,
        now: Instant,
    ) -> bool {
        let mut due = false;
        for neighbour in key
            .mesh_neighbourhood_dependents()
            .filter(|neighbour| *neighbour != key)
        {
            if self.sub_chunk_is_due(neighbour, now) {
                due = true;
                if self.requests.is_expected(neighbour) {
                    self.requests
                        .queue
                        .prioritize_mesh_blocker(neighbour.chunk());
                }
            }
        }
        due
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
                    self.authority
                        .terrain()
                        .sub_chunk(SubChunkKey::new(key.dimension, x, y, z))
                });
            adjacent[mesh_offset_index(offset)] = neighbour;
        }
        MeshSnapshot {
            center,
            biomes: self.biome_neighbourhood(key),
            adjacent,
            column_above: self
                .authority
                .terrain()
                .chunk(key.chunk())
                .into_iter()
                .flat_map(|chunk| chunk.sub_chunks())
                .filter_map(|(y, chunk)| {
                    y.checked_sub(key.y)
                        .filter(|&offset| offset >= 2)
                        .map(|offset| (offset, chunk))
                })
                .collect(),
            light_halo,
        }
    }
    pub(in crate::stream) fn biome_neighbourhood(&self, key: SubChunkKey) -> BiomeNeighbourhood {
        let mut biomes = std::array::from_fn(|_| None);
        for dy in -1_i8..=1 {
            for dz in -1_i8..=1 {
                for dx in -1_i8..=1 {
                    let Some(x) = key.x.checked_add(i32::from(dx)) else {
                        continue;
                    };
                    let Some(y) = key.y.checked_add(i32::from(dy)) else {
                        continue;
                    };
                    let Some(z) = key.z.checked_add(i32::from(dz)) else {
                        continue;
                    };
                    let slot =
                        ::meshing::biome_volume_index(dx, dy, dz).expect("bounded biome halo");
                    biomes[slot] = self.authority.terrain().biome_storage(SubChunkKey::new(
                        key.dimension,
                        x,
                        y,
                        z,
                    ));
                }
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
            && self.lighting.block_generations.get(&key).copied() == Some(slot.block_generation)
            && self.lighting.ownership.get(&key).is_some_and(|ownership| {
                ownership.block_generation == slot.block_generation
                    && ownership.light_revision == slot.light_revision
            })
            && self
                .lighting
                .store
                .light(key)
                .is_some_and(|light| Arc::ptr_eq(light, &slot.light))
    }
    pub(in crate::stream) fn requeue_current_mesh_completion(
        &mut self,
        key: SubChunkKey,
        revision: u64,
        urgent: bool,
    ) {
        let Some(dirty) = self
            .revisions
            .dirty(key)
            .filter(|dirty| dirty.revision == revision)
        else {
            return;
        };
        if !self.mesh_jobs.pending.contains_key(&key) {
            self.mesh_jobs.enqueue(
                key,
                PendingMesh {
                    revision,
                    since: dirty.since,
                    queued_at: Instant::now(),
                    urgent,
                },
            );
        }
    }
    pub(in crate::stream) fn accept_mesh_completion(&mut self, mut completion: MeshCompletion) {
        completion._job_permit.take();
        self.stats.phase2_stages.mesh_jobs_completed = self
            .stats
            .phase2_stages
            .mesh_jobs_completed
            .saturating_add(1);
        self.stats.observe_mesh_queue_wait(completion.queue_wait);
        if self.mesh_jobs.in_flight.get(&completion.key) == Some(&completion.revision) {
            self.mesh_jobs.in_flight.remove(&completion.key);
            self.mesh_cancellations.remove(&completion.key);
            self.urgent_mesh_in_flight.remove(&completion.key);
        }
        if let Some(denied) = self.publish_mesh_completion(completion) {
            self.stage_denied_mesh_completion(denied);
        }
    }
    /// Retries completions that were denied a publication permit, in arrival order.
    pub(in crate::stream) fn retry_staged_mesh_completions(&mut self) {
        let count = self.staged_mesh_completions.len();
        for index in 0..count {
            if self.mesh_changes.len() >= MAX_PENDING_MESH_CHANGES
                || (index != 0 && self.poll_budget_exhausted())
            {
                break;
            }
            let Some(completion) = self.staged_mesh_completions.pop_front() else {
                break;
            };
            let bytes = chunk_publication_byte_len(&completion.mesh, &completion.biome);
            self.staged_mesh_bytes -= bytes;
            if let Some(denied) = self.publish_mesh_completion(completion) {
                self.staged_mesh_bytes += bytes;
                self.staged_mesh_completions.push_front(denied);
                break;
            }
        }
    }
    /// Keeps a current mesh for a later permit instead of meshing it again;
    /// past the staging bound it falls back to rescheduling.
    fn stage_denied_mesh_completion(&mut self, completion: MeshCompletion) {
        let bytes = chunk_publication_byte_len(&completion.mesh, &completion.biome);
        if self.staged_mesh_completions.len() >= MAX_STAGED_MESH_COMPLETIONS
            || self.staged_mesh_bytes.saturating_add(bytes) > MAX_STAGED_MESH_BYTES
        {
            self.requeue_current_mesh_completion(
                completion.key,
                completion.revision,
                completion.urgent,
            );
            return;
        }
        self.staged_mesh_bytes += bytes;
        if completion.urgent {
            self.staged_mesh_completions.push_front(completion);
        } else {
            self.staged_mesh_completions.push_back(completion);
        }
    }
    /// Publishes a current completion; returns it when no permit is available.
    fn publish_mesh_completion(&mut self, completion: MeshCompletion) -> Option<MeshCompletion> {
        let source_is_current = self
            .authority
            .terrain()
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
            self.requeue_current_mesh_completion(
                completion.key,
                completion.revision,
                completion.urgent,
            );
            return None;
        }
        self.stats.max_mesh_duration = self.stats.max_mesh_duration.max(completion.duration);
        self.stats.last_mesh_completion_at = Some(Instant::now());
        let dirty = self
            .revisions
            .dirty(completion.key)
            .expect("current mesh completion has a dirty revision");
        let publication_bytes = chunk_publication_byte_len(&completion.mesh, &completion.biome);
        let permit = match &self.publication_allowance {
            Some(allowance) if publication_bytes == 0 => {
                let Some(permit) = allowance.try_admit_zero_byte_with_priority(completion.urgent)
                else {
                    return Some(completion);
                };
                Some(permit)
            }
            Some(allowance) => {
                let Some(permit) = allowance.try_admit_payload(publication_bytes) else {
                    return Some(completion);
                };
                Some(permit)
            }
            None => None,
        };
        self.set_connectivity(completion.key, Some(completion.mesh.connectivity()));
        if self.resident.contains(&completion.key) {
            let registered = self.register_mesh_dependency_mask(
                completion.key,
                completion.revision,
                completion.dependency_mask,
            );
            debug_assert!(registered);
        }
        let urgent = completion.urgent;
        let change = WorldMeshChange::Upsert {
            output_permit: completion.output_permit,
            key: completion.key,
            mesh: completion.mesh,
            biome: completion.biome,
            tint_identity: completion.tint_identity,
            generation: completion.revision,
            dirty_since: dirty.since,
            urgent,
            permit,
        };
        if urgent {
            self.mesh_changes.push_front(change);
        } else {
            self.mesh_changes.push_back(change);
        }
        self.stats.phase2_stages.mesh_changes_queued = self
            .stats
            .phase2_stages
            .mesh_changes_queued
            .saturating_add(1);
        None
    }
}

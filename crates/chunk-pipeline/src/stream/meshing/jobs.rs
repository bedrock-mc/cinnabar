use super::super::residency::ColumnDue;
use super::super::*;

impl WorldStream {
    /// Keeps zero-byte removals independent of the initial geometry preparation limit.
    pub(in crate::stream) fn dispatch_mesh_jobs_with_limits(
        &mut self,
        camera_position: [f32; 3],
        budget: usize,
        removal_budget: usize,
    ) -> usize {
        self.dispatch_mesh_jobs_reserving(camera_position, budget, removal_budget, 0)
    }

    /// [`Self::dispatch_mesh_jobs_with_limits`], letting urgent meshes take up to
    /// `urgent_reserve` jobs beyond the worker cap, so a block the local player changed remeshes
    /// while streaming work fills every worker.
    pub(in crate::stream) fn dispatch_mesh_jobs_reserving(
        &mut self,
        camera_position: [f32; 3],
        budget: usize,
        removal_budget: usize,
        urgent_reserve: usize,
    ) -> usize {
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!(
            "mesh.dispatch",
            budget,
            pending = self.mesh_jobs.pending.len()
        )
        .entered();
        if (budget == 0 && removal_budget == 0) || self.mesh_jobs.pending.is_empty() {
            return 0;
        }

        let view = self.scheduler_view(camera_position);
        let (resident, known_air) = (&self.resident, &self.known_air);
        let terrain = self.authority.terrain();
        let classifier = self.classifier;
        let detail = &self.full_detail;
        let vertical = &self.vertical_detail;
        let needs_geometry = |key| {
            requires_geometry(
                detail, vertical, resident, known_air, terrain, classifier, key,
            )
        };
        let mut woken = Vec::new();
        let probe_near = self
            .mesh_jobs
            .ingress(view, self.poll_deadline, |key, _, pending| {
                let lane = if needs_geometry(key) {
                    RESIDENT_MESH_LANE
                } else {
                    MESH_REMOVAL_LANE
                };
                if lane == RESIDENT_MESH_LANE && scheduler::mesh_is_near(view, key) {
                    woken.push(key);
                }
                (lane, pending.urgent)
            });
        woken.append(&mut self.mesh_jobs.refresh.take_woken());

        let occupied = self.admitted_mesh_jobs.load(Ordering::Acquire);
        let mesh_worker_cap = super::admission::mesh_worker_cap();
        let regular_budget = budget.min(mesh_worker_cap.saturating_sub(occupied));
        let worker_budget = budget.min((mesh_worker_cap + urgent_reserve).saturating_sub(occupied));
        let probe = |key| {
            let pending = self.mesh_jobs.pending.get(&key).copied()?;
            (needs_geometry(key)
                && !self.mesh_jobs.in_flight.contains_key(&key)
                && self.revisions.is_current(key, pending.revision))
            .then_some((
                PendingSchedulerCandidate::new(key, pending.revision, view, pending.urgent),
                pending,
                MeshCandidateOrigin::Probe,
            ))
        };
        let mut resident_candidates = if probe_near {
            scheduler::near_camera_keys(view, self.authority.current_dimension())
                .filter_map(&probe)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        // Fresh and woken nearby keys compete beside the lanes without leaving them, so lane
        // rotation is unchanged and a blocked neighbour keeps its deferral.
        for key in woken {
            if !resident_candidates
                .iter()
                .any(|(candidate, _, _)| candidate.key == key)
                && let Some(candidate) = probe(key)
            {
                resident_candidates.push(candidate);
            }
        }
        if view.startup_center.is_none() {
            let already_probed = resident_candidates
                .iter()
                .map(|(candidate, _, _)| candidate.key)
                .collect();
            resident_candidates.extend(self.mesh_retry_candidates(view, &already_probed));
        }
        resident_candidates.sort_unstable_by_key(|candidate| std::cmp::Reverse(candidate.0));
        let truncated = resident_candidates.split_off(
            resident_candidates
                .len()
                .min(MAX_PENDING_SCHEDULER_SCANS_PER_POLL),
        );
        for (candidate, _, origin) in truncated {
            self.requeue_mesh_candidate(candidate, origin);
        }
        let mut dispatch = workers::WORKERS.batch(workers::Lane::Mesh);
        let mut dispatched = 0;
        let mut examined = false;
        let now = Instant::now();
        self.prioritize_transfer_mesh_candidates(&mut resident_candidates);
        resident_candidates.sort_unstable_by_key(|candidate| std::cmp::Reverse(candidate.0));
        let mut probed = resident_candidates.into_iter().peekable();
        let mut ready_pops = 0;
        let mut seen = HashSet::new();
        loop {
            // The ready lane yields in rank order beside the probed candidates, one pop at a
            // time, so work this call cannot dispatch stays queued rather than cycling through.
            let ready_first = ready_pops < MAX_PENDING_SCHEDULER_SCANS_PER_POLL
                && self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                    .ready
                    .peek()
                    .is_some_and(|ready| probed.peek().is_none_or(|(next, _, _)| ready > next));
            let (candidate, pending, origin) = if ready_first {
                ready_pops += 1;
                let mut candidate = self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                    .ready
                    .pop()
                    .expect("a peeked ready candidate");
                let key = candidate.key;
                self.air_requeued.remove(&key);
                let Some(pending) = self.mesh_jobs.pending.get(&key).copied() else {
                    continue;
                };
                if pending.revision != candidate.revision {
                    continue;
                }
                // A rank queued under an earlier view yields only if it still leads under this one.
                candidate.refresh_rank(view);
                if probed.peek().is_some_and(|(next, _, _)| *next > candidate)
                    || self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .ready
                        .peek()
                        .is_some_and(|next| *next > candidate)
                {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .ready
                        .push(candidate);
                    continue;
                }
                if !self.revisions.is_current(key, pending.revision) {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .deferred
                        .push(candidate);
                    continue;
                }
                // Air needs no worker, so its removal never waits for an older build in flight.
                if !self.mesh_requires_geometry(key) {
                    self.air_removals.insert(key, pending.revision);
                    continue;
                }
                if self.mesh_jobs.in_flight.contains_key(&key) || seen.contains(&key) {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .deferred
                        .push(candidate);
                    continue;
                }
                (candidate, pending, MeshCandidateOrigin::Queued)
            } else if let Some(next) = probed.next() {
                next
            } else {
                break;
            };
            let key = candidate.key;
            if !seen.insert(key) {
                if origin == MeshCandidateOrigin::Queued {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .deferred
                        .push(candidate);
                }
                continue;
            }
            let exhausted = self.mesh_changes.len() >= MAX_PENDING_MESH_CHANGES
                || dispatched >= worker_budget
                || (examined && self.poll_budget_exhausted());
            if exhausted || (dispatched >= regular_budget && !candidate.urgent) {
                self.requeue_mesh_candidate(candidate, origin);
                // Only a transfer candidate can rank above urgent work still to come.
                if !exhausted && candidate.transfer {
                    continue;
                }
                for (candidate, _, origin) in probed {
                    self.requeue_mesh_candidate(candidate, origin);
                }
                break;
            }
            if !self.revisions.is_current(key, pending.revision)
                || self.mesh_jobs.in_flight.contains_key(&key)
            {
                if origin == MeshCandidateOrigin::Queued {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .deferred
                        .push(candidate);
                }
                continue;
            }
            examined = true;
            self.mesh_jobs.retries.enqueue(key, pending.revision);
            if self.mesh_neighbour_is_due(key, now) {
                self.mesh_jobs.retries.block(key, pending.revision);
                if origin == MeshCandidateOrigin::Queued {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .deferred
                        .push(candidate);
                }
                continue;
            }
            let Some(center) = self.authority.terrain().sub_chunk(key) else {
                self.mesh_jobs.retries.block(key, pending.revision);
                if origin == MeshCandidateOrigin::Queued {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .deferred
                        .push(candidate);
                }
                continue;
            };
            let Some(light_halo) = self.mesh_light_halo(key) else {
                self.mesh_jobs.retries.block(key, pending.revision);
                if origin == MeshCandidateOrigin::Queued {
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
                self.mesh_jobs.retries.block(key, pending.revision);
                if origin == MeshCandidateOrigin::Queued {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .deferred
                        .push(candidate);
                }
                continue;
            };
            let snapshot = self.mesh_snapshot(key, center, light_halo);
            self.mesh_jobs.remove_pending(&key);
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
            let biome_records = self.biome_record_cache(key.dimension);
            let tint_identity = self.biome_tint_identity();
            let dispatched_at = Instant::now();
            dispatch.spawn(pending.urgent, move || {
                #[cfg(feature = "tracy")]
                let _zone =
                    tracing::info_span!("mesh.build", key = ?key, revision = pending.revision)
                        .entered();
                let started = Instant::now();
                let queue_wait = queue_wait(pending.queued_at, started);
                let dispatch_wait = started.saturating_duration_since(dispatched_at);
                let source = Arc::clone(&snapshot.center);
                let biome_sources = snapshot.biomes.clone();
                let light_halo = snapshot.light_halo.clone();
                let biome = biome_records.record(&biome_sources);
                let mesh = if cancelled.load(Ordering::Acquire) {
                    ChunkMesh::default()
                } else {
                    snapshot.mesh(classifier, &runtime_assets, network_id_mode, &biome)
                };
                let dependency_mask = if cancelled.load(Ordering::Acquire) {
                    MeshDependencyMask::default()
                } else {
                    snapshot.dependency_mask(classifier, &runtime_assets, network_id_mode)
                };
                output_permit.reconcile(&mesh, &biome);
                let completion = MeshCompletion {
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
                    dispatch_wait,
                    duration: started.elapsed(),
                    urgent: pending.urgent,
                };
                #[cfg(feature = "benchmark-support")]
                super::super::benchmark_support::biomes::observe_mesh(completion.duration);
                #[cfg(feature = "tracy")]
                drop(_zone);
                #[cfg(feature = "tracy")]
                let _zone = tracing::info_span!("mesh.completion_send", key = ?completion.key, revision = completion.revision).entered();
                workers::send_result(&tx, completion);
            });
            self.stats.last_mesh_dispatch_at = Some(Instant::now());
            self.stats.publication_stages.mesh_jobs_dispatched = self
                .stats
                .publication_stages
                .mesh_jobs_dispatched
                .saturating_add(1);
            dispatched += 1;
        }

        drop(dispatch);
        // Collected after dispatch, so air the ready lane yielded is removed in the same call.
        let mut removal_candidates = Vec::new();
        let removal_authority = self
            .publication_allowance
            .as_ref()
            .map_or(removal_budget, |allowance| {
                allowance.zero_byte_admission_capacity_with_priority(true)
            })
            .min(removal_budget)
            .min(MAX_PENDING_MESH_CHANGES.saturating_sub(self.mesh_changes.len()));
        self.route_air_removals(view, removal_authority);
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
            } else if requires_geometry(
                &self.full_detail,
                &self.vertical_detail,
                &self.resident,
                &self.known_air,
                self.authority.terrain(),
                self.classifier,
                key,
            ) {
                self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                    .ready
                    .push(candidate);
            } else if removal_candidates.iter().any(
                |(collected, _): &(PendingSchedulerCandidate, PendingMesh)| collected.key == key,
            ) {
                // A coalesced removal and the original candidate can both reach this lane; one
                // removal spends one permit.
                continue;
            } else if removal_candidates.len() >= removal_authority {
                self.mesh_jobs.lanes[MESH_REMOVAL_LANE]
                    .deferred
                    .push(candidate);
                break;
            } else {
                removal_candidates.push((candidate, pending));
            }
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
            self.mesh_jobs.remove_pending(&key);
            removed = true;
            let resident_air = self.resident.contains(&key)
                && self
                    .authority
                    .terrain()
                    .sub_chunk(key)
                    .is_some_and(|source| self.classifier.is_sub_chunk_air(&source));
            if self.known_air.contains(&key) || resident_air {
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
            self.queue_mesh_change(change);
        }
        dispatched
    }
    /// Moves due air sections from the keyed set into the removal lane, where removals order by
    /// priority under the poll deadline. Bounded attempts resume after the last key examined, so
    /// stale entries are cleared and sections that need geometry again return to the worker queue
    /// whatever the removal credit.
    fn route_air_removals(&mut self, view: SchedulerView, removal_authority: usize) {
        self.prune_air_requeued();
        let mut due = Vec::new();
        let entry = |(key, revision): (&SubChunkKey, &u64)| (*key, *revision);
        let keys = match self.air_removal_cursor.take() {
            Some(start) => self
                .air_removals
                .range((std::ops::Bound::Excluded(start), std::ops::Bound::Unbounded))
                .chain(self.air_removals.range(..=start))
                .map(entry)
                .take(MAX_PENDING_SCHEDULER_SCANS_PER_POLL)
                .collect::<Vec<_>>(),
            None => self
                .air_removals
                .iter()
                .map(entry)
                .take(MAX_PENDING_SCHEDULER_SCANS_PER_POLL)
                .collect(),
        };
        for (attempt, (key, revision)) in keys.into_iter().enumerate() {
            if attempt != 0 && self.poll_budget_exhausted() {
                break;
            }
            self.air_removal_cursor = Some(key);
            let pending = self.mesh_jobs.pending.get(&key).copied().filter(|pending| {
                pending.revision == revision && self.revisions.is_current(key, revision)
            });
            // A missing or newer revision is routed at ingress.
            let Some(pending) = pending else {
                self.air_removals.remove(&key);
                continue;
            };
            let candidate = PendingSchedulerCandidate::new(key, revision, view, pending.urgent);
            if self.mesh_requires_geometry(key) {
                self.air_removals.remove(&key);
                // One returned record per revision, however often the section flips while the
                // worker queue is full.
                if self.air_requeued.insert(key, revision) != Some(revision) {
                    self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                        .ready
                        .push(candidate);
                }
            } else {
                due.push(candidate);
            }
        }
        // Removal credit goes to the highest-priority air first; the rest wait in the set.
        due.sort_unstable_by_key(|candidate| std::cmp::Reverse(*candidate));
        for candidate in due.into_iter().take(removal_authority) {
            self.air_removals.remove(&candidate.key);
            self.mesh_jobs.lanes[MESH_REMOVAL_LANE]
                .ready
                .push(candidate);
        }
    }
    /// Drops a bounded, rotating slice of returned-section records whose pending revision moved
    /// on: a newer revision has its own queue record, so the older return no longer stands for it.
    fn prune_air_requeued(&mut self) {
        if self.air_requeued.is_empty() || self.poll_budget_exhausted() {
            return;
        }
        let entry = |(key, revision): (&SubChunkKey, &u64)| (*key, *revision);
        let entries = match self.air_requeue_cursor.take() {
            Some(start) => self
                .air_requeued
                .range((std::ops::Bound::Excluded(start), std::ops::Bound::Unbounded))
                .chain(self.air_requeued.range(..=start))
                .map(entry)
                .take(AIR_REQUEUE_PRUNES_PER_DISPATCH)
                .collect::<Vec<_>>(),
            None => self
                .air_requeued
                .iter()
                .map(entry)
                .take(AIR_REQUEUE_PRUNES_PER_DISPATCH)
                .collect(),
        };
        for (key, revision) in entries {
            self.air_requeue_cursor = Some(key);
            if self
                .mesh_jobs
                .pending
                .get(&key)
                .is_none_or(|pending| pending.revision != revision)
            {
                self.air_requeued.remove(&key);
            }
        }
    }
    /// Whether `key` still needs a worker-built mesh rather than a removal.
    pub(in crate::stream) fn mesh_requires_geometry(&self, key: SubChunkKey) -> bool {
        requires_geometry(
            &self.full_detail,
            &self.vertical_detail,
            &self.resident,
            &self.known_air,
            self.authority.terrain(),
            self.classifier,
            key,
        )
    }
    /// Returns an unexamined lane or probe record; FIFO retries are already retained.
    fn requeue_mesh_candidate(
        &mut self,
        candidate: PendingSchedulerCandidate,
        origin: MeshCandidateOrigin,
    ) {
        match origin {
            MeshCandidateOrigin::Queued => self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                .ready
                .push(candidate),
            MeshCandidateOrigin::Probe => self.mesh_jobs.refresh.requeue_woken(candidate.key),
            MeshCandidateOrigin::Retry => {}
        }
    }
    /// Faces, AO and smooth light sample all 26 neighbours; meshing before one the server still
    /// owes arrives bakes a hole or dark corner, so the mesh waits for it.
    pub(in crate::stream) fn mesh_neighbour_is_due(
        &mut self,
        key: SubChunkKey,
        now: Instant,
    ) -> bool {
        // The 26 neighbours share nine columns, so each column's facts are read once; a loaded
        // column with no outstanding requests owes nothing, whatever the height.
        let mut columns = [None::<ColumnDue>; 9];
        for neighbour in key.mesh_neighbourhood_dependents() {
            if neighbour == key {
                continue;
            }
            let index = (i64::from(neighbour.x) - i64::from(key.x) + 1) * 3
                + (i64::from(neighbour.z) - i64::from(key.z) + 1);
            let column = *columns[index as usize]
                .get_or_insert_with(|| self.column_due(neighbour.chunk(), now));
            if !column.settled() && self.sub_chunk_is_due_in(neighbour, || column) {
                return true;
            }
        }
        false
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
            rendering: self.mesh_render_settings,
            center,
            biomes: self.biome_neighbourhood(key),
            adjacent,
            column: self
                .authority
                .terrain()
                .chunk(key.chunk())
                .map(world::Chunk::shared_sub_chunks),
            center_y: key.y,
            light_halo,
            java_meshing: self.java_meshing.clone(),
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
                        (Some(key), Some(slot))
                            if !self.mesh_light_slot_is_current(
                                key,
                                slot,
                                [-i32::from(dx), -i32::from(dy), -i32::from(dz)],
                            ) =>
                        {
                            return false;
                        }
                        _ => {}
                    }
                }
            }
        }
        true
    }
    /// Whether `slot` still gives the mesh centred `toward_center` from it the light it would
    /// sample now; a relight that leaves the sampled shell unchanged keeps the mesh current.
    pub(in crate::stream) fn mesh_light_slot_is_current(
        &self,
        key: SubChunkKey,
        slot: &MeshLightSlot,
        toward_center: [i32; 3],
    ) -> bool {
        slot.key == key
            && matches!(
                self.light_source_state(key),
                LightSourceState::Current { ownership, light }
                    if ownership.block_generation == slot.block_generation
                        && ((ownership.light_revision == slot.light_revision
                            && Arc::ptr_eq(light, &slot.light))
                            || !sampled_light_changed(&slot.light, light, toward_center))
            )
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
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("mesh.completion", key = ?completion.key, revision = completion.revision).entered();
        completion._job_permit.take();
        self.stats.publication_stages.mesh_jobs_completed = self
            .stats
            .publication_stages
            .mesh_jobs_completed
            .saturating_add(1);
        self.stats
            .observe_mesh_queue_wait(completion.queue_wait, completion.dispatch_wait);
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
                || !self.retry_staged_front()
            {
                break;
            }
        }
    }

    /// Retries at most `limit` urgent staged completions, which staging keeps at the front.
    pub(in crate::stream) fn retry_urgent_staged_mesh_completions(&mut self, limit: usize) {
        for _ in 0..limit {
            if self.mesh_changes.len() >= MAX_PENDING_MESH_CHANGES
                || !self
                    .staged_mesh_completions
                    .front()
                    .is_some_and(|completion| completion.urgent)
                || !self.retry_staged_front()
            {
                break;
            }
        }
    }

    /// Publishes the front staged completion; false when it is denied again or none is staged.
    fn retry_staged_front(&mut self) -> bool {
        let Some(completion) = self.staged_mesh_completions.pop_front() else {
            return false;
        };
        let bytes = chunk_publication_byte_len(&completion.mesh, &completion.biome);
        self.staged_mesh_bytes -= bytes;
        if let Some(denied) = self.publish_mesh_completion(completion) {
            self.staged_mesh_bytes += bytes;
            self.staged_mesh_completions.push_front(denied);
            return false;
        }
        true
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
            || !self.wants_section_mesh(completion.key)
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
        self.queue_mesh_change(change);
        None
    }
}

/// Resident sections whose palettes are all air, and sections past full detail, publish an
/// empty mesh without a worker.
fn requires_geometry(
    full_detail: &super::super::full_detail::FullDetail,
    vertical: &super::super::vertical_detail::VerticalDetail,
    resident: &ColumnSubChunkSet,
    known_air: &ColumnSubChunkSet,
    terrain: &ChunkStore,
    classifier: BlockClassifier,
    key: SubChunkKey,
) -> bool {
    full_detail.wants_mesh(key.chunk(), resident)
        && vertical.includes(key.y)
        && resident.contains(&key)
        && !known_air.contains(&key)
        && terrain
            .sub_chunk(key)
            .is_none_or(|source| !classifier.is_sub_chunk_air(&source))
}

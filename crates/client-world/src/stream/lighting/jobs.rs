use super::super::*;

impl WorldStream {
    pub(in crate::stream) fn dispatch_light_jobs(
        &mut self,
        camera_position: [f32; 3],
        budget: usize,
    ) -> usize {
        let light_job_cap = if self.pending_light.len() > INITIAL_LIGHT_BACKLOG_THRESHOLD
            || self.pending_mesh.len() > INITIAL_LIGHT_BACKLOG_THRESHOLD
        {
            initial_light_job_cap()
        } else {
            effective_light_job_cap()
        };
        let worker_budget = light_job_cap.saturating_sub(self.in_flight_light_batches.len());
        let solve_budget = budget.min(worker_budget);
        if self.fatal_light_failure || solve_budget == 0 {
            return 0;
        }

        let camera_cell = scheduler_camera_cell(camera_position);
        if self.light_scheduler_camera_cell != Some(camera_cell) {
            let mut deferred = std::mem::take(&mut self.pending_light_deferred)
                .into_iter()
                .filter_map(|candidate| {
                    self.pending_light
                        .get(&candidate.key)
                        .is_some_and(|pending| pending.revision == candidate.revision)
                        .then_some((candidate.key, candidate.revision))
                })
                .collect::<HashSet<_>>();
            deferred.extend(self.pending_light_scan.iter().copied());
            let waiting = self
                .light_waiters
                .values()
                .flat_map(|waiters| waiters.iter().copied())
                .collect::<HashSet<_>>();
            let mut ready = Vec::new();
            let mut next_round = Vec::new();
            for (&key, pending) in &self.pending_light {
                if waiting.contains(&key) {
                    continue;
                }
                let candidate = PendingSchedulerCandidate::new(
                    key,
                    pending.revision,
                    camera_position,
                    pending.urgent,
                );
                if pending.urgent
                    || self.light_priority_wakeups.get(&key) == Some(&pending.revision)
                {
                    ready.push(candidate);
                } else if deferred.contains(&(key, pending.revision)) {
                    next_round.push(candidate);
                } else {
                    ready.push(candidate);
                }
            }
            self.pending_light_ready = BinaryHeap::from(ready);
            self.pending_light_deferred = BinaryHeap::from(next_round);
            self.pending_light_scan.clear();
            self.light_scheduler_camera_cell = Some(camera_cell);
        } else {
            let ingress_budget = self
                .pending_light_scan
                .len()
                .min(MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
            for _ in 0..ingress_budget {
                let Some((key, queued_revision)) = self.pending_light_scan.pop_front() else {
                    break;
                };
                let Some(pending) = self
                    .pending_light
                    .get(&key)
                    .copied()
                    .filter(|pending| pending.revision == queued_revision)
                else {
                    continue;
                };
                let candidate = PendingSchedulerCandidate::new(
                    key,
                    queued_revision,
                    camera_position,
                    pending.urgent,
                );
                if pending.urgent || self.light_priority_wakeups.get(&key) == Some(&queued_revision)
                {
                    self.pending_light_ready.push(candidate);
                } else {
                    self.pending_light_deferred.push(candidate);
                }
            }
        }
        if self.pending_light_ready.is_empty() {
            std::mem::swap(
                &mut self.pending_light_ready,
                &mut self.pending_light_deferred,
            );
        }

        let mut prepared_batches = Vec::with_capacity(solve_budget);
        let mut selected = HashSet::new();
        let mut scanned = 0;
        while prepared_batches.len() < solve_budget
            && scanned < MAX_PENDING_SCHEDULER_SCANS_PER_POLL
        {
            let Some(mut candidate) = self.pending_light_ready.pop() else {
                break;
            };
            scanned += 1;
            if let Some((highest_key, highest_pending)) =
                self.highest_pending_light_in_column(candidate.key)
                && highest_key != candidate.key
            {
                self.pending_light_deferred.push(candidate);
                candidate = PendingSchedulerCandidate::new(
                    highest_key,
                    highest_pending.revision,
                    camera_position,
                    highest_pending.urgent,
                );
            }
            let key = candidate.key;
            let revision = candidate.revision;
            let Some(pending) = self.pending_light.get(&key).copied() else {
                continue;
            };
            if pending.revision != revision {
                continue;
            }
            if !self.light_revisions.is_current(key, revision) {
                self.pending_light_deferred.push(candidate);
                continue;
            }
            if self.in_flight_light.contains_key(&key) {
                self.pending_light_deferred.push(candidate);
                continue;
            }
            if !self.resident.contains(&key) {
                self.pending_light_deferred.push(candidate);
                continue;
            }
            if !self.original_light_column_context_ready(key) {
                self.pending_light_deferred.push(candidate);
                continue;
            }
            if !self.light_dispatch_ready(key) {
                if let Some(above) = offset_sub_chunk_key(key, [0, 1, 0]) {
                    self.light_waiters.entry(above).or_default().insert(key);
                }
                self.light_priority_wakeups.remove(&key);
                continue;
            }
            if key
                .mesh_dependents()
                .filter(|candidate| *candidate != key)
                .any(|neighbour| {
                    selected.contains(&neighbour) || self.in_flight_light.contains_key(&neighbour)
                })
            {
                self.pending_light_deferred.push(candidate);
                continue;
            }
            let Some(block_generation) = self.block_generations.get(&key).copied() else {
                self.pending_light_deferred.push(candidate);
                continue;
            };
            let Some(bounds) = light_bounds(key) else {
                self.pending_light.remove(&key);
                self.light_priority_wakeups.remove(&key);
                continue;
            };

            self.next_light_batch_id = self.next_light_batch_id.wrapping_add(1).max(1);
            let batch_id = self.next_light_batch_id;
            let mut batch_keys = HashSet::from([key]);
            let mut batch_inputs = vec![(key, pending, block_generation, bounds)];
            let mut lower = key;
            while batch_inputs.len() < MAX_LIGHT_COLUMN_BATCH_SUB_CHUNKS {
                let Some(next) = offset_sub_chunk_key(lower, [0, -1, 0]) else {
                    break;
                };
                let Some(next_pending) = self.pending_light.get(&next).copied() else {
                    break;
                };
                if !self.light_revisions.is_current(next, next_pending.revision)
                    || self.in_flight_light.contains_key(&next)
                    || !self.resident.contains(&next)
                {
                    break;
                }
                if next
                    .mesh_dependents()
                    .filter(|candidate| *candidate != next)
                    .any(|neighbour| {
                        !batch_keys.contains(&neighbour)
                            && (selected.contains(&neighbour)
                                || self.in_flight_light.contains_key(&neighbour))
                    })
                {
                    break;
                }
                let Some(next_block_generation) = self.block_generations.get(&next).copied() else {
                    break;
                };
                let Some(next_bounds) = light_bounds(next) else {
                    self.pending_light.remove(&next);
                    self.light_priority_wakeups.remove(&next);
                    break;
                };
                batch_keys.insert(next);
                batch_inputs.push((next, next_pending, next_block_generation, next_bounds));
                lower = next;
            }
            let batch_urgent = batch_inputs.iter().any(|(_, pending, _, _)| pending.urgent);
            let batch = batch_inputs
                .into_iter()
                .map(|(key, mut pending, block_generation, bounds)| {
                    pending.urgent = batch_urgent;
                    self.take_prepared_light_job(
                        key,
                        pending,
                        block_generation,
                        bounds,
                        &batch_keys,
                        batch_id,
                    )
                })
                .collect::<Vec<_>>();
            for member in &batch_keys {
                self.last_dispatched_light_batch.insert(*member, batch_id);
            }
            selected.extend(batch_keys);
            self.in_flight_light_batches.insert(batch_id, batch.len());
            prepared_batches.push(batch);
        }

        let dispatched = prepared_batches.iter().map(Vec::len).sum::<usize>();
        self.stats.phase2_stages.light_jobs_dispatched = self
            .stats
            .phase2_stages
            .light_jobs_dispatched
            .saturating_add(dispatched as u64);
        for batch in prepared_batches {
            let tx = self.light_tx.clone();
            rayon::spawn(move || {
                let started = Instant::now();
                let solved = solve_prepared_light_batch(batch);
                let duration = started.elapsed();
                for entry in solved {
                    let _ = tx.send(LightCompletion {
                        key: entry.key,
                        identity: entry.identity,
                        result: entry.result,
                        queue_wait: queue_wait(entry.queued_at, started),
                        duration,
                    });
                }
            });
        }
        dispatched
    }
    fn take_prepared_light_job(
        &mut self,
        key: SubChunkKey,
        pending: PendingLight,
        block_generation: u64,
        bounds: LightBounds,
        retained_batch: &HashSet<SubChunkKey>,
        batch_id: u64,
    ) -> PreparedLightJob {
        if !self.light_ownership.contains_key(&key) {
            debug_assert!(self.light_store.light(key).is_some());
        }
        self.remove_light_waiter_target(key);
        let blocks = self.light_block_snapshot(key);
        self.register_untrusted_light_waiters(key, retained_batch);
        let prior = self.light_prior_snapshot(key);
        let identity = LightJobIdentity {
            revision: pending.revision,
            block_generation,
            previous_light_generation: self.light_store.light(key).map(|light| light.generation()),
            batch_id,
            urgent: pending.urgent,
        };
        self.pending_light.remove(&key);
        self.light_priority_wakeups.remove(&key);
        self.in_flight_light.insert(key, identity);
        PreparedLightJob {
            key,
            identity,
            blocks,
            prior,
            bounds,
            queued_at: pending.queued_at,
        }
    }
    pub(in crate::stream) fn accept_light_completion(&mut self, completion: LightCompletion) {
        self.stats.phase2_stages.light_jobs_completed = self
            .stats
            .phase2_stages
            .light_jobs_completed
            .saturating_add(1);
        self.stats.observe_light_queue_wait(completion.queue_wait);
        self.remove_in_flight_light(completion.key, Some(completion.identity));
        if self.fatal_light_failure {
            self.remove_light_waiters_for(completion.key);
            self.stats.stale_light_jobs = self.stats.stale_light_jobs.saturating_add(1);
            return;
        }
        let current = self
            .light_revisions
            .is_current(completion.key, completion.identity.revision)
            && self.block_generations.get(&completion.key).copied()
                == Some(completion.identity.block_generation)
            && self.resident.contains(&completion.key)
            && self
                .light_store
                .light(completion.key)
                .map(|light| light.generation())
                == completion.identity.previous_light_generation;
        if !current {
            self.stats.stale_light_jobs = self.stats.stale_light_jobs.saturating_add(1);
            return;
        }
        let solved = match completion.result {
            Ok(solved) => solved,
            Err(error) => {
                let fatal = match error {
                    LightJobError::Solve(error) => WorldStreamFatalError::LightSolve {
                        key: completion.key,
                        error,
                    },
                    LightJobError::MissingTargetOutput => {
                        WorldStreamFatalError::MissingLightTarget {
                            key: completion.key,
                        }
                    }
                };
                self.light_failures.insert(
                    completion.key,
                    LightFailure {
                        revision: completion.identity.revision,
                        block_generation: completion.identity.block_generation,
                        error,
                    },
                );
                self.fatal_light_failure = true;
                self.fatal_error = Some(fatal);
                self.pending_light.clear();
                self.pending_light_scan.clear();
                self.pending_light_ready.clear();
                self.pending_light_deferred.clear();
                self.light_priority_wakeups.clear();
                self.light_waiters.clear();
                self.stats.light_solve_failures = self.stats.light_solve_failures.saturating_add(1);
                return;
            }
        };
        let SolvedLightJob {
            replacement,
            direct_sky,
            used_uniform_fast_path,
            light_levels_changed,
            direct_sky_changed,
            changed_faces,
        } = solved;
        if used_uniform_fast_path {
            self.stats.light_uniform_fast_path_jobs =
                self.stats.light_uniform_fast_path_jobs.saturating_add(1);
        }
        if !light_levels_changed && !direct_sky_changed {
            let Some(light_revision) = completion.identity.previous_light_generation else {
                self.stats.stale_light_jobs = self.stats.stale_light_jobs.saturating_add(1);
                return;
            };
            let Some(current_direct) = self
                .direct_sky
                .get(&completion.key)
                .filter(|direct| direct.light_revision == light_revision)
                .cloned()
            else {
                self.stats.stale_light_jobs = self.stats.stale_light_jobs.saturating_add(1);
                return;
            };
            self.light_ownership.insert(
                completion.key,
                LightOwnership {
                    block_generation: completion.identity.block_generation,
                    light_revision,
                },
            );
            self.light_revisions
                .clear_if_current(completion.key, completion.identity.revision);
            self.stats.max_light_duration = self.stats.max_light_duration.max(completion.duration);
            self.stats.accepted_light_jobs = self.stats.accepted_light_jobs.saturating_add(1);
            self.stats.noop_light_jobs = self.stats.noop_light_jobs.saturating_add(1);
            self.finish_accepted_light_completion(
                completion.key,
                completion.identity.batch_id,
                &current_direct,
                changed_faces,
                completion.identity.urgent,
            );
            return;
        }
        if !light_levels_changed && direct_sky_changed {
            let Some(light_revision) = completion.identity.previous_light_generation else {
                self.stats.stale_light_jobs = self.stats.stale_light_jobs.saturating_add(1);
                return;
            };
            let new_direct = StoredDirectSky {
                light_revision,
                mask: direct_sky,
            };
            self.light_ownership.insert(
                completion.key,
                LightOwnership {
                    block_generation: completion.identity.block_generation,
                    light_revision,
                },
            );
            self.direct_sky.insert(completion.key, new_direct.clone());
            self.light_revisions
                .clear_if_current(completion.key, completion.identity.revision);
            self.stats.max_light_duration = self.stats.max_light_duration.max(completion.duration);
            self.stats.accepted_light_jobs = self.stats.accepted_light_jobs.saturating_add(1);
            self.stats.provenance_only_light_jobs =
                self.stats.provenance_only_light_jobs.saturating_add(1);
            self.finish_accepted_light_completion(
                completion.key,
                completion.identity.batch_id,
                &new_direct,
                changed_faces,
                completion.identity.urgent,
            );
            return;
        }
        let new_direct = StoredDirectSky {
            light_revision: completion.identity.revision,
            mask: direct_sky,
        };
        if !self.light_store.commit_if_generation(
            completion.key,
            completion.identity.previous_light_generation,
            replacement,
        ) {
            self.stats.stale_light_jobs = self.stats.stale_light_jobs.saturating_add(1);
            return;
        }
        self.light_ownership.insert(
            completion.key,
            LightOwnership {
                block_generation: completion.identity.block_generation,
                light_revision: completion.identity.revision,
            },
        );
        self.direct_sky.insert(completion.key, new_direct.clone());
        self.light_revisions
            .clear_if_current(completion.key, completion.identity.revision);
        self.stats.max_light_duration = self.stats.max_light_duration.max(completion.duration);
        self.stats.accepted_light_jobs = self.stats.accepted_light_jobs.saturating_add(1);
        self.stats.value_changed_light_jobs = self.stats.value_changed_light_jobs.saturating_add(1);
        self.stats.light_mesh_invalidations = self.stats.light_mesh_invalidations.saturating_add(1);
        self.mark_changed_light_mesh_dependents(
            completion.key,
            changed_faces,
            Instant::now(),
            completion.identity.urgent,
        );

        self.finish_accepted_light_completion(
            completion.key,
            completion.identity.batch_id,
            &new_direct,
            changed_faces,
            completion.identity.urgent,
        );
    }
    pub(in crate::stream) fn finish_accepted_light_completion(
        &mut self,
        key: SubChunkKey,
        batch_id: u64,
        direct_sky: &StoredDirectSky,
        changed_faces: [bool; 6],
        urgent: bool,
    ) {
        let mut requeue = self.light_waiters.remove(&key).unwrap_or_default();
        let completed_uniform_direct_sky = self
            .light_store
            .light(key)
            .is_some_and(|light| is_uniform_direct_sky(light, direct_sky.mask.as_ref()));
        for (offset, changed) in LIGHT_NEIGHBOUR_OFFSETS.into_iter().zip(changed_faces) {
            if !changed {
                continue;
            }
            if let Some(neighbour) = offset_sub_chunk_key(key, offset) {
                let neighbour_in_flight = self.in_flight_light.contains_key(&neighbour);
                let neighbour_in_same_batch =
                    self.last_dispatched_light_batch.get(&neighbour) == Some(&batch_id);
                if !neighbour_in_flight
                    && let Some(pending) = self.pending_light.get_mut(&neighbour)
                {
                    let revision = pending.revision;
                    if urgent {
                        pending.urgent = true;
                        self.pending_light_scan.push_front((neighbour, revision));
                    }
                    self.light_priority_wakeups.insert(neighbour, revision);
                    continue;
                }
                if neighbour_in_same_batch {
                    continue;
                }
                if completed_uniform_direct_sky && self.known_air_has_vertical_direct_sky(neighbour)
                {
                    continue;
                }
                requeue.insert(neighbour);
            }
        }
        for neighbour in requeue {
            if let Some(revision) = self.mark_light_dirty_exact_with_priority(neighbour, urgent) {
                self.light_priority_wakeups.insert(neighbour, revision);
            }
        }
    }
    pub(in crate::stream) fn known_air_has_vertical_direct_sky(&self, key: SubChunkKey) -> bool {
        if key.dimension != 0 || !self.known_air.contains(&key) {
            return false;
        }
        let top_sub_chunk_y = vanilla_dimension_range(0).and_then(|range| {
            range
                .base_sub_chunk_y
                .checked_add(i32::try_from(range.sub_chunk_count).ok()?)?
                .checked_sub(1)
        });
        if Some(key.y) == top_sub_chunk_y {
            return true;
        }
        let Some(above) = offset_sub_chunk_key(key, [0, 1, 0]) else {
            return false;
        };
        self.light_is_current(above)
            && self.light_store.light(above).is_some_and(|light| {
                self.direct_sky
                    .get(&above)
                    .is_some_and(|direct| is_uniform_direct_sky(light, direct.mask.as_ref()))
            })
    }
}

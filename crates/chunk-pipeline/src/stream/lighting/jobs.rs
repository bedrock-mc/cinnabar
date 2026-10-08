use super::super::*;

impl WorldStream {
    /// Keeps nearby pending work's priority while executing its highest light dependency.
    pub(in crate::stream) fn near_light_column_candidate(
        &self,
        key: SubChunkKey,
        view: SchedulerView,
    ) -> Option<PendingSchedulerCandidate> {
        let (highest, pending) = self.highest_pending_light_in_column(key)?;
        if !self.resident.contains(&highest)
            || !self
                .lighting
                .revisions
                .is_current(highest, pending.revision)
            || self.lighting.jobs.in_flight.contains_key(&highest)
            || !self.original_light_column_context_ready(highest)
            || !self.light_dispatch_ready(highest)
        {
            return None;
        }
        let mut candidate =
            PendingSchedulerCandidate::new(highest, pending.revision, view, pending.urgent);
        let mut priority = candidate;
        for member in self.light_column_sources(key) {
            if let Some(pending) = self.lighting.jobs.pending.get(&member) {
                priority = priority.max(PendingSchedulerCandidate::new(
                    member,
                    pending.revision,
                    view,
                    pending.urgent,
                ));
            }
        }
        candidate.distance_squared = priority.distance_squared;
        candidate.startup_class = priority.startup_class;
        candidate.urgent = priority.urgent;
        Some(candidate)
    }

    pub(in crate::stream) fn dispatch_light_jobs(
        &mut self,
        camera_position: [f32; 3],
        budget: usize,
    ) -> usize {
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!(
            "light.dispatch",
            budget,
            pending = self.lighting.jobs.pending.len()
        )
        .entered();
        let light_job_cap = if self.lighting.jobs.pending.len() > INITIAL_LIGHT_BACKLOG_THRESHOLD
            || self.mesh_jobs.pending.len() > INITIAL_LIGHT_BACKLOG_THRESHOLD
        {
            initial_light_job_cap()
        } else {
            effective_light_job_cap()
        };
        let occupied = self
            .lighting
            .in_flight_batches
            .len()
            .max(self.lighting.running_jobs.load(Ordering::Acquire));
        let worker_budget = light_job_cap.saturating_sub(occupied);
        let solve_budget = budget.min(worker_budget);
        if self.lighting.fatal_failure || solve_budget == 0 || self.lighting.jobs.pending.is_empty()
        {
            return 0;
        }

        let view = self.scheduler_view(camera_position);
        let wakeups = &self.lighting.priority_wakeups;
        let probe_near =
            self.lighting
                .jobs
                .ingress(view, self.poll_deadline, |key, revision, pending| {
                    (0, pending.urgent || wakeups.get(&key) == Some(&revision))
                });

        let mut near = self.transfer_light_candidates();
        if probe_near {
            near.extend(
                scheduler::near_light_columns(view, self.authority.current_dimension())
                    .filter_map(|key| self.near_light_column_candidate(key, view)),
            );
        }
        let mut prepared_batches = Vec::with_capacity(solve_budget);
        let mut selected = HashSet::new();
        let mut scanned = 0;
        while prepared_batches.len() < solve_budget
            && scanned < MAX_PENDING_SCHEDULER_SCANS_PER_POLL
            && (prepared_batches.is_empty() || !self.poll_budget_exhausted())
        {
            let queued = self.lighting.jobs.lanes[0]
                .ready
                .peek()
                .is_some_and(|candidate| near.peek().is_none_or(|local| candidate >= local));
            let Some(mut candidate) = (if queued {
                &mut self.lighting.jobs.lanes[0].ready
            } else {
                &mut near
            })
            .pop() else {
                break;
            };
            let mut queued = queued;
            if queued {
                candidate.refresh_rank(view);
            }
            if queued && near.peek().is_some_and(|local| *local > candidate) {
                self.lighting.jobs.lanes[0].ready.push(candidate);
                candidate = near.pop().expect("near candidate was inspected");
                queued = false;
            }
            scanned += 1;
            if let Some((highest_key, highest_pending)) =
                self.highest_pending_light_in_column(candidate.key)
                && highest_key != candidate.key
            {
                if queued {
                    self.lighting.jobs.lanes[0].deferred.push(candidate);
                }
                queued = false;
                let priority = candidate;
                candidate = PendingSchedulerCandidate::new(
                    highest_key,
                    highest_pending.revision,
                    view,
                    highest_pending.urgent || priority.urgent,
                );
                candidate.distance_squared = priority.distance_squared;
                candidate.startup_class = priority.startup_class;
                candidate.transfer = priority.transfer;
            }
            let key = candidate.key;
            let revision = candidate.revision;
            let Some(pending) = self.lighting.jobs.pending.get(&key).copied() else {
                continue;
            };
            if pending.revision != revision {
                continue;
            }
            if !self.lighting.revisions.is_current(key, revision) {
                if queued {
                    self.lighting.jobs.lanes[0].deferred.push(candidate);
                }
                continue;
            }
            if self.lighting.jobs.in_flight.contains_key(&key) {
                if queued {
                    self.lighting.jobs.lanes[0].deferred.push(candidate);
                }
                continue;
            }
            if !self.resident.contains(&key) {
                if queued {
                    self.lighting.jobs.lanes[0].deferred.push(candidate);
                }
                continue;
            }
            if !self.original_light_column_context_ready(key) {
                if queued {
                    self.lighting.jobs.lanes[0].deferred.push(candidate);
                }
                continue;
            }
            if !self.light_dispatch_ready(key) {
                if let Some(above) = offset_sub_chunk_key(key, [0, 1, 0]) {
                    self.lighting.waiters.entry(above).or_default().insert(key);
                }
                self.lighting.priority_wakeups.remove(&key);
                continue;
            }
            if key
                .mesh_dependents()
                .filter(|candidate| *candidate != key)
                .any(|neighbour| {
                    selected.contains(&neighbour)
                        || self.lighting.jobs.in_flight.contains_key(&neighbour)
                })
            {
                if queued {
                    self.lighting.jobs.lanes[0].deferred.push(candidate);
                }
                continue;
            }
            let Some(block_generation) = self.lighting.block_generations.get(&key).copied() else {
                if queued {
                    self.lighting.jobs.lanes[0].deferred.push(candidate);
                }
                continue;
            };
            let Some(bounds) = light_bounds(key) else {
                self.lighting.jobs.pending.remove(&key);
                self.lighting.priority_wakeups.remove(&key);
                continue;
            };

            self.lighting.next_batch_id = self.lighting.next_batch_id.wrapping_add(1).max(1);
            let batch_id = self.lighting.next_batch_id;
            let mut batch_keys = HashSet::from([key]);
            let mut batch_inputs = vec![(key, pending, block_generation, bounds)];
            let mut lower = key;
            while batch_inputs.len() < MAX_LIGHT_COLUMN_BATCH_SUB_CHUNKS {
                let Some(next) = offset_sub_chunk_key(lower, [0, -1, 0]) else {
                    break;
                };
                let Some(next_pending) = self.lighting.jobs.pending.get(&next).copied() else {
                    break;
                };
                if !self
                    .lighting
                    .revisions
                    .is_current(next, next_pending.revision)
                    || self.lighting.jobs.in_flight.contains_key(&next)
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
                                || self.lighting.jobs.in_flight.contains_key(&neighbour))
                    })
                {
                    break;
                }
                let Some(next_block_generation) =
                    self.lighting.block_generations.get(&next).copied()
                else {
                    break;
                };
                let Some(next_bounds) = light_bounds(next) else {
                    self.lighting.jobs.pending.remove(&next);
                    self.lighting.priority_wakeups.remove(&next);
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
                self.lighting
                    .last_dispatched_batch
                    .insert(*member, batch_id);
            }
            selected.extend(batch_keys);
            self.lighting
                .in_flight_batches
                .insert(batch_id, batch.len());
            prepared_batches.push(batch);
        }

        let dispatched = prepared_batches.iter().map(Vec::len).sum::<usize>();
        self.stats.phase2_stages.light_jobs_dispatched = self
            .stats
            .phase2_stages
            .light_jobs_dispatched
            .saturating_add(dispatched as u64);
        let mut dispatch = workers::WORKERS.batch(workers::Lane::Light);
        for batch in prepared_batches {
            let tx = self.lighting.tx.clone();
            let running = RunningLightJob::start(&self.lighting.running_jobs);
            dispatch.spawn_with_scratch(move |scratch| {
                #[cfg(feature = "tracy")]
                let _zone = tracing::info_span!("light.solve", sections = batch.len()).entered();
                let started = Instant::now();
                let solved = solve_prepared_light_batch_with_scratch(batch, scratch);
                let duration = started.elapsed();
                #[cfg(feature = "tracy")]
                drop(_zone);
                // Release the worker slot before publishing: a drained completion means a free slot.
                drop(running);
                for entry in solved {
                    let completion = LightCompletion {
                        key: entry.key,
                        identity: entry.identity,
                        result: entry.result,
                        queue_wait: queue_wait(entry.queued_at, started),
                        duration,
                    };
                    #[cfg(feature = "tracy")]
                    let _zone = tracing::info_span!("light.completion_send", key = ?completion.key)
                        .entered();
                    let _ = tx.send(completion);
                }
            });
        }
        drop(dispatch);
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
        if !self.lighting.ownership.contains_key(&key) {
            debug_assert!(self.lighting.store.light(key).is_some());
        }
        self.lighting.remove_waiter_target(key);
        let blocks = self.light_block_snapshot(key);
        self.register_untrusted_light_waiters(key, retained_batch);
        let prior = self.light_prior_snapshot(key);
        let identity = LightJobIdentity {
            revision: pending.revision,
            block_generation,
            previous_light_generation: self
                .lighting
                .store
                .light(key)
                .map(|light| light.generation()),
            batch_id,
            urgent: pending.urgent,
        };
        self.lighting.jobs.pending.remove(&key);
        self.lighting.priority_wakeups.remove(&key);
        self.lighting.jobs.in_flight.insert(key, identity);
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
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("light.completion", key = ?completion.key).entered();
        self.stats.phase2_stages.light_jobs_completed = self
            .stats
            .phase2_stages
            .light_jobs_completed
            .saturating_add(1);
        self.stats.observe_light_queue_wait(completion.queue_wait);
        self.lighting
            .remove_in_flight(completion.key, Some(completion.identity));
        if self.lighting.fatal_failure {
            self.lighting.remove_waiters_for(completion.key);
            self.record_stale_light(completion.key, completion.identity, "fatal_failure");
            return;
        }
        let current = self
            .lighting
            .revisions
            .is_current(completion.key, completion.identity.revision)
            && self
                .lighting
                .block_generations
                .get(&completion.key)
                .copied()
                == Some(completion.identity.block_generation)
            && self.resident.contains(&completion.key)
            && self
                .lighting
                .store
                .light(completion.key)
                .map(|light| light.generation())
                == completion.identity.previous_light_generation;
        if !current {
            self.record_stale_light(completion.key, completion.identity, "identity_changed");
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
                self.lighting.failures.insert(
                    completion.key,
                    LightFailure {
                        revision: completion.identity.revision,
                        block_generation: completion.identity.block_generation,
                        error,
                    },
                );
                self.lighting.fatal_failure = true;
                self.fatal_error = Some(fatal);
                self.lighting.clear_after_fatal();
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
        let monotonic_faces =
            self.monotonic_light_faces(completion.key, &replacement, direct_sky.as_ref());
        if used_uniform_fast_path {
            self.stats.light_uniform_fast_path_jobs =
                self.stats.light_uniform_fast_path_jobs.saturating_add(1);
        }
        if !light_levels_changed && !direct_sky_changed {
            let Some(light_revision) = completion.identity.previous_light_generation else {
                self.record_stale_light(
                    completion.key,
                    completion.identity,
                    "missing_noop_generation",
                );
                return;
            };
            let Some(current_direct) = self
                .lighting
                .direct_sky
                .get(&completion.key)
                .filter(|direct| direct.light_revision == light_revision)
                .cloned()
            else {
                self.record_stale_light(
                    completion.key,
                    completion.identity,
                    "missing_noop_provenance",
                );
                return;
            };
            self.lighting.ownership.insert(
                completion.key,
                LightOwnership {
                    block_generation: completion.identity.block_generation,
                    light_revision,
                },
            );
            self.lighting
                .revisions
                .clear_if_current(completion.key, completion.identity.revision);
            self.stats.max_light_duration = self.stats.max_light_duration.max(completion.duration);
            self.stats.accepted_light_jobs = self.stats.accepted_light_jobs.saturating_add(1);
            self.stats.noop_light_jobs = self.stats.noop_light_jobs.saturating_add(1);
            self.finish_accepted_light_completion_with_dominance(
                completion.key,
                completion.identity.batch_id,
                &current_direct,
                changed_faces,
                completion.identity.urgent,
                monotonic_faces,
            );
            return;
        }
        if !light_levels_changed && direct_sky_changed {
            let Some(light_revision) = completion.identity.previous_light_generation else {
                self.record_stale_light(
                    completion.key,
                    completion.identity,
                    "missing_provenance_generation",
                );
                return;
            };
            let new_direct = StoredDirectSky {
                light_revision,
                mask: direct_sky,
            };
            self.lighting.ownership.insert(
                completion.key,
                LightOwnership {
                    block_generation: completion.identity.block_generation,
                    light_revision,
                },
            );
            self.lighting
                .direct_sky
                .insert(completion.key, new_direct.clone());
            self.lighting
                .revisions
                .clear_if_current(completion.key, completion.identity.revision);
            self.stats.max_light_duration = self.stats.max_light_duration.max(completion.duration);
            self.stats.accepted_light_jobs = self.stats.accepted_light_jobs.saturating_add(1);
            self.stats.provenance_only_light_jobs =
                self.stats.provenance_only_light_jobs.saturating_add(1);
            self.finish_accepted_light_completion_with_dominance(
                completion.key,
                completion.identity.batch_id,
                &new_direct,
                changed_faces,
                completion.identity.urgent,
                monotonic_faces,
            );
            return;
        }
        let new_direct = StoredDirectSky {
            light_revision: completion.identity.revision,
            mask: direct_sky,
        };
        if !self.lighting.store.commit_if_generation(
            completion.key,
            completion.identity.previous_light_generation,
            replacement,
        ) {
            self.record_stale_light(
                completion.key,
                completion.identity,
                "commit_generation_changed",
            );
            return;
        }
        self.lighting.ownership.insert(
            completion.key,
            LightOwnership {
                block_generation: completion.identity.block_generation,
                light_revision: completion.identity.revision,
            },
        );
        self.lighting
            .direct_sky
            .insert(completion.key, new_direct.clone());
        self.lighting
            .revisions
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

        self.finish_accepted_light_completion_with_dominance(
            completion.key,
            completion.identity.batch_id,
            &new_direct,
            changed_faces,
            completion.identity.urgent,
            monotonic_faces,
        );
    }
    #[cfg(test)]
    pub(in crate::stream) fn finish_accepted_light_completion(
        &mut self,
        key: SubChunkKey,
        batch_id: u64,
        direct_sky: &StoredDirectSky,
        changed_faces: [bool; 6],
        urgent: bool,
    ) {
        self.finish_accepted_light_completion_with_dominance(
            key,
            batch_id,
            direct_sky,
            changed_faces,
            urgent,
            [false; 6],
        );
    }
    fn finish_accepted_light_completion_with_dominance(
        &mut self,
        key: SubChunkKey,
        batch_id: u64,
        direct_sky: &StoredDirectSky,
        changed_faces: [bool; 6],
        urgent: bool,
        monotonic_faces: [bool; 6],
    ) {
        let mut requeue = self.lighting.waiters.remove(&key).unwrap_or_default();
        let completed_uniform_direct_sky = self
            .lighting
            .store
            .light(key)
            .is_some_and(|light| is_uniform_direct_sky(light, direct_sky.mask.as_ref()));
        for (offset, changed) in LIGHT_NEIGHBOUR_OFFSETS.into_iter().zip(changed_faces) {
            if !changed {
                continue;
            }
            if let Some(neighbour) = offset_sub_chunk_key(key, offset) {
                let neighbour_in_flight = self.lighting.jobs.in_flight.contains_key(&neighbour);
                let neighbour_in_same_batch =
                    self.lighting.last_dispatched_batch.get(&neighbour) == Some(&batch_id);
                if !neighbour_in_flight
                    && let Some(pending) = self.lighting.jobs.pending.get_mut(&neighbour)
                {
                    let revision = pending.revision;
                    if urgent {
                        pending.urgent = true;
                        self.lighting.jobs.scan.push_front((neighbour, revision));
                    }
                    self.lighting.priority_wakeups.insert(neighbour, revision);
                    continue;
                }
                if neighbour_in_same_batch {
                    continue;
                }
                if completed_uniform_direct_sky
                    && self.known_air.contains(&neighbour)
                    && self.light_is_current(neighbour)
                    && self.lighting.store.light(neighbour).is_some_and(|light| {
                        self.lighting
                            .direct_sky
                            .get(&neighbour)
                            .is_some_and(|direct| {
                                is_uniform_direct_sky(light, direct.mask.as_ref())
                            })
                    })
                {
                    continue;
                }
                requeue.insert(neighbour);
            }
        }
        for neighbour in requeue {
            if self.current_known_target_dominates_source_face(key, neighbour, monotonic_faces) {
                continue;
            }
            if let Some(pending) = self.lighting.jobs.pending.get_mut(&neighbour) {
                let revision = pending.revision;
                let effective_urgent = urgent || pending.urgent;
                if effective_urgent {
                    pending.urgent = true;
                }
                self.lighting
                    .jobs
                    .rescan(neighbour, revision, effective_urgent);
                self.lighting.priority_wakeups.insert(neighbour, revision);
                continue;
            }
            if let Some(revision) = self.mark_light_dirty_exact_with_priority(neighbour, urgent) {
                self.lighting.priority_wakeups.insert(neighbour, revision);
            }
        }
    }
    pub(in crate::stream) fn known_air_has_vertical_direct_sky(&self, key: SubChunkKey) -> bool {
        if key.dimension != 0 || !self.known_air.contains(&key) {
            return false;
        }
        let top_sub_chunk_y = self.light_column_top_sub_chunk_y(key);
        if Some(key.y) == top_sub_chunk_y {
            return true;
        }
        let Some(above) = offset_sub_chunk_key(key, [0, 1, 0]) else {
            return false;
        };
        self.light_is_current(above)
            && self.lighting.store.light(above).is_some_and(|light| {
                self.lighting
                    .direct_sky
                    .get(&above)
                    .is_some_and(|direct| is_uniform_direct_sky(light, direct.mask.as_ref()))
            })
    }
}

/// Occupies one light worker slot until the solve itself finishes.
struct RunningLightJob(Arc<AtomicUsize>);

impl RunningLightJob {
    fn start(running: &Arc<AtomicUsize>) -> Self {
        running.fetch_add(1, Ordering::AcqRel);
        Self(Arc::clone(running))
    }
}

impl Drop for RunningLightJob {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

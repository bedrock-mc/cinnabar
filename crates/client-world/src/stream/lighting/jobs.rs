use super::super::*;

impl WorldStream {
    pub(in crate::stream) fn dispatch_light_jobs(
        &mut self,
        camera_position: [f32; 3],
        budget: usize,
    ) -> usize {
        let worker_budget = MAX_IN_FLIGHT_LIGHT_JOBS.saturating_sub(self.in_flight_light.len());
        let solve_budget = budget.min(worker_budget);
        if self.fatal_light_failure || solve_budget == 0 {
            return 0;
        }
        let mut candidates = self
            .pending_light
            .iter()
            .map(|(&key, &pending)| {
                (
                    distance_squared(key, camera_position),
                    key,
                    pending.revision,
                    pending.queued_at,
                )
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
        });

        let mut prepared = Vec::with_capacity(solve_budget);
        let mut selected_initial_air = HashSet::new();
        for (_, key, revision, queued_at) in candidates {
            if prepared.len() >= solve_budget {
                break;
            }
            if !self.light_revisions.is_current(key, revision)
                || self.in_flight_light.contains_key(&key)
                || !self.resident.contains(&key)
                || !self.light_dispatch_ready(key)
            {
                continue;
            }
            let Some(block_generation) = self.block_generations.get(&key).copied() else {
                continue;
            };
            let initial_known_air =
                self.known_air.contains(&key) && self.light_store.light(key).is_none();
            if initial_known_air
                && key
                    .mesh_dependents()
                    .filter(|candidate| *candidate != key)
                    .any(|neighbour| {
                        selected_initial_air.contains(&neighbour)
                            || self
                                .in_flight_light
                                .get(&neighbour)
                                .is_some_and(|identity| {
                                    identity.previous_light_generation.is_none()
                                        && self.known_air.contains(&neighbour)
                                })
                    })
            {
                continue;
            }
            let Some(bounds) = light_bounds(key) else {
                self.pending_light.remove(&key);
                continue;
            };
            let blocks = self.light_block_snapshot(key);
            self.register_untrusted_light_waiters(key);
            let prior = self.light_prior_snapshot(key);
            let identity = LightJobIdentity {
                revision,
                block_generation,
                previous_light_generation: self
                    .light_store
                    .light(key)
                    .map(|light| light.generation()),
            };
            self.pending_light.remove(&key);
            self.in_flight_light.insert(key, identity);
            prepared.push(PreparedLightJob {
                key,
                identity,
                blocks,
                prior,
                bounds,
                queued_at,
            });
            if initial_known_air {
                selected_initial_air.insert(key);
            }
        }

        let dispatched = prepared.len();
        for job in prepared {
            let tx = self.light_tx.clone();
            rayon::spawn(move || {
                let started = Instant::now();
                let queue_wait = queue_wait(job.queued_at, started);
                let key = job.key;
                let identity = job.identity;
                let result = solve_prepared_light_job(job);
                let _ = tx.send(LightCompletion {
                    key,
                    identity,
                    result,
                    queue_wait,
                    duration: started.elapsed(),
                });
            });
        }
        dispatched
    }
    pub(in crate::stream) fn accept_light_completion(&mut self, completion: LightCompletion) {
        self.stats.observe_light_queue_wait(completion.queue_wait);
        if self.in_flight_light.get(&completion.key) == Some(&completion.identity) {
            self.in_flight_light.remove(&completion.key);
        }
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
            self.finish_accepted_light_completion(completion.key, &current_direct, changed_faces);
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
            self.finish_accepted_light_completion(completion.key, &new_direct, changed_faces);
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
        self.mark_light_mesh_dependents(completion.key, Instant::now());

        self.finish_accepted_light_completion(completion.key, &new_direct, changed_faces);
    }
    pub(in crate::stream) fn finish_accepted_light_completion(
        &mut self,
        key: SubChunkKey,
        direct_sky: &StoredDirectSky,
        changed_faces: [bool; 6],
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
                if self.pending_light.contains_key(&neighbour)
                    && !self.in_flight_light.contains_key(&neighbour)
                {
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
            self.mark_light_dirty_exact(neighbour);
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

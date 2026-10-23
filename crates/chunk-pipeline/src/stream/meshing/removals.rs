use super::super::*;

impl WorldStream {
    /// Publishes and counts ready air and withdrawals without traversing geometry ingress.
    pub(in crate::stream) fn dispatch_mesh_removals(
        &mut self,
        view: SchedulerView,
        removal_budget: usize,
    ) -> usize {
        if self.service_yield_requested() {
            return 0;
        }
        let mut removal_candidates = Vec::new();
        let removal_authority = self
            .publication_allowance
            .as_ref()
            .map_or(removal_budget, |allowance| {
                allowance.zero_byte_admission_capacity_with_priority(true)
            })
            .min(removal_budget)
            .min(MAX_PENDING_MESH_CHANGES.saturating_sub(self.mesh_changes.len()));
        self.mesh_jobs.wake_lane(
            MESH_REMOVAL_LANE,
            view,
            removal_authority,
            self.poll_deadline,
        );
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
            } else if self.mesh_requires_geometry(key) {
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
        let mut removed = 0;
        while let Some((candidate, pending)) = removal_candidates.next() {
            if removed != 0 && self.poll_budget_exhausted() {
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
            removed += 1;
            self.air_removals.remove(&key);
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
        removed
    }
    /// Moves due air sections from the keyed set into the removal lane, where removals order by
    /// priority under the poll deadline. Bounded attempts resume after the last key examined, so
    /// stale entries are cleared and sections that need geometry again return to the worker queue
    /// whatever the removal credit.
    fn route_air_removals(&mut self, view: SchedulerView, removal_authority: usize) {
        self.prune_air_requeued();
        let mut due = Vec::new();
        let entry = |(key, revision): (&SubChunkKey, &u64)| (*key, *revision);
        let scan_budget = removal_authority
            .max(MAX_PENDING_SCHEDULER_SCANS_PER_POLL)
            .min(MAX_PENDING_MESH_QUEUE_WORK_PER_POLL);
        let keys = match self.air_removal_cursor.take() {
            Some(start) => self
                .air_removals
                .range((std::ops::Bound::Excluded(start), std::ops::Bound::Unbounded))
                .chain(self.air_removals.range(..=start))
                .map(entry)
                .take(scan_budget)
                .collect::<Vec<_>>(),
            None => self
                .air_removals
                .iter()
                .map(entry)
                .take(scan_budget)
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
            // Missing and newer revisions no longer belong to this entry.
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
}

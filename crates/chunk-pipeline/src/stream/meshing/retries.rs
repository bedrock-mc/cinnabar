use super::super::*;

impl WorldStream {
    /// Offers bounded FIFO retries not already probed, without adding heap records.
    pub(in crate::stream) fn mesh_retry_candidates(
        &mut self,
        view: SchedulerView,
        already_probed: &HashSet<SubChunkKey>,
    ) -> Vec<(PendingSchedulerCandidate, PendingMesh, MeshCandidateOrigin)> {
        let mut candidates = Vec::new();
        let budget = self
            .mesh_jobs
            .retries
            .begin_poll()
            .min(MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
        let now = Instant::now();
        let mut examined = false;
        for _ in 0..budget {
            if examined && self.poll_budget_exhausted() {
                break;
            }
            let Some(retry) = self.mesh_jobs.retries.pop() else {
                break;
            };
            let Some(pending) = self
                .mesh_jobs
                .pending
                .get(&retry.key)
                .copied()
                .filter(|pending| pending.revision == retry.revision)
            else {
                continue;
            };
            if !self.mesh_requires_geometry(retry.key) {
                continue;
            }
            if already_probed.contains(&retry.key) {
                self.mesh_jobs.retries.requeue(retry);
                continue;
            }
            if !self.mesh_jobs.retries.is_due(&retry) {
                self.mesh_jobs.retries.requeue(retry);
                continue;
            }
            let candidate =
                PendingSchedulerCandidate::new(retry.key, retry.revision, view, pending.urgent);
            let ready_leads = self.mesh_jobs.lanes[RESIDENT_MESH_LANE]
                .ready
                .peek()
                .copied()
                .filter(|ready| {
                    self.mesh_jobs
                        .pending
                        .get(&ready.key)
                        .is_some_and(|pending| pending.revision == ready.revision)
                })
                .is_some_and(|mut ready| {
                    ready.refresh_rank(view);
                    ready >= candidate
                });
            if ready_leads {
                self.mesh_jobs.retries.requeue(retry);
                continue;
            }
            examined = true;
            // Output credit is part of readiness; dispatch reserves it again in rank order.
            let ready = self.revisions.is_current(retry.key, retry.revision)
                && !self.mesh_jobs.in_flight.contains_key(&retry.key)
                && !self.mesh_neighbour_is_due(retry.key, now)
                && self.mesh_light_halo(retry.key).is_some()
                && self
                    .authority
                    .terrain()
                    .sub_chunk(retry.key)
                    .is_some_and(|source| {
                        self.mesh_memory
                            .try_admit(
                                &source,
                                self.authority.runtime_assets(),
                                self.authority.network_id_mode(),
                            )
                            .is_some()
                    });
            if ready {
                candidates.push((candidate, pending, MeshCandidateOrigin::Retry));
                self.mesh_jobs.retries.requeue(retry);
            } else {
                self.mesh_jobs.retries.defer(retry);
            }
        }
        candidates
    }
}

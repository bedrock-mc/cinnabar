use super::*;

impl WorldStream {
    pub fn take_mesh_changes(&mut self) -> Vec<WorldMeshChange> {
        self.mesh_changes.drain(..).collect()
    }
    pub fn pop_mesh_change(&mut self) -> Option<WorldMeshChange> {
        self.mesh_changes.pop_front()
    }
    pub fn pending_mesh_change_count(&self) -> usize {
        self.mesh_changes.len()
    }
    pub fn unacknowledged_mesh_count(&self) -> usize {
        self.revisions.entries.len()
    }
    pub fn is_mesh_clean(&self, key: SubChunkKey) -> bool {
        self.resident.contains(&key) && self.revisions.dirty(key).is_none()
    }
    // A rejected change stays intact so the caller can retry without cloning
    // packed streams or adding an allocation to this hot ownership path.
    #[allow(clippy::result_large_err)]
    pub fn retry_mesh_change_front(
        &mut self,
        change: WorldMeshChange,
    ) -> Result<(), WorldMeshChange> {
        if self.mesh_changes.len() >= MAX_PENDING_MESH_CHANGES {
            return Err(change);
        }
        self.mesh_changes.push_front(change);
        Ok(())
    }
    pub fn acknowledge_mesh_upload(
        &mut self,
        key: SubChunkKey,
        generation: u64,
        dirty_since: Instant,
        applied_at: Instant,
    ) {
        let Some(dirty) = self.revisions.dirty(key) else {
            return;
        };
        if dirty.revision != generation || dirty.since != dirty_since {
            return;
        }
        self.stats.max_remesh_latency = self
            .stats
            .max_remesh_latency
            .max(applied_at.saturating_duration_since(dirty_since));
        self.stats.last_mesh_ack_at = Some(
            self.stats
                .last_mesh_ack_at
                .map_or(applied_at, |latest| latest.max(applied_at)),
        );
        self.applied_mesh_generations.insert(key, generation);
        self.revisions.clear_if_current(key, generation);
    }
    pub fn take_committed_controls(&mut self) -> Vec<CommittedControlEvent> {
        self.committed_controls.drain(..).collect()
    }
    pub fn take_fatal_error(&mut self) -> Option<WorldStreamFatalError> {
        self.fatal_error.take()
    }
    pub fn render_players(&self) -> Vec<(&ActorSnapshot, Option<&PlayerProfile>)> {
        self.actors
            .render_players(Some(self.local_player_runtime_id))
    }
    pub fn actor(&self, runtime_id: u64) -> Option<&ActorSnapshot> {
        self.actors.get(runtime_id)
    }
    pub fn actor_count(&self) -> usize {
        self.actors.len()
    }
    pub fn stats(&self) -> WorldStreamStats {
        let completed_decode_results = self
            .heavy_sequences
            .len()
            .saturating_sub(self.pending_decode.len())
            .saturating_sub(self.in_flight_decode_jobs);
        let [
            adjudicated_static_block_entities,
            adjudicated_logical_block_entities,
            deferred_block_entities,
            unknown_block_entities,
        ] = self.block_entity_visuals.counts();
        WorldStreamStats {
            received_radius_chunks: self.chunk_radius,
            publisher_radius_chunks: self.publisher_radius_chunks,
            resident_sub_chunks: self.resident.len(),
            adjudicated_static_block_entities,
            adjudicated_logical_block_entities,
            deferred_block_entities,
            unknown_block_entities,
            pending_mesh_jobs: self.pending_mesh.len(),
            in_flight_mesh_jobs: self.in_flight.len(),
            pending_light_jobs: self.pending_light.len(),
            in_flight_light_jobs: self.in_flight_light.len(),
            terminal_light_failures: self.light_failures.len(),
            admitted_world_events: self.submitted.len(),
            admitted_heavy_events: self.heavy_sequences.len(),
            queued_decode_jobs: self.pending_decode.len(),
            in_flight_decode_jobs: self.in_flight_decode_jobs,
            completed_decode_results,
            pending_retry_requests: self.queued_retry_request_count(),
            awaiting_sub_chunk_responses: self.sub_chunk_deadlines.len(),
            ..self.stats
        }
    }
    pub fn begin_timed_session(&mut self) {
        self.stats.max_decode_queue_wait = Duration::ZERO;
        self.stats.max_light_queue_wait = Duration::ZERO;
        self.stats.max_mesh_queue_wait = Duration::ZERO;
        self.stats.max_decode_duration = Duration::ZERO;
        self.stats.max_mesh_duration = Duration::ZERO;
        self.stats.max_light_duration = Duration::ZERO;
        self.stats.max_remesh_latency = Duration::ZERO;
        self.stats.last_chunk_commit_at = None;
        self.stats.last_mesh_dispatch_at = None;
        self.stats.last_mesh_completion_at = None;
        self.stats.last_mesh_ack_at = None;
    }
}

impl WorldStream {
    #[must_use]
    pub const fn actor_session_id(&self) -> u64 {
        self.actor_session_id
    }
}

use super::*;

impl WorldStream {
    pub fn set_publication_allowance(&mut self, allowance: PublicationAllowance) {
        self.publication_allowance = Some(allowance);
    }

    pub fn take_mesh_changes(&mut self) -> Vec<WorldMeshChange> {
        let changes = self.mesh_changes.drain(..).collect::<Vec<_>>();
        self.stats.phase2_stages.mesh_changes_dequeued = self
            .stats
            .phase2_stages
            .mesh_changes_dequeued
            .saturating_add(changes.len() as u64);
        changes
    }
    pub fn pop_mesh_change(&mut self) -> Option<WorldMeshChange> {
        let change = self.mesh_changes.pop_front();
        if change.is_some() {
            self.stats.phase2_stages.mesh_changes_dequeued = self
                .stats
                .phase2_stages
                .mesh_changes_dequeued
                .saturating_add(1);
        }
        change
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
        self.stats.phase2_stages.mesh_changes_queued = self
            .stats
            .phase2_stages
            .mesh_changes_queued
            .saturating_add(1);
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
        self.stats.phase2_stages.mesh_uploads_acknowledged = self
            .stats
            .phase2_stages
            .mesh_uploads_acknowledged
            .saturating_add(1);
    }
    pub fn take_committed_controls(&mut self) -> Vec<CommittedControlEvent> {
        self.committed_controls.drain(..).collect()
    }
    pub fn take_committed_ui(&mut self) -> Vec<CommittedUiEvent> {
        self.committed_ui.drain(..).collect()
    }
    pub fn take_fatal_error(&mut self) -> Option<WorldStreamFatalError> {
        self.fatal_error.take()
    }
    pub fn render_players(&self) -> Vec<(&ActorSnapshot, Option<&PlayerProfile>)> {
        self.actors
            .render_players(Some(self.local_player_runtime_id))
    }
    pub fn actor_display_name(&self, unique_id: i64) -> Option<std::sync::Arc<str>> {
        self.actors.actor_display_name(unique_id)
    }
    /// Every username on the retained authoritative player list, sorted.
    pub fn player_list_usernames(&self) -> Vec<std::sync::Arc<str>> {
        self.actors.player_list_usernames()
    }
    /// The authoritative `(current, maximum)` health of the actor with this
    /// unique id, if it is known and well-formed.
    pub fn actor_health_by_unique(&self, unique_id: i64) -> Option<(f32, f32)> {
        self.actors.health_by_unique(unique_id)
    }
    /// Resolves one wire item stack against the retained item registry and
    /// compiled item visual routes.
    pub fn canonical_item_stack(
        &self,
        stack: &protocol::NetworkItemStack,
    ) -> Option<crate::item::CanonicalItemStack> {
        self.actors.canonical_item_stack(stack)
    }
    pub fn advance_actor_interpolation_ticks(&mut self, ticks: u32) {
        self.actors.advance_interpolation_ticks(ticks);
    }
    pub fn actor(&self, runtime_id: u64) -> Option<&ActorSnapshot> {
        self.actors.get(runtime_id)
    }
    pub fn actor_player_profile(&self, runtime_id: u64) -> Option<&PlayerProfile> {
        self.actors.player_profile(runtime_id)
    }
    pub fn actor_rig(&self, runtime_id: u64) -> Option<ActorRigSnapshot<'_>> {
        self.actors.actor_rig(runtime_id)
    }
    pub fn actor_rigs(&self) -> Vec<ActorRigSnapshot<'_>> {
        self.actors.actor_rigs()
    }
    pub const fn actor_animation_stats(&self) -> ActorAnimationStats {
        self.actors.animation_stats()
    }
    pub fn actor_equipment(&self, runtime_id: u64) -> Option<&ActorEquipmentSnapshot> {
        self.actors.equipment(runtime_id)
    }
    pub fn actor_action(&self, runtime_id: u64) -> Option<&RemoteActionSnapshot> {
        self.actors.action(runtime_id)
    }
    pub fn actor_action_history(&self, runtime_id: u64) -> &[RemoteActionSnapshot] {
        self.actors.action_history(runtime_id)
    }
    pub const fn actor_action_stats(&self) -> RemoteActionStats {
        self.actors.action_stats()
    }
    pub fn pending_item_resolution_count(&self) -> usize {
        self.actors.pending_item_resolution_count()
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

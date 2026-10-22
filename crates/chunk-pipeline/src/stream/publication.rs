use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
use {super::*, render_api::PublicationAllowance};

impl WorldStream {
    /// Distinguishes proven air data from withdrawing a non-empty column's near drawer.
    pub fn terrain_removal_kind(&self, key: SubChunkKey) -> render_api::TerrainRemovalKind {
        if self.known_air.contains(&key)
            || self
                .authority
                .terrain()
                .sub_chunk(key)
                .is_some_and(|source| self.classifier.is_sub_chunk_air(&source))
        {
            render_api::TerrainRemovalKind::Empty
        } else {
            render_api::TerrainRemovalKind::Withdrawn
        }
    }

    /// Publishes local placement and its transition to retained pose and animation readers.
    pub fn set_local_actor_presentation_position(&mut self, position: [f32; 3], reanchor: bool) {
        self.authority
            .set_local_actor_presentation_position(position, reanchor);
    }

    /// Retains StartGame's level mode for remote-player target admission.
    pub fn set_world_default_game_mode(&mut self, mode: client_world::ingestion::GameModeUpdate) {
        self.authority.set_world_default_game_mode(mode);
    }

    /// Read-only view of the admitted world; mutation stays behind ordered admission.
    #[must_use]
    pub const fn authority(&self) -> &client_world::WorldAuthority {
        &self.authority
    }

    /// Delivers each committed primitive-shape packet once in network order.
    pub fn pop_primitive_shapes(
        &mut self,
    ) -> Option<render_api::primitive_shapes::PrimitiveShapesEvent> {
        self.authority.pop_primitive_shapes()
    }

    pub fn set_publication_allowance(&mut self, allowance: PublicationAllowance) {
        self.publication_allowance = Some(allowance);
    }

    pub fn take_mesh_changes(&mut self) -> Vec<WorldMeshChange> {
        let changes = self.mesh_changes.drain().collect::<Vec<_>>();
        self.stats.publication_stages.mesh_changes_dequeued = self
            .stats
            .publication_stages
            .mesh_changes_dequeued
            .saturating_add(changes.len() as u64);
        changes
    }
    pub fn pop_mesh_change(&mut self) -> Option<WorldMeshChange> {
        let change = self.mesh_changes.pop_front();
        if change.is_some() {
            self.stats.publication_stages.mesh_changes_dequeued = self
                .stats
                .publication_stages
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
        let superseded = self.mesh_changes.push_front(change);
        self.count_queued_mesh_change(superseded);
        Ok(())
    }
    pub(super) fn queue_mesh_change(&mut self, change: WorldMeshChange) {
        let superseded = self.mesh_changes.push(change);
        self.count_queued_mesh_change(superseded);
    }
    /// A superseded change counts as dequeued so queued minus dequeued stays the pending count.
    fn count_queued_mesh_change(&mut self, superseded: bool) {
        let stages = &mut self.stats.publication_stages;
        stages.mesh_changes_queued = stages.mesh_changes_queued.saturating_add(1);
        self.count_discarded_mesh_changes(usize::from(superseded));
    }
    pub(super) fn retain_mesh_changes(&mut self, keep: impl FnMut(&WorldMeshChange) -> bool) {
        let discarded = self.mesh_changes.retain(keep);
        self.count_discarded_mesh_changes(discarded);
    }
    fn count_discarded_mesh_changes(&mut self, discarded: usize) {
        let stages = &mut self.stats.publication_stages;
        stages.mesh_changes_dequeued = stages
            .mesh_changes_dequeued
            .saturating_add(discarded as u64);
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
        // An evicted key's removal ack must not re-enter the map its eviction just pruned.
        if self.resident.contains(&key) || self.known_air.contains(&key) {
            self.applied_mesh_generations.insert(key, generation);
            // A column that wants no mesh was meshed as a removal.
            if self.wants_mesh(key.chunk()) {
                self.withdrawn_meshes.remove(&key);
            } else {
                self.withdrawn_meshes.insert(key);
            }
        } else {
            self.applied_mesh_generations.remove(&key);
            self.withdrawn_meshes.remove(&key);
        }
        self.revisions.clear_if_current(key, generation);
        self.note_near_drawn(key.chunk());
        self.acknowledge_actor_block_syncs(key, generation);
        self.stats.publication_stages.mesh_uploads_acknowledged = self
            .stats
            .publication_stages
            .mesh_uploads_acknowledged
            .saturating_add(1);
    }
    /// Returns undelivered controls to the front without changing their order.
    pub fn restore_committed_controls(
        &mut self,
        controls: impl DoubleEndedIterator<Item = CommittedControlEvent>,
    ) {
        self.authority.restore_committed_controls(controls)
    }

    pub fn take_committed_controls(&mut self) -> Vec<CommittedControlEvent> {
        self.authority.take_committed_controls()
    }
    pub fn take_committed_ui(&mut self) -> Vec<CommittedUiEvent> {
        self.authority.take_committed_ui()
    }
    pub fn take_committed_audio(&mut self) -> Vec<CommittedAudioEvent> {
        self.authority.take_committed_audio()
    }
    pub fn take_committed_particles(&mut self) -> Vec<CommittedParticleEvent> {
        self.authority.take_committed_particles()
    }
    pub fn take_committed_camera(&mut self) -> Vec<CommittedCameraEvent> {
        self.authority.take_committed_camera()
    }
    pub fn take_fatal_error(&mut self) -> Option<WorldStreamFatalError> {
        self.fatal_error.take()
    }
    /// Installs the StartGame item registry so server-defined item ids resolve
    /// before any play-time registry arrives. False when it is refused.
    pub fn seed_item_registry(
        &mut self,
        registry: client_world::ingestion::ItemRegistryEvent,
    ) -> bool {
        self.authority.seed_item_registry(registry)
    }
    /// Advances simulation ticks with one visual evaluation per tick.
    pub fn advance_actor_interpolation_ticks(&mut self, ticks: u32) {
        self.authority.advance_actor_interpolation_ticks(ticks)
    }
    /// Predicts remote actor positions for this frame's picks without advancing them.
    pub fn predict_remote_actor_motion(&mut self, ticks: u32) {
        self.authority.predict_remote_actor_motion(ticks)
    }
    /// Advances elapsed tick state, evaluating animation once for this rendered frame.
    pub fn advance_actor_interpolation_frame(&mut self, ticks: u32) {
        self.authority.advance_actor_interpolation_frame(ticks)
    }
    /// Drains decoded actor status events (hurt, death, taming, totem, ...) for particle and sound consumers.
    pub fn take_actor_status_notices(&mut self) -> Vec<client_world::ActorStatusNotice> {
        self.authority.take_actor_status_notices()
    }
    /// Drains where MobEquipment and MobArmorEquipment events landed, for diagnostics.
    pub fn take_equipment_notices(&mut self) -> Vec<client_world::EquipmentNotice> {
        self.authority.take_equipment_notices()
    }

    /// Records the `(runtime_id, degrees)` bed orientation that backs `query.sleep_rotation`.
    pub fn set_actor_bed_rotations(&mut self, samples: &[(u64, f32)]) {
        self.authority.set_actor_bed_rotations(samples)
    }
    /// Records `(runtime_id, in_water, in_lava)` samples that back the fluid animation queries.
    pub fn set_actor_fluids(&mut self, samples: &[(u64, bool, bool)]) {
        self.authority.set_actor_fluids(samples)
    }
    /// Records `(runtime_id, submerged)` breathing-point samples that hide entity shadows.
    pub fn set_actor_breathing_liquids(&mut self, samples: &[(u64, bool)]) {
        self.authority.set_actor_breathing_liquids(samples)
    }
    /// Sets the view `[pitch, yaw]` (degrees) that camera-facing billboard rigs sample per tick.
    pub fn set_actor_camera_rotation(&mut self, rotation: [f32; 2]) {
        self.authority.set_actor_camera_rotation(rotation)
    }
    /// Activates or clears the current world's optional native totem-use counting interval.
    pub fn set_totem_tracking(&mut self, enabled: bool) {
        self.authority.set_totem_tracking(enabled);
    }
    /// Sets the view outside which rigs hold their pose at each tick; `None` animates all.
    pub fn set_actor_animation_view(&mut self, view: Option<actor_animation::ActorAnimationView>) {
        self.authority.set_actor_animation_view(view)
    }
    /// Sets the view's world position that camera-relative queries sample per tick.
    pub fn set_actor_camera_position(&mut self, position: [f32; 3]) {
        self.authority.set_actor_camera_position(position)
    }
    /// Feeds this frame's client-authored local-player pose into the shared actor rig. Call
    /// before advancing interpolation and reading rigs so the third-person body and
    /// first-person hand read a driven rig instead of a static fallback.
    pub fn sync_local_player_pose(&mut self, feed: &LocalPlayerFeed) {
        self.authority.sync_local_player_pose(feed)
    }

    /// Publishes a client-selected appearance against the retained local player profile.
    pub fn update_local_player_skin(&mut self, profile: &client_world::PlayerProfile) -> bool {
        self.authority
            .update_local_player_skin(profile.skin.clone())
    }
    /// Arm swings the server sent for the local player since the last call.
    pub fn take_local_server_swings(&mut self) -> Vec<protocol::SwingSource> {
        self.authority.take_local_server_swings()
    }
    /// Starts the local arm swing lasting `ticks`, the duration its packet guard used.
    pub fn start_local_player_swing(&mut self, ticks: i32) {
        self.authority.start_local_player_swing(ticks)
    }
    /// Uses committed local swing samples without re-admitting them on the remote actor clock.
    pub fn sync_local_swing(&mut self, progress: actor_animation::LocalSwingProgress) {
        self.authority.sync_local_swing(progress);
    }
    /// Selects the local motion authority before the actor clock advances unrelated animation.
    pub fn set_local_motion_authority(
        &mut self,
        authority: Option<(SessionGeneration, PositionAuthorityGeneration)>,
    ) {
        self.authority.set_local_motion_authority(authority);
    }
    /// Advances local torso motion with each admitted physical tick's matching swing samples.
    pub fn sync_local_swing_motion(
        &mut self,
        authority: (SessionGeneration, PositionAuthorityGeneration),
        samples: impl IntoIterator<Item = actor_animation::LocalSwingMotionSample>,
    ) {
        self.authority.sync_local_swing_motion(authority, samples);
    }
    /// Drops the local player's Java equip progress to zero, as a block placement does.
    pub fn reset_local_java_equip(&mut self) {
        self.authority.reset_local_java_equip()
    }
    /// Installs the compiler receiving each packet stop expression and its declared version.
    pub fn set_server_animation_compiler(
        &mut self,
        compiler: actor_animation::ServerAnimationCompiler,
    ) {
        self.authority.set_server_animation_compiler(compiler);
    }

    /// Item use durations (ticks by identifier) that drive `query.main_hand_item_max_duration`.
    /// Layers the session's server-pack entity catalog over the vanilla one; its entities
    /// win by identifier for actors spawned afterwards.
    pub fn set_pack_entities(
        &mut self,
        assets: Option<(std::sync::Arc<assets::RuntimeEntityAssets>, Vec<u32>)>,
    ) {
        self.authority.set_pack_entities(assets)
    }

    /// Seeds `query.property` definitions from pack behavior defaults for entity types the
    /// server has not synced.
    pub fn seed_property_defaults(
        &mut self,
        types: &[(std::sync::Arc<str>, Vec<client_world::PropertyDefault>)],
    ) {
        self.authority.seed_property_defaults(types)
    }

    /// Installs compiled item-use overlays before numerical admission.
    pub fn set_item_use_overlays(
        &mut self,
        overlays: std::sync::Arc<
            std::collections::BTreeMap<Box<str>, assets::equipment::CompiledItemUseOverlay>,
        >,
    ) {
        self.authority.set_item_use_overlays(overlays)
    }

    pub fn set_item_use_durations(
        &mut self,
        durations: std::sync::Arc<std::collections::BTreeMap<Box<str>, u32>>,
    ) {
        self.authority.set_item_use_durations(durations)
    }
    /// Installs effective item-directed attack and kinetic presentation facts.
    pub fn set_item_attack_timings(
        &mut self,
        timings: std::sync::Arc<
            std::collections::BTreeMap<Box<str>, client_world::ItemAttackTiming>,
        >,
    ) {
        self.authority.set_item_attack_timings(timings)
    }
    /// Pending and in-flight mesh job counts, the `pending_mesh_jobs` and `in_flight_mesh_jobs`
    /// of [`Self::stats`] without its request-queue walks, for per-frame callers.
    #[must_use]
    pub fn mesh_job_counts(&self) -> (usize, usize) {
        (self.mesh_jobs.pending.len(), self.mesh_jobs.in_flight.len())
    }
    pub fn stats(&self) -> WorldStreamStats {
        let completed_decode_results = self
            .order
            .heavy_count()
            .saturating_sub(self.pending_decode.len())
            .saturating_sub(self.in_flight_decode_jobs);
        let [
            adjudicated_static_block_entities,
            adjudicated_logical_block_entities,
            deferred_block_entities,
            unknown_block_entities,
        ] = self.block_entity_visuals.counts();
        WorldStreamStats {
            audio_nondefault_camera_observed: self.authority.audio_nondefault_camera_observed(),
            received_radius_chunks: self.chunk_radius,
            publisher_radius_chunks: self.publisher.radius_chunks,
            resident_sub_chunks: self.resident.len(),
            adjudicated_static_block_entities,
            adjudicated_logical_block_entities,
            deferred_block_entities,
            unknown_block_entities,
            pending_mesh_jobs: self.mesh_jobs.pending.len(),
            in_flight_mesh_jobs: self.mesh_jobs.in_flight.len(),
            pending_light_jobs: self.lighting.jobs.pending.len(),
            in_flight_light_jobs: self.lighting.jobs.in_flight.len(),
            terminal_light_failures: self.lighting.failures.len(),
            admitted_world_events: self.order.admitted_count(),
            admitted_heavy_events: self.order.heavy_count(),
            committed_audio_events: self.authority.committed_audio_count(),
            committed_camera_events: self.authority.committed_camera_count(),
            queued_decode_jobs: self.pending_decode.len(),
            in_flight_decode_jobs: self.in_flight_decode_jobs,
            completed_decode_results,
            pending_retry_requests: self.requests.queued_retry_count(),
            awaiting_sub_chunk_responses: self.requests.deadlines.len(),
            ..self.stats
        }
    }
    pub fn begin_timed_session(&mut self) {
        self.stats.max_decode_queue_wait = Duration::ZERO;
        self.stats.max_light_queue_wait = Duration::ZERO;
        self.stats.max_mesh_queue_wait = Duration::ZERO;
        self.stats.max_mesh_dispatch_wait = Duration::ZERO;
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

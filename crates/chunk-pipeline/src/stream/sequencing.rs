use {super::*, render_api::MAX_VIEW_RADIUS_CHUNKS};

pub(super) fn chunk_commit_is_mutation_failure(error: &DecodeError) -> bool {
    matches!(error, DecodeError::CollisionRevision(_))
}

impl WorldStream {
    pub(super) fn record_normalization_error(&mut self, reason: NormalizationErrorReason) {
        self.stats.normalization_errors = self.stats.normalization_errors.saturating_add(1);
        self.stats.normalization_reasons.record(reason);
    }
    /// Commits prepared block mutations and invalidates what they changed.
    pub(super) fn commit_block_mutations(
        &mut self,
        prepared: Vec<PreparedSubChunkMutation>,
    ) -> bool {
        let relight = prepared
            .iter()
            .filter(|mutation| mutation.changed())
            .filter_map(|mutation| {
                self.block_light_semantics_changed(
                    self.authority
                        .terrain()
                        .sub_chunk(mutation.key())
                        .as_deref(),
                    mutation.replacement(),
                )
                .then_some(mutation.key())
            })
            .collect::<BTreeSet<_>>();
        self.commit_block_mutations_with_relight(prepared, &relight)
    }

    /// Publishes an atomic prepared batch using its already computed light summary.
    pub(super) fn commit_block_mutations_with_relight(
        &mut self,
        prepared: Vec<PreparedSubChunkMutation>,
        relight: &BTreeSet<SubChunkKey>,
    ) -> bool {
        self.diagnose_light_mutations(&prepared, relight);
        let Ok(changed) = self.authority.commit_prepared_block_updates(prepared) else {
            return false;
        };
        let now = Instant::now();
        for key in changed {
            self.reconcile_block_crack_column(key.chunk());
            self.refresh_block_entity_visuals_for_sub_chunk(key);
            self.sync_resident(key);
            self.mark_live_mutation_changed(key, now, relight.contains(&key));
        }
        true
    }
    pub(super) fn apply_prepared(&mut self, event: PreparedWorldEvent) {
        self.apply_prepared_with_sequence(event, None);
    }
    pub(super) fn apply_prepared_with_sequence(
        &mut self,
        event: PreparedWorldEvent,
        sequence: Option<u64>,
    ) {
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("stream.commit", sequence).entered();
        match event {
            PreparedWorldEvent::InlineLevelChunk {
                event,
                decoded,
                duration,
            } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                let key = ChunkKey::new(event.dimension, event.x, event.z);
                if !self.column_is_data_interesting(key) {
                    self.record_normalization_error(NormalizationErrorReason::InactiveInlineChunk);
                    return;
                }
                // Cohort membership follows the request-mode ordering
                // contract exactly: only after the data-interest gate
                // above and the submit-time supported-dimension
                // admission.
                self.record_required_level_chunk(&event);
                self.record_column_arrival(key, Instant::now());
                let range = self
                    .authority
                    .dimension_range(event.dimension)
                    .expect("inline events are range-checked before decode");
                let stored_keys = decoded
                    .sub_chunks()
                    .map(|(y, _)| SubChunkKey::from_chunk(key, y))
                    .collect::<BTreeSet<_>>();
                // Vanilla reads every slot the payload left empty as air.
                let new_keys = (0..range.sub_chunk_count)
                    .map(|offset| {
                        SubChunkKey::from_chunk(key, range.base_sub_chunk_y + offset as i32)
                    })
                    .collect::<BTreeSet<_>>();
                let air_keys = new_keys
                    .difference(&stored_keys)
                    .copied()
                    .collect::<BTreeSet<_>>();
                let old_keys = self.resident.column(key).copied().collect::<BTreeSet<_>>();
                let old_air = self.known_air.column(key).copied().collect::<BTreeSet<_>>();
                let Ok(applied) = self.authority.commit_level_chunk(key, decoded) else {
                    self.record_normalization_error(NormalizationErrorReason::BlockMutationFailure);
                    return;
                };
                self.diagnose_inline_column(&event, &stored_keys);
                self.reconcile_block_crack_column(key);
                self.loaded_columns.insert(key);
                self.requests.purge_columns(&BTreeSet::from([key]));
                for old in &old_keys {
                    self.resident.remove(old);
                }
                for old in &old_air {
                    self.known_air.remove(old);
                }
                for stale in old_keys.difference(&new_keys) {
                    self.set_connectivity(*stale, None);
                }
                for no_longer_air in old_air.difference(&air_keys) {
                    self.set_connectivity(*no_longer_air, None);
                }
                self.resident.extend(new_keys.iter().copied());
                for air in air_keys {
                    self.record_known_air(air);
                }
                self.refresh_block_entity_visuals_for_chunk(key);
                let now = Instant::now();
                let preexpanded_dirty = applied.dirty;
                let mut changed_sources = applied.changed.into_iter().collect::<BTreeSet<_>>();
                changed_sources.extend(new_keys.difference(&old_keys).copied());
                changed_sources.extend(old_keys.difference(&new_keys).copied());
                self.mark_changed_sources_with_mesh_dirty(changed_sources, preexpanded_dirty, now);
                self.stats.last_chunk_commit_at = Some(now);
            }
            PreparedWorldEvent::RequestLevelChunk {
                event,
                decoded,
                duration,
            } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                self.apply_request_level_chunk(event, decoded, sequence);
            }
            PreparedWorldEvent::SubChunks {
                dimension,
                entries,
                duration,
            } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                let mut committed_any = false;
                for entry in entries {
                    let key = SubChunkKey::new(
                        dimension,
                        entry.position[0],
                        entry.position[1],
                        entry.position[2],
                    );
                    if !self.column_is_data_interesting(key.chunk()) {
                        self.stats.phase2_outcomes.stale =
                            self.stats.phase2_outcomes.stale.saturating_add(1);
                        continue;
                    }
                    let admitted = self.requests.consume_admitted_reply(key);
                    if !self.requests.is_expected(key) {
                        self.stats.phase2_outcomes.stale =
                            self.stats.phase2_outcomes.stale.saturating_add(1);
                        if admitted && self.requests.consume_correlated_attempt(key) {
                            continue;
                        }
                        self.record_normalization_error(
                            NormalizationErrorReason::UnexpectedSubChunk,
                        );
                        continue;
                    }
                    self.requests.consume_confirmed_attempt(key);
                    self.requests.disarm_deadline(key);
                    let mut arrival_source =
                        self.diagnose_sub_chunk_reply(key, &entry.result, entry.diagnostics);
                    let (completed, committed) = match entry.result {
                        PreparedSubChunkResult::Decoded(decoded) => {
                            self.stats.phase2_outcomes.success =
                                self.stats.phase2_outcomes.success.saturating_add(1);
                            let decoded_air = decoded.sub_chunk().has_no_storages();
                            let committed =
                                match self.authority.commit_decoded_sub_chunk(key, decoded) {
                                    Ok(Some(changed)) => {
                                        if decoded_air {
                                            self.record_known_air(changed);
                                        } else {
                                            self.sync_resident(changed);
                                        }
                                        self.mark_changed(changed, Instant::now());
                                        true
                                    }
                                    Ok(None) => {
                                        if decoded_air && self.record_known_air(key) {
                                            self.mark_changed(key, Instant::now());
                                        }
                                        true
                                    }
                                    Err(error) => {
                                        if chunk_commit_is_mutation_failure(&error) {
                                            self.record_normalization_error(
                                                NormalizationErrorReason::BlockMutationFailure,
                                            );
                                        } else {
                                            self.stats.decode_errors =
                                                self.stats.decode_errors.saturating_add(1);
                                        }
                                        false
                                    }
                                };
                            (true, committed)
                        }
                        PreparedSubChunkResult::AllAir => {
                            self.stats.phase2_outcomes.all_air =
                                self.stats.phase2_outcomes.all_air.saturating_add(1);
                            match self.authority.apply_all_air(key) {
                                Ok(changed) => {
                                    let became_known = self.record_known_air(key);
                                    if changed.is_some() || became_known {
                                        self.mark_changed(key, Instant::now());
                                    }
                                    (true, true)
                                }
                                Err(_) => {
                                    self.record_normalization_error(
                                        NormalizationErrorReason::BlockMutationFailure,
                                    );
                                    (true, false)
                                }
                            }
                        }
                        PreparedSubChunkResult::Unavailable(unavailable) => {
                            self.stats.phase2_outcomes.unavailable =
                                self.stats.phase2_outcomes.unavailable.saturating_add(1);
                            self.stats.unavailable_sub_chunks =
                                self.stats.unavailable_sub_chunks.saturating_add(1);
                            match unavailable {
                                client_world::ingestion::SubChunkUnavailable::InvalidDimension => {
                                    self.record_normalization_error(
                                        NormalizationErrorReason::InvalidDimensionSubChunk,
                                    );
                                    (true, false)
                                }
                                client_world::ingestion::SubChunkUnavailable::ChunkNotFound
                                | client_world::ingestion::SubChunkUnavailable::PlayerNotFound => {
                                    (self.retry_or_complete_sub_chunk(key), false)
                                }
                                // Vanilla writes nothing; an empty slot lights as air.
                                client_world::ingestion::SubChunkUnavailable::YIndexOutOfBounds => {
                                    if self.authority.terrain().sub_chunk(key).is_none()
                                        && self.record_known_air(key)
                                    {
                                        self.mark_changed(key, Instant::now());
                                    } else {
                                        // An out-of-bounds reply preserves data already known here.
                                        arrival_source = None;
                                    }
                                    (true, true)
                                }
                                client_world::ingestion::SubChunkUnavailable::Undefined
                                | client_world::ingestion::SubChunkUnavailable::Unknown(_) => {
                                    (true, false)
                                }
                            }
                        }
                    };
                    committed_any |= committed;
                    if committed {
                        self.diagnose_sub_chunk_commit(key, arrival_source);
                        self.record_sub_chunk_arrival(key, Instant::now());
                        self.stats.phase2_stages.subchunks_committed = self
                            .stats
                            .phase2_stages
                            .subchunks_committed
                            .saturating_add(1);
                    }
                    if committed {
                        self.refresh_block_entity_visuals_for_sub_chunk(key);
                        self.reconcile_block_crack_column(key.chunk());
                    }
                    if completed {
                        self.complete_requested_sub_chunk(key, committed);
                    }
                }
                if committed_any {
                    let now = Instant::now();
                    self.stats.last_chunk_commit_at = Some(now);
                }
            }
            PreparedWorldEvent::BlockUpdates { result, duration } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                match result {
                    Ok(prepared) => {
                        if !self.commit_block_mutations_with_relight(
                            prepared.mutations,
                            &prepared.relight,
                        ) {
                            self.record_normalization_error(
                                NormalizationErrorReason::BlockMutationFailure,
                            );
                        }
                    }
                    Err(_) => {
                        self.record_normalization_error(
                            NormalizationErrorReason::BlockMutationFailure,
                        );
                    }
                }
            }
            PreparedWorldEvent::SyncedBlockUpdates {
                result,
                events,
                duration,
            } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                match result {
                    Ok(prepared) => {
                        if !self.commit_block_mutations_with_relight(
                            prepared.mutations,
                            &prepared.relight,
                        ) {
                            self.record_normalization_error(
                                NormalizationErrorReason::BlockMutationFailure,
                            );
                        } else {
                            self.queue_actor_block_syncs(events);
                        }
                    }
                    Err(_) => self
                        .record_normalization_error(NormalizationErrorReason::BlockMutationFailure),
                }
            }
            PreparedWorldEvent::BlockEntityUpdate {
                key,
                decoded,
                duration,
            } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                if !block_entity_y_is_valid(self.authority.dimension_range(key.dimension), key.y) {
                    self.record_normalization_error(
                        NormalizationErrorReason::InvalidBlockEntityPosition,
                    );
                    return;
                }
                if !self.column_is_data_interesting(key.chunk()) {
                    self.record_normalization_error(
                        NormalizationErrorReason::InactiveBlockEntityUpdate,
                    );
                    return;
                }
                match decoded {
                    Ok(nbt) => match self.authority.commit_block_entity_update(key, nbt) {
                        Ok(true) => self.refresh_block_entity_visual(key),
                        Ok(false) => {}
                        Err(_) => {
                            self.stats.decode_errors = self.stats.decode_errors.saturating_add(1);
                        }
                    },
                    Err(_) => self.stats.decode_errors = self.stats.decode_errors.saturating_add(1),
                }
            }
            PreparedWorldEvent::Immediate(event) => self.apply_immediate(event, sequence),
            PreparedWorldEvent::CommitOnly => {}
            PreparedWorldEvent::NormalizationFailure => {
                self.record_normalization_error(NormalizationErrorReason::EmptySubChunkBatch);
            }
        }
    }
    pub(super) fn apply_immediate(&mut self, event: WorldEvent, sequence: Option<u64>) {
        match event {
            WorldEvent::DimensionHeights(heights) => {
                self.light_diagnostics.heights = heights;
            }
            WorldEvent::BiomeDefinitions(event) => {
                self.replace_biome_definitions(event.definitions);
            }
            WorldEvent::LevelChunk(_) => {
                unreachable!("LevelChunk packets are prepared on workers")
            }
            WorldEvent::ChunkResync(event) => {
                let Some(range) = self.authority.dimension_range(event.dimension) else {
                    if let Some(sequence) = sequence {
                        self.cancel_request_reservation(sequence);
                    }
                    self.record_normalization_error(
                        NormalizationErrorReason::UnsupportedLevelChunkDimension,
                    );
                    return;
                };
                let key = ChunkKey::new(event.dimension, event.x, event.z);
                if !self.column_is_data_interesting(key) {
                    if let Some(sequence) = sequence {
                        self.cancel_request_reservation(sequence);
                    }
                    self.record_normalization_error(NormalizationErrorReason::InactiveLevelChunk);
                    return;
                }
                if let Some(ys) = event.requested_sub_chunk_ys.as_deref() {
                    self.enqueue_exact_recovery_requests(key, range, ys, sequence);
                } else {
                    let count = event
                        .requested_sub_chunks
                        .unwrap_or(range.sub_chunk_count)
                        .min(range.sub_chunk_count);
                    self.enqueue_request(key, range.base_sub_chunk_y, count, sequence);
                }
            }
            WorldEvent::BlockUpdates(_) | WorldEvent::SyncedBlockUpdates(_) => {
                unreachable!("block-update batches are prepared on workers")
            }
            WorldEvent::BlockEntityUpdate(_) => {
                unreachable!("block-entity updates are prepared on workers")
            }
            WorldEvent::ChunkRadiusUpdated(radius) => {
                if radius < 0 {
                    self.record_normalization_error(NormalizationErrorReason::InvalidChunkRadius);
                    return;
                }
                self.chunk_radius = Some(radius.min(MAX_VIEW_RADIUS_CHUNKS));
                self.reevaluate_chunk_retention();
            }
            WorldEvent::PublisherUpdate(update) => {
                let consumes_local_reset = self.publisher.provisional_rebase;
                self.publisher.center = Some(update.center);
                self.publisher.radius_blocks = Some(update.radius_blocks);
                let cohort = ViewCohort::from_publisher(
                    self.authority.current_dimension(),
                    update.center,
                    update.radius_blocks,
                );
                self.publisher.radius_chunks = Some(cohort.radius.min(MAX_VIEW_RADIUS_CHUNKS));
                if self.publisher.cohort != Some(cohort) {
                    if self.publisher.provisional_rebase {
                        self.publisher.required_columns =
                            std::mem::take(&mut self.publisher.required_columns)
                                .into_iter()
                                .filter(|key| self.column_is_data_interesting(*key))
                                .collect();
                    } else {
                        self.publisher.required_columns.clear();
                    }
                    let Some(next_epoch) = self.publisher.epoch.checked_add(1) else {
                        self.publisher.cohort = None;
                        self.publisher.provisional_rebase = false;
                        self.publisher.required_columns.clear();
                        return;
                    };
                    self.publisher.epoch = next_epoch;
                }
                self.publisher.cohort = Some(cohort);
                self.prune_column_deadlines();
                if consumes_local_reset {
                    self.publisher.local_reset.consumed =
                        self.publisher.local_reset.consumed.saturating_add(1);
                }
                self.publisher.provisional_rebase = false;
            }
            WorldEvent::OpenSign(event) => self.consume_open_sign(event),
            WorldEvent::MapData(event) => self.consume_map_data(&event),
            WorldEvent::BlockEvent(event) => {
                let sequence = sequence.expect("sequenced block events commit through submit");
                self.consume_block_event(sequence, event);
            }
            WorldEvent::ChangeDimension(change) => {
                let sequence = sequence.expect("sequenced dimension changes commit through submit");
                self.dimension_transfer_priority = None;
                self.actor_block_syncs = actor_block_sync::ActorBlockSyncs::default();
                self.replace_block_crack_dimension(sequence);
                self.clear_block_events();
                self.evict_all_resident();
                self.block_entity_visuals.clear();
                self.authority.reset_dimension(sequence, change.dimension);
                let resolved = self.authority.resolve_position(change.position);
                self.local_player_chunk = None;
                self.publisher
                    .reset_for_dimension(resolved.position.map(floor_to_i32));
                self.last_retention_center = None;
                self.last_retention_radius = None;
                self.authority
                    .push_committed_control(CommittedControlEvent::ChangeDimension {
                        sequence,
                        change,
                        resolved,
                    });
            }
            WorldEvent::DimensionChangeAck { .. } => {
                // Native selects the session's local player by subclient and
                // intentionally ignores the action's runtime actor ID.
                self.authority
                    .push_committed_control(CommittedControlEvent::DimensionChangeAck {
                        sequence: sequence
                            .expect("dimension acknowledgement commits through submit"),
                        dimension_epoch: self.authority.form_dimension_epoch(),
                    });
            }
            WorldEvent::Respawn(respawn) => {
                let sequence = sequence.expect("sequenced respawns commit through submit");
                let resolved = if respawn.ready_to_spawn() {
                    let resolved = self.authority.resolve_position(respawn.position);
                    self.local_player_chunk = None;
                    self.provisionally_rebase_for_local_teleport(resolved.position);
                    self.reevaluate_chunk_retention();
                    resolved
                } else {
                    self.authority.resolved_server_position()
                };
                self.authority
                    .push_committed_control(CommittedControlEvent::Respawn {
                        sequence,
                        respawn,
                        resolved,
                    });
            }
            WorldEvent::MovePlayer(movement) => {
                let sequence = sequence.expect("sequenced MovePlayer commits through submit");
                self.authority.apply_player_move(sequence, movement);
                if movement.runtime_id != self.authority.local_player_runtime_id() {
                    return;
                }
                let source_cohort = self.publisher.cohort;
                if self.publisher.source_capture_sequence == Some(sequence) {
                    self.capture_source_columns();
                    self.publisher.source_capture_sequence = None;
                }
                let resolved = self.authority.resolve_position(movement.position);
                // Unmarked moves reconcile like corrections against a past tick; only teleports recenter.
                if movement.mode.is_teleport() {
                    self.local_player_chunk = None;
                    self.provisionally_rebase_for_local_teleport(resolved.position);
                    self.reevaluate_chunk_retention();
                }
                self.authority
                    .push_committed_control(CommittedControlEvent::MovePlayer {
                        sequence,
                        movement,
                        resolved,
                        source_cohort,
                    });
            }
            WorldEvent::PlayerMovementCorrection(correction) => {
                // Vehicle rewind subjects have no local-player consumer yet;
                // skipping here keeps them out of resolution, retention, and
                // the correction-tick guard until riding rewind handling
                // exists. The protocol record itself is retained upstream.
                if !correction.subject.is_player() {
                    return;
                }
                let sequence =
                    sequence.expect("sequenced movement corrections commit through submit");
                if !self
                    .authority
                    .accept_movement_correction_tick(correction.tick)
                {
                    return;
                }
                // A correction names a past tick; retention waits for physics to reconcile it.
                let resolved = self.authority.resolve_position(correction.position);
                self.authority.push_committed_control(
                    CommittedControlEvent::PlayerMovementCorrection {
                        sequence,
                        correction,
                        resolved,
                    },
                );
            }
            WorldEvent::Particle(mut event) => {
                self.remap_particle_block_ids(&mut event);
                let sequence = sequence.expect("sequenced particle events commit through submit");
                self.authority
                    .push_committed_particle(CommittedParticleEvent {
                        sequence,
                        dimension: self.authority.current_dimension(),
                        event,
                    });
            }
            WorldEvent::BlockCrack(event) => {
                let sequence = sequence.expect("sequenced block cracks commit through submit");
                self.consume_block_crack(sequence, event);
                self.authority
                    .push_committed_ui(CommittedUiEvent::BlockCrack {
                        sequence,
                        dimension: self.authority.current_dimension(),
                        event,
                    });
            }
            WorldEvent::SubChunkReplyAdmission(_) => {
                unreachable!("SubChunk reply admissions commit ordering only")
            }
            WorldEvent::SubChunks(_) => unreachable!("sub-chunk batches are prepared on workers"),
            event => self
                .authority
                .apply_ordered_event(event, sequence)
                .expect("terrain events are handled by the coordinator"),
        }
    }
    pub(super) fn apply_request_level_chunk(
        &mut self,
        event: LevelChunkEvent,
        decoded: (DecodedBiomeColumn, DecodedBlockEntities),
        sequence: Option<u64>,
    ) {
        let key = ChunkKey::new(event.dimension, event.x, event.z);
        if !self.column_is_data_interesting(key) {
            self.record_normalization_error(NormalizationErrorReason::InactiveLevelChunk);
            return;
        }
        let Some(range) = self.authority.dimension_range(event.dimension) else {
            self.record_normalization_error(
                NormalizationErrorReason::UnsupportedLevelChunkDimension,
            );
            return;
        };
        self.record_required_level_chunk(&event);
        self.record_column_arrival(key, Instant::now());
        let (count, has_authoritative_upper_air) = match event.mode {
            LevelChunkMode::LimitedRequests { highest } => {
                (usize::from(highest).min(range.sub_chunk_count), true)
            }
            LevelChunkMode::LimitlessRequests => (range.sub_chunk_count, false),
            LevelChunkMode::Inline { .. } => {
                unreachable!("inline LevelChunk packets are prepared on workers")
            }
        };
        let (biomes, block_entities) = decoded;
        if self.authority.terrain().biome_column_matches(key, &biomes) {
            self.loaded_columns.remove(&key);
            self.requests.collision_failures.remove(&key);
            self.requests.purge_columns(&BTreeSet::from([key]));
        } else {
            self.evict_column(key);
        }
        self.diagnose_request_column(&event);
        let biome_dirty = self.authority.commit_biome_column(key, biomes);
        let now = Instant::now();
        for dirty in biome_dirty {
            if self.resident.contains(&dirty) && self.authority.terrain().sub_chunk(dirty).is_some()
            {
                self.mark_dirty_exact(dirty, now);
            }
        }
        self.authority
            .commit_chunk_block_entities(key, block_entities);
        self.refresh_block_entity_visuals_for_chunk(key);
        self.enqueue_request(key, range.base_sub_chunk_y, count, sequence);
        if has_authoritative_upper_air {
            let first_air_y = range
                .base_sub_chunk_y
                .saturating_add(i32::try_from(count).expect("vanilla subchunk count fits i32"));
            let end_y = range.base_sub_chunk_y.saturating_add(
                i32::try_from(range.sub_chunk_count).expect("vanilla subchunk count fits i32"),
            );
            let mut changed = BTreeSet::new();
            for y in first_air_y..end_y {
                let air = SubChunkKey::from_chunk(key, y);
                let Ok(removed) = self.authority.apply_request_mode_air(air) else {
                    self.record_normalization_error(NormalizationErrorReason::BlockMutationFailure);
                    return;
                };
                self.reconcile_block_crack_column(key);
                let removed = removed.is_some();
                let became_known = self.record_known_air(air);
                self.diagnose_request_air_commit(air);
                if removed {
                    self.refresh_block_entity_visuals_for_sub_chunk(air);
                }
                if removed || became_known {
                    changed.insert(air);
                }
            }
            self.mark_changed_sources(changed, Instant::now());
        }
    }

    fn record_required_level_chunk(&mut self, event: &LevelChunkEvent) {
        let key = ChunkKey::new(event.dimension, event.x, event.z);
        if (self.publisher.cohort.is_some() || self.publisher.provisional_rebase)
            && self.column_is_data_interesting(key)
        {
            self.publisher.required_columns.insert(key);
        }
    }
}

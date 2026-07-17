use super::*;

impl WorldStream {
    pub(super) fn record_normalization_error(&mut self, reason: NormalizationErrorReason) {
        self.stats.normalization_errors = self.stats.normalization_errors.saturating_add(1);
        self.stats.normalization_reasons.record(reason);
    }
    pub(super) fn apply_ready(&mut self) {
        if self.blocking_block_updates.is_some() {
            return;
        }
        while let Some(event) = self.ordered.pop_next() {
            let sequence = self.ordered.next_sequence().saturating_sub(1);
            match event {
                PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(events)) => {
                    let batches = self.snapshot_block_mutation_batches(events);
                    if batches.is_empty() {
                        self.submitted.remove(&sequence);
                        self.heavy_sequences.remove(&sequence);
                        continue;
                    }
                    self.enqueue_decode_job(DecodeJob::BlockUpdates {
                        sequence,
                        batches,
                        air_runtime_id: self.classifier.air_network_id(),
                    });
                    self.blocking_block_updates = Some(sequence);
                    break;
                }
                event => {
                    self.submitted.remove(&sequence);
                    self.heavy_sequences.remove(&sequence);
                    self.apply_prepared_with_sequence(event, Some(sequence));
                    self.cancel_request_reservation(sequence);
                }
            }
        }
    }
    pub(super) fn apply_prepared(&mut self, event: PreparedWorldEvent) {
        self.apply_prepared_with_sequence(event, None);
    }
    pub(super) fn apply_prepared_with_sequence(
        &mut self,
        event: PreparedWorldEvent,
        sequence: Option<u64>,
    ) {
        match event {
            PreparedWorldEvent::InlineLevelChunk {
                event,
                decoded,
                duration,
            } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                let key = ChunkKey::new(event.dimension, event.x, event.z);
                if !self.column_is_active(key) {
                    self.record_normalization_error(NormalizationErrorReason::InactiveInlineChunk);
                    return;
                }
                match decoded {
                    Ok(decoded) => {
                        let range = vanilla_dimension_range(event.dimension)
                            .expect("inline events are range-checked before decode");
                        let count = match event.mode {
                            LevelChunkMode::Inline { count } => count,
                            _ => unreachable!("prepared LevelChunk must be inline"),
                        };
                        let stored_keys = decoded
                            .sub_chunks()
                            .map(|(y, _)| SubChunkKey::from_chunk(key, y))
                            .collect::<BTreeSet<_>>();
                        let new_keys = (0..count)
                            .map(|offset| {
                                SubChunkKey::from_chunk(key, range.base_sub_chunk_y + offset as i32)
                            })
                            .collect::<BTreeSet<_>>();
                        let air_keys = new_keys
                            .difference(&stored_keys)
                            .copied()
                            .collect::<BTreeSet<_>>();
                        let old_keys = self
                            .resident
                            .iter()
                            .copied()
                            .filter(|resident| resident.chunk() == key)
                            .collect::<BTreeSet<_>>();
                        let old_air = self
                            .known_air
                            .iter()
                            .copied()
                            .filter(|resident| resident.chunk() == key)
                            .collect::<BTreeSet<_>>();
                        let applied = self.store.commit_level_chunk(key, decoded);
                        self.loaded_columns.insert(key);
                        self.purge_sub_chunk_column_state(key);
                        self.resident.retain(|resident| resident.chunk() != key);
                        self.known_air.retain(|resident| resident.chunk() != key);
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
                        let mut changed_sources =
                            applied.changed.into_iter().collect::<BTreeSet<_>>();
                        changed_sources.extend(new_keys.difference(&old_keys).copied());
                        changed_sources.extend(old_keys.difference(&new_keys).copied());
                        self.mark_changed_sources_with_mesh_dirty(
                            changed_sources,
                            preexpanded_dirty,
                            now,
                        );
                        self.stats.last_chunk_commit_at = Some(now);
                    }
                    Err(_) => self.stats.decode_errors = self.stats.decode_errors.saturating_add(1),
                }
            }
            PreparedWorldEvent::RequestLevelChunk {
                event,
                decoded,
                duration,
            } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                match decoded {
                    Ok(decoded) => self.apply_request_level_chunk(event, decoded, sequence),
                    Err(_) => self.stats.decode_errors = self.stats.decode_errors.saturating_add(1),
                }
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
                    if !self.column_is_active(key.chunk()) {
                        self.stats.phase2_outcomes.stale =
                            self.stats.phase2_outcomes.stale.saturating_add(1);
                        continue;
                    }
                    let admitted = self.consume_admitted_sub_chunk_reply(key);
                    if !self.is_expected_sub_chunk(key) {
                        self.stats.phase2_outcomes.stale =
                            self.stats.phase2_outcomes.stale.saturating_add(1);
                        if admitted && self.consume_correlated_sub_chunk_attempt(key) {
                            continue;
                        }
                        self.record_normalization_error(
                            NormalizationErrorReason::UnexpectedSubChunk,
                        );
                        continue;
                    }
                    self.consume_confirmed_sub_chunk_attempt(key);
                    self.disarm_sub_chunk_deadline(key);
                    let (completed, committed) = match entry.result {
                        PreparedSubChunkResult::Decoded(Ok(decoded)) => {
                            self.stats.phase2_outcomes.success =
                                self.stats.phase2_outcomes.success.saturating_add(1);
                            let decoded_air = decoded.sub_chunk().has_no_storages();
                            let committed = match self.store.commit_decoded_sub_chunk(key, decoded)
                            {
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
                                Err(_) => {
                                    self.stats.decode_errors =
                                        self.stats.decode_errors.saturating_add(1);
                                    false
                                }
                            };
                            (true, committed)
                        }
                        PreparedSubChunkResult::Decoded(Err(_)) => {
                            self.stats.phase2_outcomes.malformed =
                                self.stats.phase2_outcomes.malformed.saturating_add(1);
                            self.stats.decode_errors = self.stats.decode_errors.saturating_add(1);
                            (self.retry_or_complete_sub_chunk(key), false)
                        }
                        PreparedSubChunkResult::AllAir => {
                            self.stats.phase2_outcomes.all_air =
                                self.stats.phase2_outcomes.all_air.saturating_add(1);
                            let changed = self.store.apply_all_air(key);
                            let became_known = self.record_known_air(key);
                            if changed.is_some() || became_known {
                                self.mark_changed(key, Instant::now());
                            }
                            (true, true)
                        }
                        PreparedSubChunkResult::Unavailable(unavailable) => {
                            self.stats.phase2_outcomes.unavailable =
                                self.stats.phase2_outcomes.unavailable.saturating_add(1);
                            self.stats.unavailable_sub_chunks =
                                self.stats.unavailable_sub_chunks.saturating_add(1);
                            match unavailable {
                                protocol::SubChunkUnavailable::YIndexOutOfBounds => {
                                    let changed = self.store.apply_all_air(key);
                                    let became_known = self.record_known_air(key);
                                    if changed.is_some() || became_known {
                                        self.mark_changed(key, Instant::now());
                                    }
                                    (true, true)
                                }
                                protocol::SubChunkUnavailable::InvalidDimension => {
                                    self.record_normalization_error(
                                        NormalizationErrorReason::InvalidDimensionSubChunk,
                                    );
                                    (true, false)
                                }
                                protocol::SubChunkUnavailable::ChunkNotFound
                                | protocol::SubChunkUnavailable::PlayerNotFound => {
                                    (self.retry_or_complete_sub_chunk(key), false)
                                }
                                protocol::SubChunkUnavailable::Undefined
                                | protocol::SubChunkUnavailable::Unknown(_) => (true, false),
                            }
                        }
                    };
                    committed_any |= committed;
                    if committed {
                        self.stats.phase2_stages.subchunks_committed = self
                            .stats
                            .phase2_stages
                            .subchunks_committed
                            .saturating_add(1);
                    }
                    if committed {
                        self.refresh_block_entity_visuals_for_sub_chunk(key);
                    }
                    if completed {
                        self.complete_requested_sub_chunk(key, committed);
                    }
                }
                if committed_any {
                    self.stats.last_chunk_commit_at = Some(Instant::now());
                }
            }
            PreparedWorldEvent::BlockUpdates { result, duration } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                match result {
                    Ok(prepared) => {
                        let changed = self.store.commit_prepared_block_updates(prepared);
                        let now = Instant::now();
                        for key in changed {
                            self.refresh_block_entity_visuals_for_sub_chunk(key);
                            self.sync_resident(key);
                            self.mark_changed(key, now);
                        }
                    }
                    Err(_) => {
                        self.record_normalization_error(
                            NormalizationErrorReason::BlockMutationFailure,
                        );
                    }
                }
            }
            PreparedWorldEvent::BlockEntityUpdate {
                key,
                decoded,
                duration,
            } => {
                self.stats.max_decode_duration = self.stats.max_decode_duration.max(duration);
                if !block_entity_y_is_valid(key.dimension, key.y) {
                    self.record_normalization_error(
                        NormalizationErrorReason::InvalidBlockEntityPosition,
                    );
                    return;
                }
                if !self.column_is_active(key.chunk()) {
                    self.record_normalization_error(
                        NormalizationErrorReason::InactiveBlockEntityUpdate,
                    );
                    return;
                }
                match decoded {
                    Ok(nbt) => match self.store.commit_block_entity_update(key, nbt) {
                        Ok(true) => self.refresh_block_entity_visual(key),
                        Ok(false) => {}
                        Err(_) => {
                            self.stats.decode_errors = self.stats.decode_errors.saturating_add(1);
                        }
                    },
                    Err(_) => {
                        self.stats.decode_errors = self.stats.decode_errors.saturating_add(1);
                    }
                }
            }
            PreparedWorldEvent::Immediate(event) => self.apply_immediate(event, sequence),
            PreparedWorldEvent::NormalizationFailure => {
                self.record_normalization_error(NormalizationErrorReason::EmptySubChunkBatch);
            }
        }
    }
    pub(super) fn apply_immediate(&mut self, event: WorldEvent, sequence: Option<u64>) {
        match event {
            WorldEvent::BiomeDefinitions(event) => {
                let live = event
                    .definitions
                    .iter()
                    .map(|definition| LiveBiomeDefinition {
                        name: definition.name.as_ref(),
                        biome_id: definition.biome_id,
                        temperature: definition.temperature,
                        downfall: definition.downfall,
                        map_water_argb: definition.map_water_color,
                    })
                    .collect::<Vec<_>>();
                let Ok(resolved) = self.runtime_assets.biome_assets().resolve_live(&live) else {
                    self.record_normalization_error(
                        NormalizationErrorReason::BiomeDefinitionResolutionFailure,
                    );
                    return;
                };
                let Some(next_revision) = self.biome_tint_revision.checked_add(1) else {
                    self.record_normalization_error(
                        NormalizationErrorReason::BiomeTintRevisionOverflow,
                    );
                    return;
                };
                self.biome_tint_revision = next_revision;
                self.biome_definitions = event.definitions;
                self.resolved_biome_tints = Arc::new(resolved);
                self.invalidate_resident_biome_tints(Instant::now());
            }
            WorldEvent::LevelChunk(_) => {
                unreachable!("LevelChunk packets are prepared on workers")
            }
            WorldEvent::BlockUpdates(_) => {
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
                self.chunk_radius = Some(radius.min(PHASE0_MAX_VIEW_RADIUS_CHUNKS));
                self.evict_outside_active_radius();
            }
            WorldEvent::PublisherUpdate(update) => {
                self.publisher_center = Some(update.center);
                self.publisher_radius_blocks = Some(update.radius_blocks);
                let cohort = ViewCohort::from_publisher(
                    self.current_dimension,
                    update.center,
                    update.radius_blocks,
                );
                self.publisher_radius_chunks =
                    Some(cohort.radius.min(PHASE0_MAX_VIEW_RADIUS_CHUNKS));
                self.committed_view_cohort = Some(cohort);
                self.evict_outside_active_radius();
            }
            WorldEvent::ChangeDimension(change) => {
                self.evict_all_resident();
                self.block_entity_visuals.clear();
                let sequence = sequence.expect("sequenced dimension changes commit through submit");
                let _ =
                    self.actors
                        .reset_dimension(self.actor_session_id, sequence, change.dimension);
                self.current_dimension = change.dimension;
                let resolved = resolve_server_position(
                    change.position,
                    self.resolved_server_position.position,
                    self.resolved_server_position.surface_anchor,
                );
                self.resolved_server_position = resolved;
                self.publisher_center = Some([
                    floor_to_i32(resolved.position[0]),
                    floor_to_i32(resolved.position[1]),
                    floor_to_i32(resolved.position[2]),
                ]);
                self.publisher_radius_blocks = None;
                self.publisher_radius_chunks = None;
                self.committed_view_cohort = None;
                self.push_committed_control(CommittedControlEvent::ChangeDimension {
                    change,
                    resolved,
                });
            }
            WorldEvent::MovePlayer(movement) => {
                let sequence = sequence.expect("sequenced MovePlayer commits through submit");
                let _ = self.actors.apply_player_move(
                    self.actor_session_id,
                    sequence,
                    self.current_dimension,
                    movement,
                );
                if movement.runtime_id != self.local_player_runtime_id {
                    return;
                }
                let source_cohort = self.committed_view_cohort;
                if self.source_capture_sequence == Some(sequence) {
                    self.capture_source_columns();
                    self.source_capture_sequence = None;
                }
                let resolved = resolve_server_position(
                    movement.position,
                    self.resolved_server_position.position,
                    self.resolved_server_position.surface_anchor,
                );
                self.resolved_server_position = resolved;
                self.push_committed_control(CommittedControlEvent::MovePlayer {
                    sequence,
                    movement,
                    resolved,
                    source_cohort,
                });
            }
            WorldEvent::PlayerMovementCorrection(correction) => {
                let sequence =
                    sequence.expect("sequenced movement corrections commit through submit");
                if self
                    .latest_movement_correction_tick
                    .is_some_and(|latest| correction.tick < latest)
                {
                    return;
                }
                self.latest_movement_correction_tick = Some(correction.tick);
                let resolved = resolve_server_position(
                    correction.position,
                    self.resolved_server_position.position,
                    self.resolved_server_position.surface_anchor,
                );
                self.resolved_server_position = resolved;
                self.push_committed_control(CommittedControlEvent::PlayerMovementCorrection {
                    sequence,
                    correction,
                    resolved,
                });
            }
            WorldEvent::SetTime(update) => {
                let sequence = sequence.expect("sequenced SetTime commits through submit");
                self.push_committed_control(CommittedControlEvent::SetTime { sequence, update });
            }
            WorldEvent::DaylightCycle(update) => {
                let sequence = sequence.expect("sequenced daylight-cycle commits through submit");
                self.push_committed_control(CommittedControlEvent::DaylightCycle {
                    sequence,
                    update,
                });
            }
            WorldEvent::Weather(update) => {
                let sequence = sequence.expect("sequenced weather commits through submit");
                self.push_committed_control(CommittedControlEvent::Weather { sequence, update });
            }
            WorldEvent::Actor(event) => {
                let sequence = sequence.expect("sequenced actor events commit through submit");
                let _ = self.actors.apply(self.actor_session_id, sequence, event);
            }
            WorldEvent::SubChunks(_) => unreachable!("sub-chunk batches are prepared on workers"),
        }
    }
    pub(super) fn apply_request_level_chunk(
        &mut self,
        event: LevelChunkEvent,
        decoded: (DecodedBiomeColumn, DecodedBlockEntities),
        sequence: Option<u64>,
    ) {
        let key = ChunkKey::new(event.dimension, event.x, event.z);
        if !self.column_is_active(key) {
            self.record_normalization_error(NormalizationErrorReason::InactiveLevelChunk);
            return;
        }
        let Some(range) = vanilla_dimension_range(event.dimension) else {
            self.record_normalization_error(
                NormalizationErrorReason::UnsupportedLevelChunkDimension,
            );
            return;
        };
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
        if self.store.biome_column_matches(key, &biomes) {
            self.loaded_columns.remove(&key);
            self.request_collision_failures.remove(&key);
            self.purge_sub_chunk_column_state(key);
        } else {
            self.evict_column(key);
        }
        let biome_dirty = self.store.commit_biome_column(key, biomes);
        let now = Instant::now();
        for dirty in biome_dirty {
            if self.resident.contains(&dirty) && self.store.sub_chunk(dirty).is_some() {
                self.mark_dirty_exact(dirty, now);
            }
        }
        self.store.commit_chunk_block_entities(key, block_entities);
        self.refresh_block_entity_visuals_for_chunk(key);
        self.enqueue_request(key, range.base_sub_chunk_y, count, sequence);
        if has_authoritative_upper_air {
            let first_air_y = range
                .base_sub_chunk_y
                .saturating_add(i32::try_from(count).expect("vanilla subchunk count fits i32"));
            let end_y = range.base_sub_chunk_y.saturating_add(
                i32::try_from(range.sub_chunk_count).expect("vanilla subchunk count fits i32"),
            );
            let changed = (first_air_y..end_y)
                .filter_map(|y| {
                    let air = SubChunkKey::from_chunk(key, y);
                    let removed = self.store.apply_request_mode_air(air).is_some();
                    let became_known = self.record_known_air(air);
                    if removed {
                        self.refresh_block_entity_visuals_for_sub_chunk(air);
                    }
                    (removed || became_known).then_some(air)
                })
                .collect::<BTreeSet<_>>();
            self.mark_changed_sources(changed, Instant::now());
        }
    }
    pub(super) fn push_committed_control(&mut self, event: CommittedControlEvent) {
        assert!(
            self.committed_controls.len() < COMMITTED_CONTROL_CAPACITY,
            "control admission invariant exceeded bounded commit-delta capacity"
        );
        self.committed_controls.push_back(event);
    }
}

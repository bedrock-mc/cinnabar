use super::*;

impl WorldStream {
    #[must_use]
    pub const fn committed_sequence(&self) -> u64 {
        self.ordered.next_sequence().saturating_sub(1)
    }

    pub fn new(bootstrap: WorldBootstrap) -> Self {
        Self::new_with_assets(
            bootstrap,
            Arc::new(RuntimeAssets::diagnostic()),
            [0.0, crate::server_position::SAFE_SERVER_HEIGHT, 0.0],
            None,
        )
    }
    pub fn new_with_assets(
        bootstrap: WorldBootstrap,
        runtime_assets: Arc<RuntimeAssets>,
        current_position: [f32; 3],
        existing_anchor: Option<[i32; 2]>,
    ) -> Self {
        Self::with_first_sequence_and_recovery(
            bootstrap,
            runtime_assets,
            1,
            current_position,
            existing_anchor,
        )
    }
    pub fn new_with_asset_sets(
        bootstrap: WorldBootstrap,
        runtime_assets: Arc<RuntimeAssets>,
        entity_assets: Arc<RuntimeEntityAssets>,
        current_position: [f32; 3],
        existing_anchor: Option<[i32; 2]>,
    ) -> Self {
        Self::with_first_sequence_and_asset_sets(
            bootstrap,
            runtime_assets,
            Some(entity_assets),
            1,
            current_position,
            existing_anchor,
        )
    }
    pub(super) fn with_first_sequence_and_recovery(
        bootstrap: WorldBootstrap,
        runtime_assets: Arc<RuntimeAssets>,
        first_sequence: u64,
        current_position: [f32; 3],
        existing_anchor: Option<[i32; 2]>,
    ) -> Self {
        Self::with_first_sequence_and_asset_sets(
            bootstrap,
            runtime_assets,
            None,
            first_sequence,
            current_position,
            existing_anchor,
        )
    }
    fn with_first_sequence_and_asset_sets(
        bootstrap: WorldBootstrap,
        runtime_assets: Arc<RuntimeAssets>,
        entity_assets: Option<Arc<RuntimeEntityAssets>>,
        first_sequence: u64,
        current_position: [f32; 3],
        existing_anchor: Option<[i32; 2]>,
    ) -> Self {
        let (decode_tx, decode_rx) = bounded(WORK_RESULT_CAPACITY);
        let (light_tx, light_rx) = bounded(LIGHT_RESULT_CAPACITY);
        let (mesh_tx, mesh_rx) = bounded(WORK_RESULT_CAPACITY);
        let resolved_server_position =
            resolve_server_position(bootstrap.player_position, current_position, existing_anchor);
        let resolved_biome_tints = Arc::new(
            runtime_assets
                .biome_assets()
                .resolve_live(&[])
                .expect("validated runtime biome assets resolve without live definitions"),
        );
        let biome_tint_stream_id = NEXT_BIOME_TINT_STREAM_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .expect("biome tint stream identity space exhausted");
        let actor_session_id = NEXT_ACTOR_SESSION_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .expect("actor session identity space exhausted");
        let network_id_mode = if bootstrap.block_network_ids_are_hashes {
            NetworkIdMode::Hashed
        } else {
            NetworkIdMode::Sequential
        };
        let air_network_id = runtime_assets
            .air_network_id(network_id_mode)
            .unwrap_or(bootstrap.air_network_id);
        let mut actors = entity_assets.map_or_else(
            || ActorStore::new(actor_session_id, bootstrap.dimension),
            |assets| {
                ActorStore::new_with_entity_assets(actor_session_id, bootstrap.dimension, assets)
            },
        );
        actors.exclude_remote_state_for(bootstrap.local_player_runtime_id);
        Self {
            store: ChunkStore::new(),
            block_entity_visuals: BlockEntityVisualDiagnostics::default(),
            actors,
            actor_session_id,
            classifier: BlockClassifier::new(air_network_id),
            network_id_mode,
            runtime_assets,
            biome_definitions: Arc::from([]),
            resolved_biome_tints,
            biome_tint_stream_id,
            biome_tint_revision: 0,
            current_dimension: bootstrap.dimension,
            local_player_runtime_id: bootstrap.local_player_runtime_id,
            ordered: SequenceBuffer::new(first_sequence),
            submitted: HashSet::new(),
            heavy_sequences: HashSet::new(),
            pending_decode: VecDeque::new(),
            in_flight_decode_jobs: 0,
            blocking_block_updates: None,
            decode_tx,
            decode_rx,
            light_tx,
            light_rx,
            mesh_tx,
            mesh_rx,
            next_block_generation: 0,
            block_generations: HashMap::new(),
            light_store: LightStore::default(),
            light_ownership: HashMap::new(),
            direct_sky: BTreeMap::new(),
            light_failures: HashMap::new(),
            light_revisions: RevisionTracker::default(),
            pending_light: HashMap::new(),
            in_flight_light: HashMap::new(),
            light_waiters: HashMap::new(),
            fatal_light_failure: false,
            fatal_error: None,
            revisions: RevisionTracker::default(),
            applied_mesh_generations: HashMap::new(),
            mesh_dependency_masks: HashMap::new(),
            pending_mesh: HashMap::new(),
            in_flight: HashMap::new(),
            resident: BTreeSet::new(),
            known_air: BTreeSet::new(),
            loaded_columns: BTreeSet::new(),
            requested_sub_chunks: HashMap::new(),
            request_collision_failures: HashSet::new(),
            sub_chunk_deadlines: BTreeSet::new(),
            correlated_sub_chunk_attempts: HashMap::new(),
            admitted_sub_chunk_replies: HashMap::new(),
            deferred_retries: VecDeque::new(),
            deferred_retry_set: HashSet::new(),
            connectivity: HashMap::new(),
            connectivity_generation: 0,
            requests: RequestQueue::default(),
            transport_pending_requests: 0,
            last_request_player_chunk: None,
            publication_allowance: None,
            mesh_changes: VecDeque::new(),
            committed_controls: VecDeque::new(),
            committed_ui: VecDeque::new(),
            pending_same_location_reset: false,
            publisher_center: Some([
                floor_to_i32(resolved_server_position.position[0]),
                floor_to_i32(resolved_server_position.position[1]),
                floor_to_i32(resolved_server_position.position[2]),
            ]),
            publisher_radius_blocks: None,
            publisher_radius_chunks: None,
            committed_view_cohort: None,
            provisional_publisher_rebase: false,
            local_resets_armed: 0,
            local_resets_consumed: 0,
            local_reset_dispatch_count: 0,
            local_reset_dispatch_total: 0,
            local_reset_dispatch_active: false,
            local_reset_dispatch_classes: [None; MAX_LOCAL_RESET_DISPATCH_EVIDENCE],
            publisher_epoch: 0,
            required_columns: BTreeSet::new(),
            source_columns: BTreeSet::new(),
            source_capture_sequence: None,
            chunk_radius: None,
            resolved_server_position,
            latest_movement_correction_tick: None,
            stats: WorldStreamStats::default(),
        }
    }
    pub(super) fn enqueue_decode_job(&mut self, job: DecodeJob) {
        self.pending_decode.push_back(QueuedDecodeJob {
            queued_at: Instant::now(),
            job,
        });
    }
    /// Commits an app-owned event's position in the shared network FIFO
    /// without duplicating that event in world-owned state.
    pub fn commit(&mut self, sequence: u64) -> Result<(), WorldStreamError> {
        if sequence < self.ordered.next_sequence() || self.submitted.contains(&sequence) {
            return Err(SequenceError::DuplicateOrPast {
                sequence,
                next: self.ordered.next_sequence(),
            }
            .into());
        }
        let retained_commits = self
            .committed_controls
            .len()
            .saturating_add(self.committed_ui.len());
        if self.submitted.len() >= MAX_ADMITTED_WORLD_EVENTS.saturating_sub(retained_commits) {
            return Err(WorldStreamError::AdmissionFull {
                sequence,
                admitted: self.submitted.len(),
                capacity: MAX_ADMITTED_WORLD_EVENTS,
                heavy_admitted: self.heavy_sequences.len(),
                heavy_capacity: MAX_ADMITTED_HEAVY_EVENTS,
            });
        }
        self.submitted.insert(sequence);
        if let Err(error) = self
            .ordered
            .insert(sequence, PreparedWorldEvent::CommitOnly)
        {
            self.submitted.remove(&sequence);
            return Err(error.into());
        }
        self.apply_ready();
        Ok(())
    }

    pub fn submit_same_location_reset(&mut self, sequence: u64) -> Result<(), WorldStreamError> {
        if sequence < self.ordered.next_sequence() || self.submitted.contains(&sequence) {
            return Err(SequenceError::DuplicateOrPast {
                sequence,
                next: self.ordered.next_sequence(),
            }
            .into());
        }
        let retained_commits = self
            .committed_controls
            .len()
            .saturating_add(self.committed_ui.len());
        if self.submitted.len() >= MAX_ADMITTED_WORLD_EVENTS.saturating_sub(retained_commits) {
            return Err(WorldStreamError::AdmissionFull {
                sequence,
                admitted: self.submitted.len(),
                capacity: MAX_ADMITTED_WORLD_EVENTS,
                heavy_admitted: self.heavy_sequences.len(),
                heavy_capacity: MAX_ADMITTED_HEAVY_EVENTS,
            });
        }
        self.submitted.insert(sequence);
        if let Err(error) = self
            .ordered
            .insert(sequence, PreparedWorldEvent::SameLocationReset)
        {
            self.submitted.remove(&sequence);
            return Err(error.into());
        }
        self.pending_same_location_reset = true;
        self.apply_ready();
        Ok(())
    }

    #[must_use]
    pub const fn has_pending_same_location_reset(&self) -> bool {
        self.pending_same_location_reset
    }

    pub fn submit(&mut self, sequence: u64, event: WorldEvent) -> Result<(), WorldStreamError> {
        if sequence < self.ordered.next_sequence() || self.submitted.contains(&sequence) {
            return Err(SequenceError::DuplicateOrPast {
                sequence,
                next: self.ordered.next_sequence(),
            }
            .into());
        }

        let heavy = matches!(
            event,
            WorldEvent::LevelChunk(_)
                | WorldEvent::SubChunks(_)
                | WorldEvent::BlockUpdates(_)
                | WorldEvent::BlockEntityUpdate(_)
        );
        let creates_request = match &event {
            WorldEvent::LevelChunk(LevelChunkEvent {
                mode: LevelChunkMode::LimitedRequests { highest },
                ..
            }) => *highest != 0,
            WorldEvent::LevelChunk(LevelChunkEvent {
                mode: LevelChunkMode::LimitlessRequests,
                ..
            }) => true,
            _ => false,
        };
        if creates_request && self.requests.len() >= OUTBOUND_REQUEST_CAPACITY {
            return Err(WorldStreamError::OutboundFull {
                sequence,
                pending: self.requests.len(),
                capacity: OUTBOUND_REQUEST_CAPACITY,
            });
        }
        let retained_commits = self
            .committed_controls
            .len()
            .saturating_add(self.committed_ui.len());
        if self.submitted.len() >= MAX_ADMITTED_WORLD_EVENTS.saturating_sub(retained_commits)
            || (heavy && self.heavy_sequences.len() >= MAX_ADMITTED_HEAVY_EVENTS)
        {
            return Err(WorldStreamError::AdmissionFull {
                sequence,
                admitted: self.submitted.len(),
                capacity: MAX_ADMITTED_WORLD_EVENTS,
                heavy_admitted: self.heavy_sequences.len(),
                heavy_capacity: MAX_ADMITTED_HEAVY_EVENTS,
            });
        }
        self.submitted.insert(sequence);
        if heavy {
            self.heavy_sequences.insert(sequence);
        }
        if creates_request {
            self.requests.reserve(sequence);
        }

        match event {
            WorldEvent::LevelChunk(
                event @ LevelChunkEvent {
                    mode: LevelChunkMode::Inline { count },
                    ..
                },
            ) => {
                let Some(range) = vanilla_dimension_range(event.dimension)
                    .filter(|range| count <= range.sub_chunk_count)
                else {
                    self.heavy_sequences.remove(&sequence);
                    self.ordered
                        .insert(sequence, PreparedWorldEvent::NormalizationFailure)?;
                    self.apply_ready();
                    return Ok(());
                };
                self.enqueue_decode_job(DecodeJob::InlineLevelChunk {
                    sequence,
                    event,
                    base_sub_chunk_y: range.base_sub_chunk_y,
                    count,
                    biome_storage_count: range.sub_chunk_count,
                });
            }
            WorldEvent::LevelChunk(
                event @ LevelChunkEvent {
                    mode: LevelChunkMode::LimitedRequests { .. } | LevelChunkMode::LimitlessRequests,
                    ..
                },
            ) => {
                let Some(range) = vanilla_dimension_range(event.dimension) else {
                    self.heavy_sequences.remove(&sequence);
                    self.ordered
                        .insert(sequence, PreparedWorldEvent::NormalizationFailure)?;
                    self.apply_ready();
                    return Ok(());
                };
                self.enqueue_decode_job(DecodeJob::RequestLevelChunk {
                    sequence,
                    event,
                    biome_base_sub_chunk_y: range.base_sub_chunk_y,
                    biome_storage_count: range.sub_chunk_count,
                });
            }
            WorldEvent::SubChunks(batch) => {
                if batch.entries.is_empty() {
                    self.heavy_sequences.remove(&sequence);
                    self.ordered
                        .insert(sequence, PreparedWorldEvent::NormalizationFailure)?;
                    self.apply_ready();
                    return Ok(());
                }
                self.record_sub_chunk_reply_admissions(&batch);
                self.enqueue_decode_job(DecodeJob::SubChunks { sequence, batch });
            }
            WorldEvent::BlockEntityUpdate(event) => {
                self.enqueue_decode_job(DecodeJob::BlockEntityUpdate { sequence, event });
            }
            immediate => {
                if let Err(error) = self
                    .ordered
                    .insert(sequence, PreparedWorldEvent::Immediate(immediate))
                {
                    self.cancel_request_reservation(sequence);
                    return Err(error.into());
                }
                self.apply_ready();
            }
        }
        Ok(())
    }
    pub fn remaining_admission_capacity(&self) -> usize {
        MAX_ADMITTED_WORLD_EVENTS
            .saturating_sub(
                self.submitted
                    .len()
                    .saturating_add(self.committed_controls.len())
                    .saturating_add(self.committed_ui.len()),
            )
            .min(MAX_ADMITTED_HEAVY_EVENTS.saturating_sub(self.heavy_sequences.len()))
            .min(OUTBOUND_REQUEST_CAPACITY.saturating_sub(self.requests.len()))
    }
}

use super::*;

impl WorldStream {
    /// Last sequence whose complete ordered mutation has been applied.
    #[must_use]
    pub fn committed_sequence(&self) -> u64 {
        self.order.committed_sequence()
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
        let _ = &*workers::WORKERS;
        let (decode_tx, decode_rx) = bounded(WORK_RESULT_CAPACITY);
        let (light_tx, light_rx) = bounded(LIGHT_RESULT_CAPACITY);
        let (mesh_tx, mesh_rx) = bounded(WORK_RESULT_CAPACITY);
        let authority = client_world::WorldAuthority::new(
            bootstrap,
            runtime_assets,
            entity_assets,
            current_position,
            existing_anchor,
        );
        let air_network_id = authority.air_block_id();
        let publisher_center = Some(
            authority
                .resolved_server_position()
                .position
                .map(floor_to_i32),
        );
        Self {
            light_diagnostics: light_diagnostics::LightingDiagnostics::new(bootstrap.dimension),
            mesh_memory: meshing::memory::MeshMemoryBudget::new(authority.runtime_assets()),
            authority,
            order: client_world::ingestion::OrderedCommitState::new(first_sequence),
            block_cracks: block_cracks::BlockCracks::default(),
            block_entity_visuals: BlockEntityVisualDiagnostics::default(),
            classifier: BlockClassifier::new(air_network_id),
            startup_terrain_announced: true,
            seasonal_foliage: seasonal_foliage::SeasonalFoliage::default(),
            pending_decode: VecDeque::new(),
            in_flight_decode_jobs: 0,
            predictions: prediction::DeferredPredictions::default(),
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
            pending_light_scan: VecDeque::new(),
            pending_light_ready: BinaryHeap::new(),
            pending_light_deferred: BinaryHeap::new(),
            light_priority_wakeups: HashMap::new(),
            light_scheduler_refresh: Default::default(),
            in_flight_light: HashMap::new(),
            next_light_batch_id: 0,
            in_flight_light_batches: HashMap::new(),
            running_light_jobs: Arc::new(AtomicUsize::new(0)),
            last_dispatched_light_batch: HashMap::new(),
            light_waiters: HashMap::new(),
            fatal_light_failure: false,
            fatal_error: None,
            revisions: RevisionTracker::default(),
            applied_mesh_generations: HashMap::new(),
            mesh_dependency_masks: HashMap::new(),
            pending_mesh: HashMap::new(),
            pending_mesh_scan: VecDeque::new(),
            pending_resident_mesh_deferred: BinaryHeap::new(),
            pending_resident_mesh_ready: BinaryHeap::new(),
            pending_mesh_removal_deferred: BinaryHeap::new(),
            pending_mesh_removal_ready: BinaryHeap::new(),
            mesh_scheduler_refresh: Default::default(),
            view_forward: None,
            in_flight: HashMap::new(),
            admitted_mesh_jobs: Arc::new(AtomicUsize::new(0)),
            mesh_cancellations: HashMap::new(),
            urgent_mesh_in_flight: HashSet::new(),
            staged_mesh_completions: VecDeque::new(),
            staged_mesh_bytes: 0,
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
            deferred_recovery_requests: VecDeque::new(),
            connectivity: FastHashMap::new(),
            connectivity_generation: 0,
            requests: RequestQueue::default(),
            transport_pending_requests: 0,
            last_request_player_chunk: None,
            unsent_column_deadlines: HashMap::new(),
            arrival_cohort: None,
            poll_deadline: None,
            frame_deadline: None,
            polling: false,
            publication_allowance: None,
            mesh_changes: VecDeque::new(),
            publisher_center,
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
            last_retention_center: None,
            last_retention_radius: None,
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
        self.order.validate_sequence(sequence)?;
        let retained_commits = self.authority.retained_commit_count();
        self.order.admit(sequence, false, retained_commits)?;
        if let Err(error) = self
            .order
            .insert_ready(sequence, PreparedWorldEvent::CommitOnly)
        {
            self.order.cancel_admission(sequence);
            return Err(error);
        }
        self.apply_ready();
        Ok(())
    }

    pub fn submit(&mut self, sequence: u64, event: WorldEvent) -> Result<(), WorldStreamError> {
        self.submit_with_level_chunk_payload(sequence, event, None)
    }

    /// Additive zero-copy ingress used by the app's private LevelChunk lane.
    pub fn submit_level_chunk_bytes(
        &mut self,
        sequence: u64,
        mut event: LevelChunkEvent,
        payload: Bytes,
    ) -> Result<(), WorldStreamError> {
        event.payload.clear();
        self.submit_with_level_chunk_payload(sequence, WorldEvent::LevelChunk(event), Some(payload))
    }

    fn submit_with_level_chunk_payload(
        &mut self,
        sequence: u64,
        event: WorldEvent,
        mut level_chunk_payload: Option<Bytes>,
    ) -> Result<(), WorldStreamError> {
        self.order.validate_sequence(sequence)?;

        let heavy = matches!(
            event,
            WorldEvent::LevelChunk(_)
                | WorldEvent::ChunkResync(_)
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
            WorldEvent::ChunkResync(event) => event
                .requested_sub_chunk_ys
                .as_ref()
                .map_or(event.requested_sub_chunks != Some(0), |ys| !ys.is_empty()),
            _ => false,
        };
        if creates_request && self.requests.len() >= OUTBOUND_REQUEST_CAPACITY {
            return Err(WorldStreamError::OutboundFull {
                sequence,
                pending: self.requests.len(),
                capacity: OUTBOUND_REQUEST_CAPACITY,
            });
        }
        let retained_commits = self.authority.retained_commit_count();
        self.order.admit(sequence, heavy, retained_commits)?;
        if creates_request {
            self.requests.reserve(sequence);
        }

        match event {
            WorldEvent::LevelChunk(
                mut event @ LevelChunkEvent {
                    mode: LevelChunkMode::Inline { count },
                    ..
                },
            ) => {
                let Some(range) = vanilla_dimension_range(event.dimension) else {
                    self.order.release_heavy(sequence);
                    self.order
                        .insert_ready(sequence, PreparedWorldEvent::NormalizationFailure)?;
                    self.apply_ready();
                    return Ok(());
                };
                let ids = self.decode_ids(event.dimension);
                self.enqueue_decode_job(DecodeJob::InlineLevelChunk {
                    sequence,
                    payload: level_chunk_payload
                        .take()
                        .unwrap_or_else(|| Bytes::from(std::mem::take(&mut event.payload))),
                    event,
                    slots: dimension_slots(range),
                    count,
                    ids,
                });
            }
            WorldEvent::LevelChunk(
                mut event @ LevelChunkEvent {
                    mode: LevelChunkMode::LimitedRequests { .. } | LevelChunkMode::LimitlessRequests,
                    ..
                },
            ) => {
                let Some(range) = vanilla_dimension_range(event.dimension) else {
                    self.order.release_heavy(sequence);
                    self.order
                        .insert_ready(sequence, PreparedWorldEvent::NormalizationFailure)?;
                    self.apply_ready();
                    return Ok(());
                };
                let ids = self.decode_ids(event.dimension);
                self.enqueue_decode_job(DecodeJob::RequestLevelChunk {
                    sequence,
                    payload: level_chunk_payload
                        .take()
                        .unwrap_or_else(|| Bytes::from(std::mem::take(&mut event.payload))),
                    event,
                    slots: dimension_slots(range),
                    ids,
                });
            }
            WorldEvent::SubChunks(batch) => {
                if batch.entries.is_empty() {
                    self.order.release_heavy(sequence);
                    self.order
                        .insert_ready(sequence, PreparedWorldEvent::NormalizationFailure)?;
                    self.apply_ready();
                    return Ok(());
                }
                self.record_sub_chunk_reply_admissions(&batch);
                let ids = self.decode_ids(batch.dimension);
                self.enqueue_decode_job(DecodeJob::SubChunks {
                    sequence,
                    batch,
                    ids,
                });
            }
            WorldEvent::SubChunkReplyAdmission(admission) => {
                self.record_sub_chunk_reply_admission(&admission);
                if let Err(error) = self
                    .order
                    .insert_ready(sequence, PreparedWorldEvent::CommitOnly)
                {
                    self.order.cancel_admission(sequence);
                    return Err(error);
                }
                self.apply_ready();
            }
            WorldEvent::BlockEntityUpdate(event) => {
                self.enqueue_decode_job(DecodeJob::BlockEntityUpdate { sequence, event });
            }
            immediate => {
                if let Err(error) = self
                    .order
                    .insert_ready(sequence, PreparedWorldEvent::Immediate(immediate))
                {
                    self.cancel_request_reservation(sequence);
                    return Err(error);
                }
                self.apply_ready();
            }
        }
        Ok(())
    }
    pub fn remaining_admission_capacity(&self) -> usize {
        self.order
            .remaining_admission_capacity(self.authority.retained_commit_count())
            .min(OUTBOUND_REQUEST_CAPACITY.saturating_sub(self.requests.len()))
    }
}

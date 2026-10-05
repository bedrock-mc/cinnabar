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
            mesh_tx,
            mesh_rx,
            lighting: lighting::Lighting::new(),
            fatal_error: None,
            revisions: RevisionTracker::default(),
            applied_mesh_generations: HashMap::new(),
            actor_block_syncs: actor_block_sync::ActorBlockSyncs::default(),
            mesh_dependency_masks: HashMap::new(),
            mesh_jobs: Default::default(),
            view_forward: None,
            dimension_transfer_priority: None,
            admitted_mesh_jobs: Arc::new(AtomicUsize::new(0)),
            mesh_cancellations: HashMap::new(),
            urgent_mesh_in_flight: HashSet::new(),
            staged_mesh_completions: VecDeque::new(),
            staged_mesh_bytes: 0,
            resident: BTreeSet::new(),
            known_air: BTreeSet::new(),
            loaded_columns: BTreeSet::new(),
            connectivity: FastHashMap::new(),
            connectivity_generation: 0,
            requests: Default::default(),
            unsent_column_deadlines: HashMap::new(),
            arrival_cohort: None,
            poll_deadline: None,
            frame_deadline: None,
            polling: false,
            publication_allowance: None,
            mesh_changes: VecDeque::new(),
            publisher: cohort::PublisherScope {
                center: publisher_center,
                ..Default::default()
            },
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
                | WorldEvent::SyncedBlockUpdates(_)
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
        if creates_request && self.requests.queue.len() >= OUTBOUND_REQUEST_CAPACITY {
            return Err(WorldStreamError::OutboundFull {
                sequence,
                pending: self.requests.queue.len(),
                capacity: OUTBOUND_REQUEST_CAPACITY,
            });
        }
        let retained_commits = self.authority.retained_commit_count();
        self.order.admit(sequence, heavy, retained_commits)?;
        if creates_request {
            self.requests.queue.reserve(sequence);
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
            .min(OUTBOUND_REQUEST_CAPACITY.saturating_sub(self.requests.queue.len()))
    }
}

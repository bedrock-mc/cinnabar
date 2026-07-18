use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PublicationStageCounters {
    pub requests_constructed: u64,
    pub requests_sent: u64,
    pub responses_admitted: u64,
    pub decode_jobs_dispatched: u64,
    pub decode_jobs_completed: u64,
    pub subchunks_committed: u64,
    pub light_jobs_dispatched: u64,
    pub light_jobs_completed: u64,
    pub mesh_jobs_dispatched: u64,
    pub mesh_jobs_completed: u64,
    pub mesh_changes_queued: u64,
    pub mesh_changes_dequeued: u64,
    pub mesh_uploads_acknowledged: u64,
    pub requests_ready: usize,
    pub requests_transport_pending: usize,
    pub subchunks_awaiting_response: usize,
    pub decode_jobs_queued: usize,
    pub decode_jobs_in_flight: usize,
    pub light_jobs_queued: usize,
    pub light_jobs_in_flight: usize,
    pub mesh_jobs_queued: usize,
    pub mesh_jobs_in_flight: usize,
    pub mesh_changes_pending: usize,
    pub mesh_uploads_unacknowledged: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubChunkOutcomeCounters {
    pub success: u64,
    pub all_air: u64,
    pub unavailable: u64,
    pub malformed: u64,
    pub stale: u64,
    pub timed_out: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StageDurations {
    pub decode: Duration,
    pub lighting: Duration,
    pub meshing: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Phase2PublicationSnapshot {
    pub session_generation: u64,
    pub publisher_epoch: u64,
    pub player_column: ChunkKey,
    /// Exact raw block radius received from NetworkChunkPublisherUpdate.
    pub publisher_radius_blocks: Option<u32>,
    pub publisher_radius_chunks: Option<i32>,
    pub required_cohort_hash: u64,
    pub required_columns: usize,
    pub loaded_required_columns: usize,
    pub required_cohort_stable: bool,
    pub stages: PublicationStageCounters,
    pub outcomes: SubChunkOutcomeCounters,
    pub max_queue_wait: StageDurations,
    pub max_worker_time: StageDurations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CohortManifestIdentity {
    pub session_generation: u64,
    pub publisher_epoch: u64,
    pub required_cohort_count: usize,
    pub required_cohort_hash: u64,
    pub generation_manifest_hash: u64,
    pub entry_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RequestClass {
    PlayerRetry,
    PlayerInitial,
    VisibleRetry,
    VisibleInitial,
    PrefetchRetry,
    PrefetchInitial,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum BuildProfileIdentity {
    #[default]
    Unknown,
    Debug,
    Release,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum PresentModeIdentity {
    #[default]
    Unknown,
    Fifo,
    Immediate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Phase2PresentationSnapshot {
    pub build_profile: BuildProfileIdentity,
    pub graphics_identity_sha256: [u8; 32],
    pub requested_present_mode: PresentModeIdentity,
    pub effective_present_mode: PresentModeIdentity,
    pub assets_manifest_sha256: [u8; 32],
    pub publisher_disk: CohortManifestIdentity,
    pub resident: CohortManifestIdentity,
    pub allocation: CohortManifestIdentity,
    pub visible: CohortManifestIdentity,
    pub submitted: CohortManifestIdentity,
    pub gpu_presented: CohortManifestIdentity,
}

impl WorldStream {
    #[must_use]
    pub fn phase2_publication_snapshot(
        &self,
        player_column: ChunkKey,
    ) -> Phase2PublicationSnapshot {
        let required = self
            .committed_view_cohort
            .filter(|cohort| cohort.dimension == player_column.dimension)
            .map_or_else(BTreeSet::new, |_| self.required_columns.clone());
        let mut stages = self.stats.phase2_stages;
        stages.requests_ready = self.pending_request_count();
        stages.requests_transport_pending = self.transport_pending_requests;
        stages.subchunks_awaiting_response = self.sub_chunk_deadlines.len();
        stages.decode_jobs_queued = self.pending_decode.len();
        stages.decode_jobs_in_flight = self.in_flight_decode_jobs;
        stages.light_jobs_queued = self.pending_light.len();
        stages.light_jobs_in_flight = self.in_flight_light.len();
        stages.mesh_jobs_queued = self.pending_mesh.len();
        stages.mesh_jobs_in_flight = self.in_flight.len();
        stages.mesh_changes_pending = self.mesh_changes.len();
        stages.mesh_uploads_unacknowledged = self.revisions.entries.len();

        Phase2PublicationSnapshot {
            session_generation: self.actor_session_id,
            publisher_epoch: self.publisher_epoch,
            player_column,
            publisher_radius_blocks: self.publisher_radius_blocks,
            publisher_radius_chunks: self.publisher_radius_chunks,
            required_cohort_hash: deterministic_chunk_key_hash(&required),
            required_columns: required.len(),
            loaded_required_columns: self.loaded_columns.intersection(&required).count(),
            required_cohort_stable: !required.is_empty()
                && self.submitted.is_empty()
                && self.pending_decode.is_empty()
                && self.in_flight_decode_jobs == 0
                && self.decode_rx.is_empty()
                && self.requests.is_empty()
                && self.transport_pending_requests == 0
                && self.requested_sub_chunks.is_empty()
                && self.deferred_retries.is_empty()
                && self.pending_light.is_empty()
                && self.in_flight_light.is_empty()
                && self.light_rx.is_empty()
                && self.pending_mesh.is_empty()
                && self.in_flight.is_empty()
                && self.mesh_rx.is_empty()
                && self.mesh_changes.is_empty()
                && self.revisions.entries.is_empty(),
            stages,
            outcomes: self.stats.phase2_outcomes,
            max_queue_wait: StageDurations {
                decode: self.stats.max_decode_queue_wait,
                lighting: self.stats.max_light_queue_wait,
                meshing: self.stats.max_mesh_queue_wait,
            },
            max_worker_time: StageDurations {
                decode: self.stats.max_decode_duration,
                lighting: self.stats.max_light_duration,
                meshing: self.stats.max_mesh_duration,
            },
        }
    }
}

pub(super) fn deterministic_chunk_key_hash(keys: &BTreeSet<ChunkKey>) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    keys.iter()
        .flat_map(|key| [key.dimension, key.x, key.z])
        .flat_map(i32::to_le_bytes)
        .fold(FNV_OFFSET_BASIS, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
        })
}

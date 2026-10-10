//! Observations gathered from app resources for the acceptance plugin.
#[cfg(feature = "acceptance")]
use ::acceptance::AcceptanceRun;
use ::acceptance::world_observation::{WorldReadyCommands, WorldReadyObservation};
use ::acceptance::world_ready::observe_world_ready;
use ::acceptance::world_ready::{SubChunkTimeoutProgress, WorldReadyWork};
use bevy::prelude::*;
use chunk_pipeline::{ForcedRemeshManifest, ForcedRemeshManifestState, WorldStream};
use render::{ChunkRenderQueue, ChunkUploadAcknowledgements, PresentedFrameGate};
use std::time::Instant;
use world::SubChunkKey;
use {
    crate::runtime::network::NetworkHandle,
    crate::runtime::visibility::{AppMetrics, CaveVisibilityCache, DiagnosticQuads},
    crate::runtime::world::{ClientWorld, WorldStreamFramePoll},
    acceptance::model_witness::ModelWitnessFileSource,
};

/// Translates explicit evidence commands into the existing world API.
struct Commands<'a>(&'a mut WorldStream);
impl WorldReadyCommands for Commands<'_> {
    /// Requests the existing exact-manifest remesh from the stream owner.
    fn remesh_published_manifest(
        &mut self,
        published: &[(SubChunkKey, u64)],
        now: Instant,
    ) -> Option<ForcedRemeshManifest> {
        self.0.remesh_published_manifest(published, now)
    }
    /// Reads completion without giving the evidence crate ownership of the stream.
    fn forced_remesh_manifest_state(
        &self,
        manifest: &ForcedRemeshManifest,
    ) -> ForcedRemeshManifestState {
        self.0.forced_remesh_manifest_state(manifest)
    }
    /// Starts the stream metrics interval after the evidence gate settles.
    fn begin_timed_session(&mut self) {
        self.0.begin_timed_session();
    }
}

/// Publishes transport, world and visibility facts without transferring their ownership.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_world_ready(
    network: Res<NetworkHandle>,
    frame_poll: Res<WorldStreamFramePoll>,
    mut client_world: ResMut<ClientWorld>,
    cache: Res<CaveVisibilityCache>,
    diagnostic_quads: Res<DiagnosticQuads>,
    render_queue: Res<ChunkRenderQueue>,
    model_witness_source: Res<ModelWitnessFileSource>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    presented_frames: Res<PresentedFrameGate>,
    mut acceptance: ResMut<AcceptanceRun>,
    mut auto_fly: ResMut<client_presentation::camera::AutoFly>,
    mut metrics: ResMut<AppMetrics>,
    mut cameras: Query<&mut Transform, With<client_presentation::camera::FlyCamera>>,
) {
    let missing_mapping_count = client_world.missing_asset_count();
    let Some(stream) = client_world.stream.as_mut() else {
        return;
    };
    let stats = stream.stats();
    let (readiness_produced, readiness_consumed) = network.readiness_ingress_progress();
    let target_cohort = acceptance.full_view_teleport.target_cohort().map(|target| {
        frame_poll
            .cohort
            .filter(|status| status.target == target)
            .unwrap_or_else(|| stream.cohort_status(target))
    });
    let mutation_target = acceptance.mutation_coordinate().map(|coordinate| {
        SubChunkKey::new(
            stream.current_dimension(),
            coordinate[0].div_euclid(16),
            coordinate[1].div_euclid(16),
            coordinate[2].div_euclid(16),
        )
    });
    let observation = WorldReadyObservation {
        missing_mapping_count,
        timeout_progress: SubChunkTimeoutProgress {
            awaiting_responses: stats.awaiting_sub_chunk_responses,
            timeouts: stats.sub_chunk_timeouts,
            retries_scheduled: stats.sub_chunk_retries_scheduled,
            retry_exhaustions: stats.sub_chunk_retry_exhaustions,
        },
        work: WorldReadyWork {
            network_events: network.pending_event_count(),
            readiness_events: network.pending_readiness_event_count(),
            network_commands: network.pending_command_count(),
            admitted_world_events: stats.admitted_world_events,
            queued_decode_jobs: stats.queued_decode_jobs,
            in_flight_decode_jobs: stats.in_flight_decode_jobs,
            completed_decode_results: stats.completed_decode_results,
            pending_light_jobs: stats.pending_light_jobs,
            in_flight_light_jobs: stats.in_flight_light_jobs,
            terminal_light_failures: stats.terminal_light_failures,
            pending_mesh_jobs: stats.pending_mesh_jobs,
            in_flight_mesh_jobs: stats.in_flight_mesh_jobs,
            pending_mesh_changes: stream.pending_mesh_change_count(),
            outbound_requests: stream.pending_request_work_count(),
            outstanding_sub_chunks: stream.outstanding_sub_chunk_count(),
            pending_retry_requests: stats.pending_retry_requests,
            render_queue_items: render_queue.retained_len(),
            pending_gpu_acknowledgements: usize::from(!acknowledgements.is_empty()),
            unacknowledged_meshes: stream.unacknowledged_mesh_count(),
        },
        committed_cohort: frame_poll.cohort,
        required_columns: stream.required_columns().clone(),
        target_cohort,
        loaded_columns: stream.loaded_column_count(),
        rendered_sub_chunks: cache.rendered.len(),
        visible_sub_chunks: cache.visible_rendered,
        mutation_target_rendered: mutation_target
            .is_some_and(|target| cache.rendered.contains_key(&target)),
        mutation_target_visible: mutation_target.is_some_and(|target| cache.is_visible(target)),
        mutation_target_clean: mutation_target.is_some_and(|target| stream.is_mesh_clean(target)),
        position: stream.resolved_server_position().position,
        local_player_runtime_id: stream.local_player_runtime_id(),
        stats,
        readiness_produced,
        readiness_consumed,
    };
    observe_world_ready(
        observation,
        &mut Commands(stream),
        &diagnostic_quads.0,
        &render_queue,
        model_witness_source.configured(),
        &presented_frames,
        &mut acceptance,
        &mut auto_fly,
        &mut metrics.0,
        cameras.single_mut().ok().as_deref_mut(),
    );
}

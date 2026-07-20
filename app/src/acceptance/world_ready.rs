use std::{io::Write, time::Instant};

use bevy::{
    log::error,
    prelude::{Query, Res, ResMut, Transform, Vec3, With},
};
use client_world::ViewCohortStatus;
use render::{
    ChunkRenderQueue, ChunkUploadAcknowledgements, PresentedFrameAck, PresentedFrameGate,
    TargetRenderExpectation,
};
use world::SubChunkKey;

use super::{
    AcceptanceRun, PHASE0_REQUESTED_RADIUS_CHUNKS,
    markers::GALLERY_ANCHOR_READY,
    model_witness::ModelWitnessFileSource,
    mutation::{MutationTracker, world_ready_markers, write_stdout_marker},
    proofs::{
        forced_remesh_proof, forced_remesh_settled_marker, teleport_proof, teleport_settled_marker,
    },
    remesh::FullViewRemeshTracker,
    teleport::{
        FullViewTeleportTracker, TeleportReadySnapshot, presented_ack_matches, render_view_cohort,
        teleport_global_stage_diagnostic_marker,
    },
};
use crate::{
    camera,
    runtime::{
        network::NetworkHandle,
        visibility::{AppMetrics, CaveVisibilityCache, DiagnosticQuads},
        world::ClientWorld,
    },
};

pub(crate) const WORLD_READY_QUIET_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(2);

impl AcceptanceRun {
    pub(crate) fn revoke_world_ready_if_cohort_changed(
        &mut self,
        current: Option<ViewCohortStatus>,
    ) -> bool {
        let Some((frozen, current)) = self.mutation_cohort.zip(current) else {
            return false;
        };
        if !self.world_ready
            || frozen.target != current.target
            || frozen.publisher_epoch != current.publisher_epoch
            || (current.is_exact()
                && frozen.expected == current.expected
                && frozen.required_hash == current.required_hash)
        {
            return false;
        }
        self.world_ready = false;
        self.deadline = None;
        self.mutation_cohort = None;
        self.world_ready_settler = WorldReadySettler::default();
        self.full_view_remesh = FullViewRemeshTracker::default();
        let teleport_enabled = self.full_view_teleport.enabled;
        let source_mutation_coordinate = self.source_mutation_coordinate;
        self.full_view_teleport = FullViewTeleportTracker::new(teleport_enabled);
        if let Some(coordinate) = source_mutation_coordinate {
            self.full_view_teleport
                .set_source_mutation_coordinate(coordinate);
        }
        self.mutation = if teleport_enabled {
            None
        } else {
            source_mutation_coordinate.map(MutationTracker::new)
        };
        true
    }
}

#[must_use]
pub(crate) fn authoritative_received_radius(received_radius_chunks: Option<i32>) -> Option<i32> {
    let received = received_radius_chunks?;
    (received > 0 && received <= PHASE0_REQUESTED_RADIUS_CHUNKS).then_some(received)
}

#[must_use]
pub(crate) fn mutation_look_target(coordinate: Option<[i32; 3]>) -> Option<Vec3> {
    coordinate.map(|coordinate| {
        Vec3::new(
            coordinate[0] as f32 + 0.5,
            coordinate[1] as f32 + 0.5,
            coordinate[2] as f32 + 0.5,
        )
    })
}

pub(crate) fn orient_mutation_camera(
    transform: &mut Transform,
    coordinate: Option<[i32; 3]>,
) -> bool {
    let Some(target) = mutation_look_target(coordinate) else {
        return false;
    };
    transform.rotation = camera::look_at_target(transform.translation, target);
    true
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WorldReadyWork {
    pub(crate) network_events: usize,
    pub(crate) network_commands: usize,
    pub(crate) admitted_world_events: usize,
    pub(crate) queued_decode_jobs: usize,
    pub(crate) in_flight_decode_jobs: usize,
    pub(crate) completed_decode_results: usize,
    pub(crate) pending_light_jobs: usize,
    pub(crate) in_flight_light_jobs: usize,
    pub(crate) terminal_light_failures: usize,
    pub(crate) pending_mesh_jobs: usize,
    pub(crate) in_flight_mesh_jobs: usize,
    pub(crate) pending_mesh_changes: usize,
    pub(crate) outbound_requests: usize,
    pub(crate) outstanding_sub_chunks: usize,
    pub(crate) pending_retry_requests: usize,
    pub(crate) render_queue_items: usize,
    pub(crate) pending_gpu_acknowledgements: usize,
    pub(crate) unacknowledged_meshes: usize,
}

impl WorldReadyWork {
    pub(crate) fn is_empty(self) -> bool {
        self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SubChunkTimeoutProgress {
    pub(crate) awaiting_responses: usize,
    pub(crate) timeouts: u64,
    pub(crate) retries_scheduled: u64,
    pub(crate) retry_exhaustions: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorldReadySnapshot {
    pub(crate) mutation_coordinate: Option<[i32; 3]>,
    pub(crate) received_radius_chunks: Option<i32>,
    pub(crate) publisher_radius_chunks: Option<i32>,
    pub(crate) cohort: Option<ViewCohortStatus>,
    pub(crate) rendered_sub_chunks: usize,
    pub(crate) resident_sub_chunks: usize,
    pub(crate) visible_sub_chunks: usize,
    pub(crate) mutation_target_rendered: bool,
    pub(crate) mutation_target_visible: bool,
    pub(crate) mutation_target_clean: bool,
    pub(crate) work: WorldReadyWork,
}

#[derive(Debug, Default)]
pub(crate) struct WorldReadySettler {
    pub(crate) candidate: Option<(WorldReadySnapshot, Instant)>,
    pub(crate) presentation: Option<WorldReadyPresentationCandidate>,
    pub(crate) next_view_generation: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct WorldReadyPresentationCandidate {
    pub(crate) snapshot: WorldReadySnapshot,
    pub(crate) expectation: TargetRenderExpectation,
    pub(crate) first_frame: Option<PresentedFrameAck>,
    pub(crate) stable: bool,
}

impl WorldReadySettler {
    pub(crate) fn reconcile_presentation(
        &mut self,
        snapshot: WorldReadySnapshot,
        mut proposed: TargetRenderExpectation,
        now: Instant,
    ) -> Option<TargetRenderExpectation> {
        if world_ready_markers(snapshot).is_none() || proposed.manifest.is_empty() {
            self.presentation = None;
            return None;
        }
        if let Some(candidate) = &self.presentation
            && candidate.snapshot == snapshot
            && candidate.expectation.cohort == proposed.cohort
            && candidate.expectation.source_cohort == proposed.source_cohort
            && candidate.expectation.manifest == proposed.manifest
        {
            return Some(candidate.expectation.clone());
        }
        self.next_view_generation = self.next_view_generation.wrapping_add(1).max(1);
        proposed.view_generation = self.next_view_generation;
        proposed.render_ready_at = now;
        self.presentation = Some(WorldReadyPresentationCandidate {
            snapshot,
            expectation: proposed.clone(),
            first_frame: None,
            stable: false,
        });
        Some(proposed)
    }

    pub(crate) fn observe_presented_frame(&mut self, acknowledgement: PresentedFrameAck) -> bool {
        let Some(candidate) = &mut self.presentation else {
            return false;
        };
        if !presented_ack_matches(
            candidate.expectation.render_ready_at,
            &candidate.expectation,
            &acknowledgement,
        ) {
            candidate.first_frame = None;
            candidate.stable = false;
            return false;
        }
        let first = candidate.first_frame.take();
        candidate.stable = first
            .as_ref()
            .is_some_and(|first| first.forms_stable_exact_pair_with(&acknowledgement));
        candidate.first_frame = Some(acknowledgement);
        candidate.stable
    }

    pub(crate) fn has_stable_presentation(&self, snapshot: WorldReadySnapshot) -> bool {
        self.presentation
            .as_ref()
            .is_some_and(|candidate| candidate.snapshot == snapshot && candidate.stable)
    }

    pub(crate) fn observe(
        &mut self,
        snapshot: WorldReadySnapshot,
        now: Instant,
    ) -> Option<[String; 2]> {
        let markers = world_ready_markers(snapshot);
        if markers.is_none() {
            self.candidate = None;
            return None;
        }
        match self.candidate {
            Some((stable, since)) if stable == snapshot => (now.saturating_duration_since(since)
                >= WORLD_READY_QUIET_INTERVAL)
                .then_some(markers.expect("settled snapshots have markers")),
            _ => {
                self.candidate = Some((snapshot, now));
                None
            }
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct GalleryAnchorEmitter {
    pub(crate) emitted: bool,
}

impl GalleryAnchorEmitter {
    pub(crate) fn observe(
        &mut self,
        enabled: bool,
        snapshot: WorldReadySnapshot,
    ) -> Option<String> {
        if self.emitted
            || !enabled
            || !snapshot.mutation_target_rendered
            || !snapshot.mutation_target_clean
        {
            return None;
        }
        let coordinate = snapshot.mutation_coordinate?;
        self.emitted = true;
        Some(format!(
            "{GALLERY_ANCHOR_READY} coordinate={},{},{} rendered=true visible={} clean=true",
            coordinate[0], coordinate[1], coordinate[2], snapshot.mutation_target_visible
        ))
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_world_ready(
    network: Res<NetworkHandle>,
    mut client_world: ResMut<ClientWorld>,
    cache: Res<CaveVisibilityCache>,
    diagnostic_quads: Res<DiagnosticQuads>,
    render_queue: Res<ChunkRenderQueue>,
    model_witness_source: Res<ModelWitnessFileSource>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    presented_frames: Res<PresentedFrameGate>,
    mut acceptance: ResMut<AcceptanceRun>,
    mut auto_fly: ResMut<camera::AutoFly>,
    mut metrics: ResMut<AppMetrics>,
    mut cameras: Query<&mut Transform, With<camera::FlyCamera>>,
) {
    let missing_mapping_count = client_world.runtime_assets.missing_count();
    let Some(stream) = client_world.stream.as_mut() else {
        return;
    };
    let stats = stream.stats();
    let timeout_progress = SubChunkTimeoutProgress {
        awaiting_responses: stats.awaiting_sub_chunk_responses,
        timeouts: stats.sub_chunk_timeouts,
        retries_scheduled: stats.sub_chunk_retries_scheduled,
        retry_exhaustions: stats.sub_chunk_retry_exhaustions,
    };
    let work = WorldReadyWork {
        network_events: network.pending_event_count(),
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
    };
    let committed_cohort = stream
        .committed_view_cohort()
        .map(|target| stream.cohort_status(target));
    if acceptance.revoke_world_ready_if_cohort_changed(committed_cohort) {
        presented_frames.clear();
        return;
    }
    let required_columns = stream.required_columns().clone();
    if acceptance.world_ready {
        let cohort = acceptance
            .full_view_teleport
            .target_cohort()
            .map(|target| stream.cohort_status(target));
        if let Some(status) = cohort {
            debug_assert_eq!(stream.committed_view_cohort(), status.committed);
        }
        let snapshot = TeleportReadySnapshot {
            received_radius_chunks: stats.received_radius_chunks,
            publisher_radius_chunks: stats.publisher_radius_chunks,
            rendered_sub_chunks: cache.rendered.len(),
            resident_sub_chunks: stats.resident_sub_chunks,
            visible_sub_chunks: cache.visible_rendered,
            loaded_columns: stream.loaded_column_count(),
            cohort,
            last_chunk_commit_at: stats.last_chunk_commit_at,
            last_mesh_dispatch_at: stats.last_mesh_dispatch_at,
            last_mesh_completion_at: stats.last_mesh_completion_at,
            last_mesh_ack_at: stats.last_mesh_ack_at,
            work,
        };
        let observed_at = Instant::now();
        let frame_count = metrics.0.frame_count();
        acceptance.full_view_teleport.note_frame(frame_count);
        let teleport = if let Some(pending) = acceptance.full_view_teleport.pending.as_ref() {
            let proposed = render_queue.freeze_target_expectation_for_columns(
                render_view_cohort(pending.target),
                Some(render_view_cohort(pending.source)),
                required_columns.iter().copied(),
                0,
                observed_at,
            );
            let expectation = proposed.and_then(|proposed| {
                acceptance
                    .full_view_teleport
                    .reconcile_presented_expectation(snapshot, proposed, observed_at)
            });
            if let Some(expectation) = expectation {
                presented_frames.set_expectation(expectation);
            } else {
                presented_frames.clear();
            }
            presented_frames
                .drain()
                .into_iter()
                .find_map(|acknowledgement| {
                    acceptance
                        .full_view_teleport
                        .observe_presented_frame(acknowledgement)
                })
        } else {
            None
        };
        if let Some(teleport) = teleport {
            presented_frames.clear();
            let cohort = snapshot
                .cohort
                .expect("teleport completion requires an exact cohort");
            let Some(remesh_manifest) =
                stream.remesh_published_manifest(&teleport.expectation.manifest, Instant::now())
            else {
                error!(
                    "could not start exact full-view remesh: frozen published manifest is stale"
                );
                return;
            };
            if !acceptance.full_view_remesh.start(
                Some(&teleport),
                cohort,
                remesh_manifest,
                frame_count,
            ) {
                error!("could not start exact full-view remesh gate after binding teleport");
                return;
            }
            let proof = teleport_proof(cohort, &teleport);
            metrics.0.record_teleport_proof(proof.clone());
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{}", teleport_settled_marker(&proof));
            let _ = writeln!(
                stdout,
                "{}",
                teleport_global_stage_diagnostic_marker(cohort.target, &teleport)
            );
            let _ = stdout.flush();
            return;
        }

        if let Some(status) = snapshot.cohort
            && let Some(marker) = acceptance.full_view_teleport.cohort_progress_line(
                status,
                snapshot.work,
                timeout_progress,
                observed_at,
            )
        {
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{marker}");
            let _ = stdout.flush();
        }

        let remesh_manifest = acceptance
            .full_view_remesh
            .pending
            .as_ref()
            .map(|pending| pending.manifest.clone());
        if let Some(remesh_manifest) = remesh_manifest {
            let manifest_state = stream.forced_remesh_manifest_state(&remesh_manifest);
            let proposal_target = acceptance.full_view_remesh.pending.as_ref().map(|pending| {
                (
                    render_view_cohort(pending.cohort.target),
                    pending.source_cohort,
                )
            });
            let proposed = if manifest_state == client_world::ForcedRemeshManifestState::Complete {
                proposal_target.and_then(|(target, source)| {
                    render_queue.freeze_target_expectation_for_columns(
                        target,
                        source,
                        required_columns.iter().copied(),
                        0,
                        observed_at,
                    )
                })
            } else {
                None
            };
            let expectation = acceptance.full_view_remesh.reconcile_presented_expectation(
                snapshot,
                manifest_state,
                proposed,
                observed_at,
                frame_count,
            );
            if let Some(expectation) = expectation {
                presented_frames.set_expectation(expectation);
            } else {
                presented_frames.clear();
            }
            let completion = presented_frames
                .drain()
                .into_iter()
                .find_map(|acknowledgement| {
                    acceptance
                        .full_view_remesh
                        .observe_presented_frame(acknowledgement, frame_count)
                });
            if let Some(completion) = completion {
                presented_frames.clear();
                let cohort = snapshot
                    .cohort
                    .expect("forced remesh completion requires the frozen exact cohort");
                let proof = forced_remesh_proof(cohort, &completion);
                metrics
                    .0
                    .record_forced_full_view_remesh_proof(proof.clone());
                let Some(target) = acceptance.full_view_teleport.completed_target_mutation else {
                    error!("forced remesh completed without deterministic mutation coordinates");
                    return;
                };
                if !acceptance.bind_mutation_cohort(cohort) {
                    error!("could not bind target mutation to the exact raw publisher cohort");
                    return;
                }
                if !acceptance.retarget_mutation(target, completion.stable_frame.gpu_completed_at) {
                    error!(
                        ?target,
                        "could not arm target-only mutation after forced remesh"
                    );
                    return;
                }
                let Some(mutation_marker) = acceptance.target_mutation_marker() else {
                    error!("target mutation armed without complete manifest-comparable evidence");
                    return;
                };
                let mut stdout = std::io::stdout().lock();
                let _ = writeln!(stdout, "{}", forced_remesh_settled_marker(&proof));
                let _ = writeln!(stdout, "{mutation_marker}");
                let _ = stdout.flush();
            }
        }

        let completed_remesh_target =
            acceptance
                .full_view_remesh
                .completed
                .as_ref()
                .map(|completion| {
                    (
                        completion.expectation.cohort,
                        completion.expectation.source_cohort,
                    )
                });
        if let Some((target, source)) = completed_remesh_target {
            let expectation = render_queue
                .freeze_target_expectation_for_columns(
                    target,
                    source,
                    required_columns.iter().copied(),
                    0,
                    observed_at,
                )
                .and_then(|proposed| {
                    acceptance.reconcile_mutation_presented_expectation(
                        proposed,
                        snapshot.cohort,
                        observed_at,
                    )
                });
            if let Some(expectation) = expectation {
                presented_frames.set_expectation(expectation);
                if let Some(latency) =
                    presented_frames
                        .drain()
                        .into_iter()
                        .find_map(|acknowledgement| {
                            acceptance.observe_presented_mutation(acknowledgement)
                        })
                {
                    presented_frames.clear();
                    metrics.0.record_mutation_to_visible(latency);
                }
            } else {
                presented_frames.clear();
            }
        }
        return;
    }
    let mutation_coordinate = acceptance.mutation_coordinate();
    if let Some(target) = mutation_look_target(mutation_coordinate) {
        auto_fly.set_look_target(target);
    }
    if let Ok(mut transform) = cameras.single_mut() {
        orient_mutation_camera(&mut transform, mutation_coordinate);
    }
    let mutation_target = mutation_coordinate.map(|coordinate| {
        SubChunkKey::new(
            stream.current_dimension(),
            coordinate[0].div_euclid(16),
            coordinate[1].div_euclid(16),
            coordinate[2].div_euclid(16),
        )
    });
    let snapshot = WorldReadySnapshot {
        mutation_coordinate,
        received_radius_chunks: stats.received_radius_chunks,
        publisher_radius_chunks: stats.publisher_radius_chunks,
        cohort: stream
            .committed_view_cohort()
            .map(|target| stream.cohort_status(target)),
        rendered_sub_chunks: cache.rendered.len(),
        resident_sub_chunks: stats.resident_sub_chunks,
        visible_sub_chunks: cache.visible_rendered,
        mutation_target_rendered: mutation_target
            .is_some_and(|target| cache.rendered.contains(&target)),
        mutation_target_visible: mutation_target.is_some_and(|target| cache.is_visible(target)),
        mutation_target_clean: mutation_target.is_some_and(|target| stream.is_mesh_clean(target)),
        work,
    };
    let ready_at = Instant::now();
    let proposed = snapshot.cohort.and_then(|status| {
        render_queue.freeze_target_expectation_for_columns(
            render_view_cohort(status.target),
            None,
            required_columns.iter().copied(),
            0,
            ready_at,
        )
    });
    let expectation = proposed.and_then(|proposed| {
        acceptance
            .world_ready_settler
            .reconcile_presentation(snapshot, proposed, ready_at)
    });
    if let Some(expectation) = expectation {
        presented_frames.set_expectation(expectation);
    } else {
        presented_frames.clear();
    }
    for acknowledgement in presented_frames.drain() {
        let _ = acceptance
            .world_ready_settler
            .observe_presented_frame(acknowledgement);
    }
    if let Some(marker) = acceptance
        .gallery_anchor
        .observe(model_witness_source.configured(), snapshot)
    {
        let mut stdout = std::io::stdout().lock();
        write_stdout_marker(&mut stdout, &marker);
    }
    let Some(markers) = acceptance.world_ready_settler.observe(snapshot, ready_at) else {
        return;
    };
    if !acceptance
        .world_ready_settler
        .has_stable_presentation(snapshot)
    {
        return;
    }
    presented_frames.clear();
    metrics
        .0
        .record_asset_counters(missing_mapping_count, diagnostic_quads.0.total());
    metrics
        .0
        .record_diagnostic_attribution(diagnostic_quads.0.snapshot());
    let asset_marker = metrics
        .0
        .asset_metrics()
        .world_ready_marker(snapshot.resident_sub_chunks, snapshot.visible_sub_chunks);
    let mut stdout = std::io::stdout().lock();
    for marker in markers {
        let _ = writeln!(stdout, "{marker}");
    }
    let _ = writeln!(stdout, "{asset_marker}");
    let _ = stdout.flush();
    stream.begin_timed_session();
    metrics.0.begin_timed_session(ready_at);
    let Some(cohort) = snapshot.cohort else {
        error!("world-ready markers emitted without an exact raw publisher cohort");
        return;
    };
    if !acceptance.bind_mutation_cohort(cohort) {
        error!("world-ready markers emitted with an inexact raw publisher cohort");
        return;
    }
    acceptance.begin_world_ready(
        ready_at,
        stream.resolved_server_position().position,
        stream.local_player_runtime_id(),
    );
}

#[cfg(test)]
use std::sync::Arc;
use std::time::{Duration, Instant};

use client_world::{CommittedControlEvent, ViewCohort, ViewCohortStatus};
use render::{PresentedFrameAck, RenderViewCohort, TargetRenderExpectation};
#[cfg(test)]
use world::SubChunkKey;

use super::mutation::leaf_forest_target_mutation_coordinate;
use super::{
    PHASE0_REQUESTED_RADIUS_CHUNKS,
    markers::{TELEPORT_COHORT, TELEPORT_GLOBAL_STAGE_DIAGNOSTIC},
    proofs::{horizontal_chunk, optional_duration_milliseconds, optional_milliseconds_token},
    world_ready::{SubChunkTimeoutProgress, WorldReadyWork, authoritative_publisher_radius},
};

const TELEPORT_COHORT_PROGRESS_INTERVAL: Duration = Duration::from_secs(1);
const FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA: u64 = (PHASE0_REQUESTED_RADIUS_CHUNKS as u64) * 2 + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TeleportReadySnapshot {
    pub(crate) received_radius_chunks: Option<i32>,
    pub(crate) publisher_radius_chunks: Option<i32>,
    pub(crate) rendered_sub_chunks: usize,
    pub(crate) resident_sub_chunks: usize,
    pub(crate) visible_sub_chunks: usize,
    pub(crate) loaded_columns: usize,
    pub(crate) cohort: Option<ViewCohortStatus>,
    pub(crate) last_chunk_commit_at: Option<Instant>,
    pub(crate) last_mesh_dispatch_at: Option<Instant>,
    pub(crate) last_mesh_completion_at: Option<Instant>,
    pub(crate) last_mesh_ack_at: Option<Instant>,
    pub(crate) work: WorldReadyWork,
}

impl TeleportReadySnapshot {
    pub(crate) fn is_binding_ready(self) -> bool {
        authoritative_publisher_radius(self.received_radius_chunks, self.publisher_radius_chunks)
            .is_some()
            && self.cohort.is_some_and(ViewCohortStatus::is_exact)
            && self.work.is_empty()
    }

    pub(crate) fn is_ready(self) -> bool {
        self.is_binding_ready()
            && self.rendered_sub_chunks != 0
            && self.resident_sub_chunks != 0
            && self.visible_sub_chunks != 0
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TeleportPresentedCandidate {
    pub(crate) snapshot: TeleportReadySnapshot,
    pub(crate) status: ViewCohortStatus,
    pub(crate) expectation: TargetRenderExpectation,
    pub(crate) first_frame: Option<PresentedFrameAck>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingFullViewTeleport {
    pub(crate) started: Instant,
    pub(crate) started_frame_count: u64,
    pub(crate) move_sequence: u64,
    pub(crate) target: ViewCohort,
    pub(crate) source: ViewCohort,
    pub(crate) target_mutation_coordinate: [i32; 3],
    pub(crate) publisher_seen: bool,
    pub(crate) publisher_latency: Option<Duration>,
    pub(crate) first_level_chunk_latency: Option<Duration>,
    pub(crate) last_level_chunk_latency: Option<Duration>,
    pub(crate) level_chunk_events: u64,
    pub(crate) first_sub_chunk_latency: Option<Duration>,
    pub(crate) last_sub_chunk_latency: Option<Duration>,
    pub(crate) sub_chunk_events: u64,
    pub(crate) peak_network_events: usize,
    pub(crate) presented_candidate: Option<TeleportPresentedCandidate>,
    pub(crate) last_progress_at: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FullViewTeleportCompletion {
    pub(crate) settle_latency: Duration,
    pub(crate) render_ready_latency: Duration,
    pub(crate) first_present_return_latency: Duration,
    pub(crate) first_gpu_completion_latency: Duration,
    pub(crate) stable_present_return_latency: Duration,
    pub(crate) stable_gpu_completion_latency: Duration,
    pub(crate) view_generation: u64,
    pub(crate) target_mutation_coordinate: [i32; 3],
    pub(crate) publisher_latency: Option<Duration>,
    pub(crate) first_level_chunk_latency: Option<Duration>,
    pub(crate) last_level_chunk_latency: Option<Duration>,
    pub(crate) level_chunk_events: u64,
    pub(crate) first_sub_chunk_latency: Option<Duration>,
    pub(crate) last_sub_chunk_latency: Option<Duration>,
    pub(crate) sub_chunk_events: u64,
    pub(crate) last_chunk_commit_latency: Option<Duration>,
    pub(crate) last_mesh_dispatch_latency: Option<Duration>,
    pub(crate) last_mesh_completion_latency: Option<Duration>,
    pub(crate) last_mesh_ack_latency: Option<Duration>,
    pub(crate) peak_network_events: usize,
    pub(crate) expectation: TargetRenderExpectation,
    pub(crate) first_frame: PresentedFrameAck,
    pub(crate) stable_frame: PresentedFrameAck,
    pub(crate) frame_count: u64,
}

#[derive(Debug)]
pub(crate) struct FullViewTeleportTracker {
    pub(crate) enabled: bool,
    pub(crate) origin_chunk: Option<[i32; 2]>,
    pub(crate) source_mutation_coordinate: Option<[i32; 3]>,
    pub(crate) local_player_runtime_id: Option<u64>,
    pub(crate) latest_publisher_ingress: Option<(u64, ViewCohort, Instant)>,
    pub(crate) pending_move_ingress: Option<(u64, Instant, u64)>,
    pub(crate) pending: Option<PendingFullViewTeleport>,
    pub(crate) completed: Option<Duration>,
    pub(crate) completed_target: Option<ViewCohort>,
    pub(crate) completed_target_mutation: Option<[i32; 3]>,
    pub(crate) next_view_generation: u64,
    pub(crate) current_frame_count: u64,
    #[cfg(test)]
    pub(crate) next_test_sequence: u64,
}

impl FullViewTeleportTracker {
    pub(crate) const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            origin_chunk: None,
            source_mutation_coordinate: None,
            local_player_runtime_id: None,
            latest_publisher_ingress: None,
            pending_move_ingress: None,
            pending: None,
            completed: None,
            completed_target: None,
            completed_target_mutation: None,
            next_view_generation: 0,
            current_frame_count: 0,
            #[cfg(test)]
            next_test_sequence: 0,
        }
    }

    pub(crate) fn begin_world_ready(&mut self, position: [f32; 3], local_player_runtime_id: u64) {
        if self.enabled {
            self.origin_chunk = horizontal_chunk(position);
            self.local_player_runtime_id = Some(local_player_runtime_id);
        }
    }

    pub(crate) fn set_source_mutation_coordinate(&mut self, coordinate: [i32; 3]) {
        self.source_mutation_coordinate = Some(coordinate);
    }

    pub(crate) fn observe_ingress(
        &mut self,
        event: &protocol::WorldEvent,
        sequence: u64,
        observed_at: Instant,
        current_dimension: i32,
        frame_count: u64,
    ) -> bool {
        if !self.enabled || self.completed.is_some() {
            return false;
        }
        match event {
            protocol::WorldEvent::PublisherUpdate(update) => {
                let publisher = ViewCohort::from_publisher(
                    current_dimension,
                    update.center,
                    update.radius_blocks,
                );
                if self
                    .latest_publisher_ingress
                    .is_none_or(|(latest, _, _)| sequence >= latest)
                {
                    self.latest_publisher_ingress = Some((sequence, publisher, observed_at));
                }
                if let Some(pending) = &mut self.pending
                    && sequence > pending.move_sequence
                    && publisher == pending.target
                    && !pending.publisher_seen
                {
                    pending.publisher_seen = true;
                    pending.publisher_latency = observed_at.checked_duration_since(pending.started);
                }
                false
            }
            protocol::WorldEvent::MovePlayer(movement) if self.pending.is_none() => {
                if self.local_player_runtime_id != Some(movement.runtime_id) {
                    return false;
                }
                let (Some(origin), Some(target)) =
                    (self.origin_chunk, horizontal_chunk(movement.position))
                else {
                    return false;
                };
                let far_enough = i64::from(origin[0]).abs_diff(i64::from(target[0]))
                    >= FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA
                    || i64::from(origin[1]).abs_diff(i64::from(target[1]))
                        >= FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA;
                if !far_enough {
                    return false;
                }
                if self
                    .pending_move_ingress
                    .is_none_or(|(pending_sequence, _, _)| sequence < pending_sequence)
                {
                    self.pending_move_ingress = Some((sequence, observed_at, frame_count));
                    return true;
                }
                false
            }
            protocol::WorldEvent::LevelChunk(event) => {
                if let Some(pending) = &mut self.pending
                    && sequence > pending.move_sequence
                    && pending
                        .target
                        .contains_column(event.dimension, [event.x, event.z])
                {
                    let latency = observed_at.saturating_duration_since(pending.started);
                    pending.first_level_chunk_latency.get_or_insert(latency);
                    pending.last_level_chunk_latency = Some(latency);
                    pending.level_chunk_events = pending.level_chunk_events.saturating_add(1);
                }
                false
            }
            protocol::WorldEvent::SubChunks(batch) => {
                if let Some(pending) = &mut self.pending
                    && sequence > pending.move_sequence
                {
                    let target_entries = batch
                        .entries
                        .iter()
                        .filter(|entry| {
                            pending.target.contains_column(
                                batch.dimension,
                                [entry.position[0], entry.position[2]],
                            )
                        })
                        .count();
                    if target_entries == 0 {
                        return false;
                    }
                    let latency = observed_at.saturating_duration_since(pending.started);
                    pending.first_sub_chunk_latency.get_or_insert(latency);
                    pending.last_sub_chunk_latency = Some(latency);
                    pending.sub_chunk_events = pending
                        .sub_chunk_events
                        .saturating_add(u64::try_from(target_entries).unwrap_or(u64::MAX));
                }
                false
            }
            _ => false,
        }
    }

    pub(crate) fn observe_committed_control(&mut self, control: &CommittedControlEvent) -> bool {
        let CommittedControlEvent::MovePlayer {
            sequence,
            movement,
            source_cohort,
            ..
        } = control
        else {
            return false;
        };
        self.commit_move(*sequence, *movement, *source_cohort)
    }

    pub(crate) fn commit_move(
        &mut self,
        sequence: u64,
        movement: protocol::MovePlayerEvent,
        source_cohort: Option<ViewCohort>,
    ) -> bool {
        if !self.enabled || self.completed.is_some() || self.pending.is_some() {
            return false;
        }
        let Some((pending_sequence, started, started_frame_count)) = self.pending_move_ingress
        else {
            return false;
        };
        if pending_sequence != sequence {
            return false;
        }
        self.pending_move_ingress = None;
        if self.local_player_runtime_id != Some(movement.runtime_id) {
            return false;
        }
        let (Some(origin), Some(target_center)) =
            (self.origin_chunk, horizontal_chunk(movement.position))
        else {
            return false;
        };
        let Some(target_mutation_coordinate) = self
            .source_mutation_coordinate
            .and_then(|source| leaf_forest_target_mutation_coordinate(movement.position, source))
        else {
            return false;
        };
        let far_enough = i64::from(origin[0]).abs_diff(i64::from(target_center[0]))
            >= FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA
            || i64::from(origin[1]).abs_diff(i64::from(target_center[1]))
                >= FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA;
        if !far_enough {
            return false;
        }
        let Some(source) = source_cohort
            .filter(|source| source.radius > 0 && source.radius <= PHASE0_REQUESTED_RADIUS_CHUNKS)
        else {
            return false;
        };
        let target = ViewCohort {
            dimension: source.dimension,
            center: target_center,
            radius: source.radius,
        };
        if source == target {
            return false;
        }
        let matching_publisher =
            self.latest_publisher_ingress
                .filter(|(publisher_sequence, publisher, _)| {
                    *publisher_sequence > sequence && *publisher == target
                });
        self.pending = Some(PendingFullViewTeleport {
            started,
            started_frame_count,
            move_sequence: sequence,
            target,
            source,
            target_mutation_coordinate,
            publisher_seen: matching_publisher.is_some(),
            publisher_latency: matching_publisher
                .and_then(|(_, _, observed_at)| observed_at.checked_duration_since(started)),
            first_level_chunk_latency: None,
            last_level_chunk_latency: None,
            level_chunk_events: 0,
            first_sub_chunk_latency: None,
            last_sub_chunk_latency: None,
            sub_chunk_events: 0,
            peak_network_events: 0,
            presented_candidate: None,
            last_progress_at: None,
        });
        true
    }

    #[cfg(test)]
    pub(crate) fn observe(
        &mut self,
        event: &protocol::WorldEvent,
        observed_at: Instant,
        current_dimension: i32,
    ) -> bool {
        self.next_test_sequence = self.next_test_sequence.saturating_add(1);
        let sequence = self.next_test_sequence;
        let source = self.origin_chunk.map(|center| ViewCohort {
            dimension: current_dimension,
            center,
            radius: PHASE0_REQUESTED_RADIUS_CHUNKS,
        });
        let capture = self.observe_ingress(
            event,
            sequence,
            observed_at,
            current_dimension,
            self.next_test_sequence,
        );
        if capture && let protocol::WorldEvent::MovePlayer(movement) = event {
            return self.commit_move(sequence, *movement, source);
        }
        false
    }

    pub(crate) fn reconcile_presented_expectation(
        &mut self,
        snapshot: TeleportReadySnapshot,
        mut proposed: TargetRenderExpectation,
        now: Instant,
    ) -> Option<TargetRenderExpectation> {
        let pending = self.pending.as_mut()?;
        pending.peak_network_events = pending
            .peak_network_events
            .max(snapshot.work.network_events);
        let Some(status) = snapshot.cohort else {
            pending.presented_candidate = None;
            return None;
        };
        let target = render_view_cohort(pending.target);
        let source = render_view_cohort(pending.source);
        if !pending.publisher_seen
            || !snapshot.is_binding_ready()
            || status.target != pending.target
            || proposed.cohort != target
            || proposed.source_cohort != Some(source)
            || proposed.manifest.is_empty()
        {
            pending.presented_candidate = None;
            return None;
        }

        if let Some(candidate) = &mut pending.presented_candidate
            && candidate.status == status
            && candidate.expectation.cohort == proposed.cohort
            && candidate.expectation.source_cohort == proposed.source_cohort
            && candidate.expectation.manifest == proposed.manifest
        {
            candidate.snapshot = snapshot;
            return Some(candidate.expectation.clone());
        }

        self.next_view_generation = self.next_view_generation.wrapping_add(1).max(1);
        proposed.view_generation = self.next_view_generation;
        proposed.render_ready_at = now;
        pending.presented_candidate = Some(TeleportPresentedCandidate {
            snapshot,
            status,
            expectation: proposed.clone(),
            first_frame: None,
        });
        Some(proposed)
    }

    pub(crate) fn observe_presented_frame(
        &mut self,
        acknowledgement: PresentedFrameAck,
    ) -> Option<FullViewTeleportCompletion> {
        let completion = {
            let pending = self.pending.as_mut()?;
            let candidate = pending.presented_candidate.as_mut()?;
            if !presented_ack_matches(pending.started, &candidate.expectation, &acknowledgement) {
                candidate.first_frame = None;
                return None;
            }
            let Some(first) = candidate.first_frame.take() else {
                candidate.first_frame = Some(acknowledgement);
                return None;
            };
            if !first.forms_stable_exact_pair_with(&acknowledgement)
                || first.present_returned_at > acknowledgement.present_returned_at
            {
                candidate.first_frame = Some(acknowledgement);
                return None;
            }

            let started = pending.started;
            Some(FullViewTeleportCompletion {
                settle_latency: acknowledgement
                    .gpu_completed_at
                    .checked_duration_since(started)?,
                render_ready_latency: candidate
                    .expectation
                    .render_ready_at
                    .checked_duration_since(started)?,
                first_present_return_latency: first
                    .present_returned_at
                    .checked_duration_since(started)?,
                first_gpu_completion_latency: first
                    .gpu_completed_at
                    .checked_duration_since(started)?,
                stable_present_return_latency: acknowledgement
                    .present_returned_at
                    .checked_duration_since(started)?,
                stable_gpu_completion_latency: acknowledgement
                    .gpu_completed_at
                    .checked_duration_since(started)?,
                view_generation: candidate.expectation.view_generation,
                target_mutation_coordinate: pending.target_mutation_coordinate,
                publisher_latency: pending.publisher_latency,
                first_level_chunk_latency: pending.first_level_chunk_latency,
                last_level_chunk_latency: pending.last_level_chunk_latency,
                level_chunk_events: pending.level_chunk_events,
                first_sub_chunk_latency: pending.first_sub_chunk_latency,
                last_sub_chunk_latency: pending.last_sub_chunk_latency,
                sub_chunk_events: pending.sub_chunk_events,
                last_chunk_commit_latency: latency_after(
                    started,
                    candidate.snapshot.last_chunk_commit_at,
                ),
                last_mesh_dispatch_latency: latency_after(
                    started,
                    candidate.snapshot.last_mesh_dispatch_at,
                ),
                last_mesh_completion_latency: latency_after(
                    started,
                    candidate.snapshot.last_mesh_completion_at,
                ),
                last_mesh_ack_latency: latency_after(started, candidate.snapshot.last_mesh_ack_at),
                peak_network_events: pending.peak_network_events,
                expectation: candidate.expectation.clone(),
                first_frame: first,
                stable_frame: acknowledgement,
                frame_count: self
                    .current_frame_count
                    .saturating_sub(pending.started_frame_count)
                    .saturating_add(1)
                    .max(2),
            })
        };
        if let Some(completion) = &completion {
            self.completed_target = self.pending.as_ref().map(|pending| pending.target);
            self.completed_target_mutation = Some(completion.target_mutation_coordinate);
            self.pending = None;
            self.completed = Some(completion.settle_latency);
        }
        completion
    }

    #[cfg(test)]
    pub(crate) fn observe_snapshot(
        &mut self,
        snapshot: TeleportReadySnapshot,
        now: Instant,
    ) -> Option<FullViewTeleportCompletion> {
        let pending = self.pending.as_ref()?;
        let proposed = TargetRenderExpectation {
            cohort: render_view_cohort(pending.target),
            source_cohort: Some(render_view_cohort(pending.source)),
            manifest: Arc::from([(
                SubChunkKey::new(
                    pending.target.dimension,
                    0,
                    pending.target.center[0],
                    pending.target.center[1],
                ),
                1,
            )]),
            view_generation: 0,
            render_ready_at: now,
        };
        let _ = self.reconcile_presented_expectation(snapshot, proposed, now);
        None
    }

    pub(crate) fn target_cohort(&self) -> Option<ViewCohort> {
        self.pending
            .as_ref()
            .map(|pending| pending.target)
            .or(self.completed_target)
    }

    pub(crate) fn note_frame(&mut self, frame_count: u64) {
        self.current_frame_count = frame_count;
    }

    pub(crate) fn cohort_progress_line(
        &mut self,
        status: ViewCohortStatus,
        work: WorldReadyWork,
        timeout_progress: SubChunkTimeoutProgress,
        now: Instant,
    ) -> Option<String> {
        let pending = self.pending.as_mut()?;
        if pending.last_progress_at.is_some_and(|previous| {
            now.saturating_duration_since(previous) < TELEPORT_COHORT_PROGRESS_INTERVAL
        }) {
            return None;
        }
        pending.last_progress_at = Some(now);
        let committed = status
            .committed
            .map_or_else(|| "none".to_owned(), cohort_tag);
        Some(format!(
            "{TELEPORT_COHORT} target={} committed={} exact={} expected={} loaded_target={} missing_target={} foreign_loaded={} foreign_requested={} foreign_resident={} source_leftover={} resident_count={} resident_hash={:016x} known_air_count={} known_air_hash={:016x} network_events={} network_commands={} admitted_world_events={} queued_decode_jobs={} in_flight_decode_jobs={} completed_decode_results={} pending_light_jobs={} in_flight_light_jobs={} terminal_light_failures={} pending_mesh_jobs={} in_flight_mesh_jobs={} pending_mesh_changes={} outbound_requests={} outstanding_sub_chunks={} pending_retry_requests={} awaiting_sub_chunk_responses={} sub_chunk_timeouts={} sub_chunk_retries_scheduled={} sub_chunk_retry_exhaustions={} render_queue_items={} pending_gpu_acknowledgements={} unacknowledged_meshes={}",
            cohort_tag(pending.target),
            committed,
            status.is_exact(),
            status.expected,
            status.loaded_target,
            status.missing_target,
            status.foreign_loaded,
            status.foreign_requested,
            status.foreign_resident,
            status.source_leftover,
            status.resident_count,
            status.resident_hash,
            status.known_air_count,
            status.known_air_hash,
            work.network_events,
            work.network_commands,
            work.admitted_world_events,
            work.queued_decode_jobs,
            work.in_flight_decode_jobs,
            work.completed_decode_results,
            work.pending_light_jobs,
            work.in_flight_light_jobs,
            work.terminal_light_failures,
            work.pending_mesh_jobs,
            work.in_flight_mesh_jobs,
            work.pending_mesh_changes,
            work.outbound_requests,
            work.outstanding_sub_chunks,
            work.pending_retry_requests,
            timeout_progress.awaiting_responses,
            timeout_progress.timeouts,
            timeout_progress.retries_scheduled,
            timeout_progress.retry_exhaustions,
            work.render_queue_items,
            work.pending_gpu_acknowledgements,
            work.unacknowledged_meshes,
        ))
    }

    #[cfg(test)]
    pub(crate) const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    #[cfg(test)]
    pub(crate) fn has_clean_candidate(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.presented_candidate.is_some())
    }
}

pub(crate) const fn render_view_cohort(cohort: ViewCohort) -> RenderViewCohort {
    RenderViewCohort::new(cohort.dimension, cohort.center, cohort.radius)
}

pub(crate) fn presented_ack_matches(
    started: Instant,
    expectation: &TargetRenderExpectation,
    acknowledgement: &PresentedFrameAck,
) -> bool {
    acknowledgement.cohort == expectation.cohort
        && acknowledgement.view_generation == expectation.view_generation
        && acknowledgement.render_ready_at == expectation.render_ready_at
        && acknowledgement.allocation_manifest == expectation.manifest
        && acknowledgement.is_exact()
        && acknowledgement
            .render_ready_at
            .checked_duration_since(started)
            .is_some()
        && acknowledgement
            .present_returned_at
            .checked_duration_since(acknowledgement.render_ready_at)
            .is_some()
        && acknowledgement
            .gpu_completed_at
            .checked_duration_since(acknowledgement.present_returned_at)
            .is_some()
}

pub(crate) fn cohort_tag(cohort: ViewCohort) -> String {
    format!(
        "{}:{}:{}:{}",
        cohort.dimension, cohort.center[0], cohort.center[1], cohort.radius
    )
}

pub(crate) fn teleport_global_stage_diagnostic_marker(
    target: ViewCohort,
    completion: &FullViewTeleportCompletion,
) -> String {
    format!(
        "{TELEPORT_GLOBAL_STAGE_DIAGNOSTIC} target={} global_commit_ms={} global_mesh_dispatch_ms={} global_mesh_complete_ms={} global_mesh_ack_ms={}",
        cohort_tag(target),
        optional_milliseconds_token(optional_duration_milliseconds(
            completion.last_chunk_commit_latency,
        )),
        optional_milliseconds_token(optional_duration_milliseconds(
            completion.last_mesh_dispatch_latency,
        )),
        optional_milliseconds_token(optional_duration_milliseconds(
            completion.last_mesh_completion_latency,
        )),
        optional_milliseconds_token(optional_duration_milliseconds(
            completion.last_mesh_ack_latency,
        )),
    )
}

pub(crate) fn latency_after(started: Instant, observed: Option<Instant>) -> Option<Duration> {
    observed.and_then(|observed| observed.checked_duration_since(started))
}

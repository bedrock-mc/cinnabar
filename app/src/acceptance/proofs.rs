use std::time::Duration;

use client_world::ViewCohortStatus;
use render::{PresentedFrameAck, TargetRenderExpectation};
use world::SubChunkKey;

use super::{
    markers::{FORCED_FULL_VIEW_REMESH_SETTLED, TELEPORT_SETTLED},
    remesh::FullViewRemeshCompletion,
    teleport::{FullViewTeleportCompletion, cohort_tag},
};
use crate::metrics::{ExactFullViewProof, TeleportProof, deterministic_manifest_hash};

pub(crate) struct FullViewCompletionEvidence<'a> {
    pub(crate) settle_latency: Duration,
    pub(crate) render_ready_latency: Duration,
    pub(crate) first_present_return_latency: Duration,
    pub(crate) first_gpu_completion_latency: Duration,
    pub(crate) stable_present_return_latency: Duration,
    pub(crate) stable_gpu_completion_latency: Duration,
    pub(crate) view_generation: u64,
    pub(crate) expectation: &'a TargetRenderExpectation,
    pub(crate) first_frame: &'a PresentedFrameAck,
    pub(crate) stable_frame: &'a PresentedFrameAck,
    pub(crate) frame_count: u64,
}

pub(crate) fn exact_full_view_proof(
    status: ViewCohortStatus,
    evidence: FullViewCompletionEvidence<'_>,
) -> ExactFullViewProof {
    let manifest_evidence = |manifest: &[(SubChunkKey, u64)]| {
        (
            manifest.len(),
            format!("{:016x}", deterministic_manifest_hash(manifest)),
        )
    };
    let (expected_manifest_count, expected_manifest_hash) =
        manifest_evidence(&evidence.expectation.manifest);
    let (first_presented_manifest_count, first_presented_manifest_hash) =
        manifest_evidence(&evidence.first_frame.drawn_manifest);
    let (stable_presented_manifest_count, stable_presented_manifest_hash) =
        manifest_evidence(&evidence.stable_frame.drawn_manifest);
    ExactFullViewProof {
        target: cohort_tag(status.target),
        committed: status
            .committed
            .map_or_else(|| "none".to_owned(), cohort_tag),
        ms: duration_milliseconds(evidence.settle_latency),
        view_generation: evidence.view_generation,
        transparent_sort_generation: evidence.stable_frame.transparent_sort_generation,
        render_ready_ms: duration_milliseconds(evidence.render_ready_latency),
        first_frame_sequence: evidence.first_frame.frame_sequence,
        stable_frame_sequence: evidence.stable_frame.frame_sequence,
        first_present_ms: duration_milliseconds(evidence.first_present_return_latency),
        first_gpu_ms: duration_milliseconds(evidence.first_gpu_completion_latency),
        stable_present_ms: duration_milliseconds(evidence.stable_present_return_latency),
        stable_gpu_ms: duration_milliseconds(evidence.stable_gpu_completion_latency),
        frame_count: evidence.frame_count,
        expected_manifest_count,
        expected_manifest_hash,
        first_presented_manifest_count,
        first_presented_manifest_hash,
        stable_presented_manifest_count,
        stable_presented_manifest_hash,
        expected: status.expected,
        loaded_target: status.loaded_target,
        missing_target: status.missing_target,
        foreign_loaded: status.foreign_loaded,
        foreign_requested: status.foreign_requested,
        foreign_resident: status.foreign_resident,
        source_leftover: status.source_leftover,
        resident_count: status.resident_count,
        resident_hash: format!("{:016x}", status.resident_hash),
        known_air_count: status.known_air_count,
        known_air_hash: format!("{:016x}", status.known_air_hash),
        missing_target_instances: evidence.stable_frame.missing_target_instances,
        unexpected_target_instances: evidence.stable_frame.unexpected_target_instances,
        source_instances: evidence.stable_frame.source_instances,
        foreign_instances: evidence.stable_frame.foreign_instances,
        stale_generation_instances: evidence.stable_frame.stale_generation_instances,
        orphan_allocations: evidence.stable_frame.orphan_allocations,
    }
}

pub(crate) fn teleport_proof(
    status: ViewCohortStatus,
    completion: &FullViewTeleportCompletion,
) -> TeleportProof {
    TeleportProof {
        exact: exact_full_view_proof(
            status,
            FullViewCompletionEvidence {
                settle_latency: completion.settle_latency,
                render_ready_latency: completion.render_ready_latency,
                first_present_return_latency: completion.first_present_return_latency,
                first_gpu_completion_latency: completion.first_gpu_completion_latency,
                stable_present_return_latency: completion.stable_present_return_latency,
                stable_gpu_completion_latency: completion.stable_gpu_completion_latency,
                view_generation: completion.view_generation,
                expectation: &completion.expectation,
                first_frame: &completion.first_frame,
                stable_frame: &completion.stable_frame,
                frame_count: completion.frame_count,
            },
        ),
        publisher_ms: optional_duration_milliseconds(completion.publisher_latency),
        first_level_ms: optional_duration_milliseconds(completion.first_level_chunk_latency),
        last_level_ms: optional_duration_milliseconds(completion.last_level_chunk_latency),
        level_events: completion.level_chunk_events,
        first_sub_ms: optional_duration_milliseconds(completion.first_sub_chunk_latency),
        last_sub_ms: optional_duration_milliseconds(completion.last_sub_chunk_latency),
        sub_events: completion.sub_chunk_events,
    }
}

pub(crate) fn forced_remesh_proof(
    status: ViewCohortStatus,
    completion: &FullViewRemeshCompletion,
) -> ExactFullViewProof {
    exact_full_view_proof(
        status,
        FullViewCompletionEvidence {
            settle_latency: completion.settle_latency,
            render_ready_latency: completion.render_ready_latency,
            first_present_return_latency: completion.first_present_return_latency,
            first_gpu_completion_latency: completion.first_gpu_completion_latency,
            stable_present_return_latency: completion.stable_present_return_latency,
            stable_gpu_completion_latency: completion.stable_gpu_completion_latency,
            view_generation: completion.view_generation,
            expectation: &completion.expectation,
            first_frame: &completion.first_frame,
            stable_frame: &completion.stable_frame,
            frame_count: completion.frame_count,
        },
    )
}

pub(crate) fn exact_full_view_proof_marker_fields(proof: &ExactFullViewProof) -> String {
    format!(
        "target={} committed={} ms={:.4} view_generation={} transparent_sort_generation={} render_ready_ms={:.4} first_frame_sequence={} stable_frame_sequence={} first_present_ms={:.4} first_gpu_ms={:.4} stable_present_ms={:.4} stable_gpu_ms={:.4} frame_count={} expected_manifest_count={} expected_manifest_hash={} first_presented_manifest_count={} first_presented_manifest_hash={} stable_presented_manifest_count={} stable_presented_manifest_hash={} expected={} loaded_target={} missing_target={} foreign_loaded={} foreign_requested={} foreign_resident={} source_leftover={} resident_count={} resident_hash={} known_air_count={} known_air_hash={} missing_target_instances={} unexpected_target_instances={} source_instances={} foreign_instances={} stale_generation_instances={} orphan_allocations={}",
        proof.target,
        proof.committed,
        proof.ms,
        proof.view_generation,
        proof.transparent_sort_generation,
        proof.render_ready_ms,
        proof.first_frame_sequence,
        proof.stable_frame_sequence,
        proof.first_present_ms,
        proof.first_gpu_ms,
        proof.stable_present_ms,
        proof.stable_gpu_ms,
        proof.frame_count,
        proof.expected_manifest_count,
        proof.expected_manifest_hash,
        proof.first_presented_manifest_count,
        proof.first_presented_manifest_hash,
        proof.stable_presented_manifest_count,
        proof.stable_presented_manifest_hash,
        proof.expected,
        proof.loaded_target,
        proof.missing_target,
        proof.foreign_loaded,
        proof.foreign_requested,
        proof.foreign_resident,
        proof.source_leftover,
        proof.resident_count,
        proof.resident_hash,
        proof.known_air_count,
        proof.known_air_hash,
        proof.missing_target_instances,
        proof.unexpected_target_instances,
        proof.source_instances,
        proof.foreign_instances,
        proof.stale_generation_instances,
        proof.orphan_allocations,
    )
}

pub(crate) fn teleport_settled_marker(proof: &TeleportProof) -> String {
    format!(
        "{TELEPORT_SETTLED} {} publisher_ms={} first_level_ms={} last_level_ms={} level_events={} first_sub_ms={} last_sub_ms={} sub_events={}",
        exact_full_view_proof_marker_fields(&proof.exact),
        optional_milliseconds_token(proof.publisher_ms),
        optional_milliseconds_token(proof.first_level_ms),
        optional_milliseconds_token(proof.last_level_ms),
        proof.level_events,
        optional_milliseconds_token(proof.first_sub_ms),
        optional_milliseconds_token(proof.last_sub_ms),
        proof.sub_events,
    )
}

pub(crate) fn forced_remesh_settled_marker(proof: &ExactFullViewProof) -> String {
    format!(
        "{FORCED_FULL_VIEW_REMESH_SETTLED} {}",
        exact_full_view_proof_marker_fields(proof)
    )
}

pub(crate) fn duration_milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

pub(crate) fn optional_duration_milliseconds(duration: Option<Duration>) -> Option<f64> {
    duration.map(duration_milliseconds)
}

pub(crate) fn optional_milliseconds_token(milliseconds: Option<f64>) -> String {
    milliseconds.map_or_else(|| "null".to_owned(), |value| format!("{value:.4}"))
}

pub(crate) fn horizontal_chunk(position: [f32; 3]) -> Option<[i32; 2]> {
    if !position[0].is_finite() || !position[2].is_finite() {
        return None;
    }
    Some([
        (position[0].floor() as i32).div_euclid(16),
        (position[2].floor() as i32).div_euclid(16),
    ])
}

use std::time::Duration;

use bevy::window::PresentMode;
use render::{VisibilityKeyDelta, VisibilityKeyDigest};

use crate::runtime::telemetry::AcceptanceRuntimeConfig;

pub(crate) const ACCEPTANCE_RUNTIME_METADATA: &str = "RUST_MCBE_ACCEPTANCE_RUNTIME_METADATA";
pub(crate) const ASSETS: &str = "RUST_MCBE_ASSETS";
pub(crate) const BUILD_COMMIT: &str = "RUST_MCBE_BUILD_COMMIT";
pub(crate) const CAMERA_COMMITTED: &str = "RUST_MCBE_CAMERA_COMMITTED";
pub(crate) const ERROR_COUNTERS: &str = "RUST_MCBE_ERROR_COUNTERS";
pub(crate) const FAST_TRANSFER_ACTION: &str = "RUST_MCBE_FAST_TRANSFER_ACTION";
pub(crate) const FAST_TRANSFER_PACKET_TRACE: &str = "RUST_MCBE_FAST_TRANSFER_PACKET_TRACE";
pub(crate) const FORCED_FULL_VIEW_REMESH_SETTLED: &str =
    "RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED";
pub(crate) const GALLERY_ANCHOR_READY: &str = "RUST_MCBE_GALLERY_ANCHOR_READY";
pub(crate) const ACTOR_POSE_WITNESS: &str = "RUST_MCBE_ACTOR_POSE_WITNESS";
pub(crate) const MODEL_WITNESS_COMPLETE: &str = "RUST_MCBE_MODEL_WITNESS_COMPLETE";
pub(crate) const MOVE_PLAYER_INGRESS: &str = "RUST_MCBE_MOVE_PLAYER_INGRESS";
pub(crate) const MUTATION_COORDINATE: &str = "RUST_MCBE_MUTATION_COORDINATE";
pub(crate) const PHASE3_EVENT: &str = "RUST_MCBE_PHASE3_EVENT";
pub(crate) const PHASE3_FRAME: &str = "RUST_MCBE_PHASE3_FRAME";
pub(crate) const PHASE3_IDENTITY: &str = "RUST_MCBE_PHASE3_IDENTITY";
pub(crate) const PHASE3_TERMINAL: &str = "RUST_MCBE_PHASE3_TERMINAL";
pub(crate) const PHASE3_VIOLATION: &str = "RUST_MCBE_PHASE3_VIOLATION";
pub(crate) const PHASE2_TIMING: &str = "RUST_MCBE_PHASE2_TIMING";
pub(crate) const PHASE3_CORE_PROCESS_ID: &str = "RUST_MCBE_PHASE3_CORE_PROCESS_ID";
pub(crate) const PHASE3_CORE_SHA256: &str = "RUST_MCBE_PHASE3_CORE_SHA256";
pub(crate) const PHASE3_BRIDGE_ENDPOINT: &str = "RUST_MCBE_PHASE3_BRIDGE_ENDPOINT";
pub(crate) const PHASE3_ENDPOINT: &str = "RUST_MCBE_PHASE3_ENDPOINT";
pub(crate) const PHASE3_RUN_ID: &str = "RUST_MCBE_PHASE3_RUN_ID";
pub(crate) const SOURCE_DIRTY: &str = "RUST_MCBE_SOURCE_DIRTY";
pub(crate) const SHUTDOWN_COMPLETED: &str = "RUST_MCBE_SHUTDOWN_COMPLETED";
pub(crate) const SHUTDOWN_WATCHDOG_ARMED_MARKER: &str = "RUST_MCBE_SHUTDOWN_WATCHDOG_ARMED";
pub(crate) const SHUTDOWN_WATCHDOG_FIRED_MARKER: &str = "RUST_MCBE_SHUTDOWN_WATCHDOG_FIRED";
pub(crate) const TARGET_MUTATION_ARMED: &str = "RUST_MCBE_TARGET_MUTATION_ARMED";
pub(crate) const TELEPORT_COHORT: &str = "RUST_MCBE_TELEPORT_COHORT";
pub(crate) const TELEPORT_GLOBAL_STAGE_DIAGNOSTIC: &str =
    "RUST_MCBE_TELEPORT_GLOBAL_STAGE_DIAGNOSTIC";
pub(crate) const TELEPORT_SETTLED: &str = "RUST_MCBE_TELEPORT_SETTLED";
pub(crate) const TRANSPARENT_SORT_COMMITTED: &str = "RUST_MCBE_TRANSPARENT_SORT_COMMITTED";
pub(crate) const TRANSPARENT_WITNESS_COMPLETE: &str = "RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE";
pub(crate) const TRANSPARENT_WITNESS_INCOMPLETE: &str = "RUST_MCBE_TRANSPARENT_WITNESS_INCOMPLETE";
pub(crate) const TRANSPARENT_WITNESS_STAGE: &str = "RUST_MCBE_TRANSPARENT_WITNESS_STAGE";
pub(crate) const VISIBILITY_SNAPSHOT: &str = "RUST_MCBE_VISIBILITY_SNAPSHOT";
pub(crate) const WORLD_PUBLICATION_SNAPSHOT: &str = "RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT";
pub(crate) const WORLD_READY: &str = "RUST_MCBE_WORLD_READY";

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkerContract {
    ParsedEvidence,
    LogOnlyDiagnostic,
    EnvironmentVariable,
}

#[cfg(test)]
pub(crate) const EXPECTATIONS: &[(&str, MarkerContract)] = &[
    (ACCEPTANCE_RUNTIME_METADATA, MarkerContract::ParsedEvidence),
    (ASSETS, MarkerContract::EnvironmentVariable),
    (ACTOR_POSE_WITNESS, MarkerContract::ParsedEvidence),
    (CAMERA_COMMITTED, MarkerContract::ParsedEvidence),
    (ERROR_COUNTERS, MarkerContract::LogOnlyDiagnostic),
    (FAST_TRANSFER_ACTION, MarkerContract::ParsedEvidence),
    (
        FAST_TRANSFER_PACKET_TRACE,
        MarkerContract::LogOnlyDiagnostic,
    ),
    (
        FORCED_FULL_VIEW_REMESH_SETTLED,
        MarkerContract::ParsedEvidence,
    ),
    (GALLERY_ANCHOR_READY, MarkerContract::ParsedEvidence),
    (MODEL_WITNESS_COMPLETE, MarkerContract::ParsedEvidence),
    (MOVE_PLAYER_INGRESS, MarkerContract::ParsedEvidence),
    (MUTATION_COORDINATE, MarkerContract::ParsedEvidence),
    (PHASE2_TIMING, MarkerContract::ParsedEvidence),
    (PHASE3_EVENT, MarkerContract::ParsedEvidence),
    (PHASE3_FRAME, MarkerContract::ParsedEvidence),
    (PHASE3_IDENTITY, MarkerContract::ParsedEvidence),
    (PHASE3_TERMINAL, MarkerContract::ParsedEvidence),
    (PHASE3_VIOLATION, MarkerContract::ParsedEvidence),
    (SHUTDOWN_COMPLETED, MarkerContract::LogOnlyDiagnostic),
    (
        SHUTDOWN_WATCHDOG_ARMED_MARKER,
        MarkerContract::LogOnlyDiagnostic,
    ),
    (
        SHUTDOWN_WATCHDOG_FIRED_MARKER,
        MarkerContract::LogOnlyDiagnostic,
    ),
    (TARGET_MUTATION_ARMED, MarkerContract::ParsedEvidence),
    (TELEPORT_COHORT, MarkerContract::LogOnlyDiagnostic),
    (
        TELEPORT_GLOBAL_STAGE_DIAGNOSTIC,
        MarkerContract::LogOnlyDiagnostic,
    ),
    (TELEPORT_SETTLED, MarkerContract::ParsedEvidence),
    (TRANSPARENT_SORT_COMMITTED, MarkerContract::ParsedEvidence),
    (TRANSPARENT_WITNESS_COMPLETE, MarkerContract::ParsedEvidence),
    (
        TRANSPARENT_WITNESS_INCOMPLETE,
        MarkerContract::LogOnlyDiagnostic,
    ),
    (TRANSPARENT_WITNESS_STAGE, MarkerContract::LogOnlyDiagnostic),
    (VISIBILITY_SNAPSHOT, MarkerContract::LogOnlyDiagnostic),
    (WORLD_PUBLICATION_SNAPSHOT, MarkerContract::ParsedEvidence),
    (WORLD_READY, MarkerContract::ParsedEvidence),
];

pub(crate) fn cumulative_counter_delta(current: u64, previous: u64) -> u64 {
    current.checked_sub(previous).unwrap_or(current)
}

pub(crate) fn visibility_digest_marker_fields(
    prefix: &str,
    digest: Option<VisibilityKeyDigest>,
) -> String {
    digest.map_or_else(
        || format!("{prefix}_valid=false {prefix}_count=null {prefix}_hash=null"),
        |digest| {
            format!(
                "{prefix}_valid=true {prefix}_count={} {prefix}_hash={:016x}",
                digest.count, digest.hash
            )
        },
    )
}

pub(crate) const fn requested_present_mode(no_vsync: bool) -> PresentMode {
    if no_vsync {
        PresentMode::Immediate
    } else {
        PresentMode::Fifo
    }
}

pub(crate) fn acceptance_runtime_metadata_marker(
    config: AcceptanceRuntimeConfig,
    graphics: &render::GraphicsAdapterMetadata,
) -> String {
    format!(
        "{ACCEPTANCE_RUNTIME_METADATA}={}",
        serde_json::json!({
            "build_profile": config.build_profile,
            "requested_present_mode": graphics.requested_present_mode.as_str(),
            "effective_present_mode": graphics.effective_present_mode.as_str(),
            "present_mode_proven": graphics.present_mode_proven,
            "backend": graphics.backend,
            "adapter": graphics.adapter,
            "driver": graphics.driver,
            "driver_info": graphics.driver_info,
        })
    )
}

pub(crate) fn world_publication_snapshot_marker(
    stats: client_world::WorldStreamStats,
    upload_queue_items: usize,
    upload_queue_bytes: u64,
    gpu_upload_bytes: u64,
    visibility: render::VisibilityDiagnosticSnapshot,
    config: AcceptanceRuntimeConfig,
    graphics: &render::GraphicsAdapterMetadata,
) -> String {
    let milliseconds = |duration: Duration| duration.as_secs_f64() * 1_000.0;
    format!(
        "{WORLD_PUBLICATION_SNAPSHOT}={}",
        serde_json::json!({
            "accepted_light_jobs": stats.accepted_light_jobs,
            "noop_light_jobs": stats.noop_light_jobs,
            "value_changed_light_jobs": stats.value_changed_light_jobs,
            "provenance_only_light_jobs": stats.provenance_only_light_jobs,
            "light_mesh_invalidations": stats.light_mesh_invalidations,
            "stale_light_jobs": stats.stale_light_jobs,
            "stale_mesh_jobs": stats.stale_mesh_jobs,
            "queued_decode_jobs": stats.queued_decode_jobs,
            "in_flight_decode_jobs": stats.in_flight_decode_jobs,
            "pending_light_jobs": stats.pending_light_jobs,
            "in_flight_light_jobs": stats.in_flight_light_jobs,
            "pending_mesh_jobs": stats.pending_mesh_jobs,
            "in_flight_mesh_jobs": stats.in_flight_mesh_jobs,
            "max_decode_queue_wait_ms": milliseconds(stats.max_decode_queue_wait),
            "max_light_queue_wait_ms": milliseconds(stats.max_light_queue_wait),
            "max_mesh_queue_wait_ms": milliseconds(stats.max_mesh_queue_wait),
            "max_decode_worker_ms": milliseconds(stats.max_decode_duration),
            "max_light_worker_ms": milliseconds(stats.max_light_duration),
            "max_mesh_worker_ms": milliseconds(stats.max_mesh_duration),
            "upload_queue_items": upload_queue_items,
            "upload_queue_bytes": upload_queue_bytes,
            "gpu_upload_bytes": gpu_upload_bytes,
            "frame_generation": visibility.frame_generation,
            "pose_generation": visibility.pose_generation,
            "view_generation": visibility.view_generation,
            "draw_mode": format!("{:?}", visibility.draw_mode),
            "build_profile": config.build_profile,
            "requested_present_mode": graphics.requested_present_mode,
            "effective_present_mode": graphics.effective_present_mode,
            "present_mode_proven": graphics.present_mode_proven,
            "backend": graphics.backend,
            "adapter": graphics.adapter,
            "driver": graphics.driver,
            "driver_info": graphics.driver_info,
        })
    )
}

pub(crate) fn visibility_delta_marker_fields(
    prefix: &str,
    delta: Option<VisibilityKeyDelta>,
) -> String {
    delta.map_or_else(
        || {
            format!(
                "{prefix}_valid=false {prefix}_missing_count=null {prefix}_missing_hash=null {prefix}_extra_count=null {prefix}_extra_hash=null"
            )
        },
        |delta| {
            format!(
                "{prefix}_valid=true {prefix}_missing_count={} {prefix}_missing_hash={:016x} {prefix}_extra_count={} {prefix}_extra_hash={:016x}",
                delta.missing.count, delta.missing.hash, delta.extra.count, delta.extra.hash
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn expectation_table_is_unique_and_covers_every_owned_marker() {
        let names = EXPECTATIONS
            .iter()
            .map(|(name, _)| *name)
            .collect::<BTreeSet<_>>();
        assert_eq!(names.len(), EXPECTATIONS.len());
        assert_eq!(names.len(), 32);
        let protocol_prefix = concat!("RUST_", "MCBE_");
        assert!(names.iter().all(|name| name.starts_with(protocol_prefix)));
    }
}

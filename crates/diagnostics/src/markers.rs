use std::time::Duration;

use render_model::{VisibilityKeyDelta, VisibilityKeyDigest};

pub const ACCEPTANCE_BRIDGE_ENDPOINT: &str = "CINNABAR_ACCEPTANCE_BRIDGE_ENDPOINT";
pub const ACCEPTANCE_CORE_PROCESS_ID: &str = "CINNABAR_ACCEPTANCE_CORE_PROCESS_ID";
pub const ACCEPTANCE_CORE_SHA256: &str = "CINNABAR_ACCEPTANCE_CORE_SHA256";
pub const ACCEPTANCE_ENDPOINT: &str = "CINNABAR_ACCEPTANCE_ENDPOINT";
pub const ACCEPTANCE_RUN_ID: &str = "CINNABAR_ACCEPTANCE_RUN_ID";
pub const ACCEPTANCE_RUNTIME_METADATA: &str = "CINNABAR_ACCEPTANCE_RUNTIME_METADATA";
pub const ANCHOR_PROBE: &str = "CINNABAR_ANCHOR_PROBE";
pub const AUDIO_WIRE_EVIDENCE: &str = "CINNABAR_AUDIO_WIRE_EVIDENCE";
pub const BUILD_COMMIT: &str = "CINNABAR_BUILD_COMMIT";
pub const CAMERA_COMMITTED: &str = "CINNABAR_CAMERA_COMMITTED";
pub const CHUNK_PUBLICATION: &str = "CINNABAR_CHUNK_PUBLICATION";
pub const CHUNK_PUBLICATION_TIMING: &str = "CINNABAR_CHUNK_PUBLICATION_TIMING";
pub const ERROR_COUNTERS: &str = "CINNABAR_ERROR_COUNTERS";
pub const FAST_TRANSFER_PACKET_TRACE: &str = "CINNABAR_FAST_TRANSFER_PACKET_TRACE";
pub const FORCED_FULL_VIEW_REMESH_SETTLED: &str = "CINNABAR_FORCED_FULL_VIEW_REMESH_SETTLED";
pub const GALLERY_ANCHOR_READY: &str = "CINNABAR_GALLERY_ANCHOR_READY";
/// Log filter spec layered over the defaults and `RUST_LOG`; see `crate::logging`.
pub const LOG: &str = "CINNABAR_LOG";
/// `1` also writes each log event as JSON Lines beside the session log.
pub const LOG_JSON: &str = "CINNABAR_LOG_JSON";
pub const MODEL_WITNESS_COMPLETE: &str = "CINNABAR_MODEL_WITNESS_COMPLETE";
pub const MOVEMENT_TRACE: &str = "CINNABAR_MOVEMENT_TRACE";
pub const MOVE_PLAYER_INGRESS: &str = "CINNABAR_MOVE_PLAYER_INGRESS";
pub const MUTATION_COORDINATE: &str = "CINNABAR_MUTATION_COORDINATE";
pub const MOVEMENT_EVENT: &str = "CINNABAR_MOVEMENT_EVENT";
pub const MOVEMENT_FRAME: &str = "CINNABAR_MOVEMENT_FRAME";
pub const MOVEMENT_IDENTITY: &str = "CINNABAR_MOVEMENT_IDENTITY";
pub const MOVEMENT_TERMINAL: &str = "CINNABAR_MOVEMENT_TERMINAL";
pub const MOVEMENT_VIOLATION: &str = "CINNABAR_MOVEMENT_VIOLATION";
pub const SOURCE_DIRTY: &str = "CINNABAR_SOURCE_DIRTY";
pub const SHUTDOWN_COMPLETED: &str = "CINNABAR_SHUTDOWN_COMPLETED";
pub const SHUTDOWN_WATCHDOG_ARMED_MARKER: &str = "CINNABAR_SHUTDOWN_WATCHDOG_ARMED";
pub const SHUTDOWN_WATCHDOG_FIRED_MARKER: &str = "CINNABAR_SHUTDOWN_WATCHDOG_FIRED";
pub const TARGET_MUTATION_ARMED: &str = "CINNABAR_TARGET_MUTATION_ARMED";
pub const TELEPORT_ACK: &str = "CINNABAR_TELEPORT_ACK";
pub const TELEPORT_COHORT: &str = "CINNABAR_TELEPORT_COHORT";
pub const TELEPORT_GLOBAL_STAGE_DIAGNOSTIC: &str = "CINNABAR_TELEPORT_GLOBAL_STAGE_DIAGNOSTIC";
pub const TELEPORT_SETTLED: &str = "CINNABAR_TELEPORT_SETTLED";
pub const STAGE_PROFILE: &str = "CINNABAR_STAGE_PROFILE";
pub const STAGE_PROFILE_FRAMES: &str = "CINNABAR_STAGE_PROFILE_FRAMES";
pub const TRANSPARENT_SORT_COMMITTED: &str = "CINNABAR_TRANSPARENT_SORT_COMMITTED";
pub const TRANSPARENT_WITNESS_COMPLETE: &str = "CINNABAR_TRANSPARENT_WITNESS_COMPLETE";
pub const TRANSPARENT_WITNESS_INCOMPLETE: &str = "CINNABAR_TRANSPARENT_WITNESS_INCOMPLETE";
pub const TRANSPARENT_WITNESS_STAGE: &str = "CINNABAR_TRANSPARENT_WITNESS_STAGE";
pub const VISIBILITY_SNAPSHOT: &str = "CINNABAR_VISIBILITY_SNAPSHOT";
pub const WORLD_PUBLICATION_SNAPSHOT: &str = "CINNABAR_WORLD_PUBLICATION_SNAPSHOT";
pub const WORLD_READY: &str = "CINNABAR_WORLD_READY";
/// `0` keeps every world-stream poll on the frame thread, for same-binary comparisons.
pub const WORLD_SERVICE: &str = "CINNABAR_WORLD_SERVICE";

#[cfg(test)]
use client_ui::diagnostic_markers::{
    CRAFT_OBSERVATION, FAST_TRANSFER_ACTION, FORM_SHAPE_PROBE, LOADING_MILESTONE,
    USE_ON_IDENTITY_EVIDENCE,
};

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerContract {
    ParsedEvidence,
    LogOnlyDiagnostic,
    EnvironmentVariable,
}

#[cfg(test)]
pub const EXPECTATIONS: &[(&str, MarkerContract)] = &[
    (ACCEPTANCE_RUNTIME_METADATA, MarkerContract::ParsedEvidence),
    (ANCHOR_PROBE, MarkerContract::EnvironmentVariable),
    (
        assets::carriers::startup::markers::ASSETS,
        MarkerContract::EnvironmentVariable,
    ),
    (AUDIO_WIRE_EVIDENCE, MarkerContract::EnvironmentVariable),
    (FORM_SHAPE_PROBE, MarkerContract::EnvironmentVariable),
    (CRAFT_OBSERVATION, MarkerContract::EnvironmentVariable),
    (
        USE_ON_IDENTITY_EVIDENCE,
        MarkerContract::EnvironmentVariable,
    ),
    (CAMERA_COMMITTED, MarkerContract::ParsedEvidence),
    (CHUNK_PUBLICATION, MarkerContract::ParsedEvidence),
    (CHUNK_PUBLICATION_TIMING, MarkerContract::ParsedEvidence),
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
    (LOADING_MILESTONE, MarkerContract::LogOnlyDiagnostic),
    (LOG, MarkerContract::EnvironmentVariable),
    (LOG_JSON, MarkerContract::EnvironmentVariable),
    (MODEL_WITNESS_COMPLETE, MarkerContract::ParsedEvidence),
    (MOVE_PLAYER_INGRESS, MarkerContract::ParsedEvidence),
    (MOVEMENT_TRACE, MarkerContract::EnvironmentVariable),
    (MUTATION_COORDINATE, MarkerContract::ParsedEvidence),
    (MOVEMENT_EVENT, MarkerContract::ParsedEvidence),
    (MOVEMENT_FRAME, MarkerContract::ParsedEvidence),
    (MOVEMENT_IDENTITY, MarkerContract::ParsedEvidence),
    (MOVEMENT_TERMINAL, MarkerContract::ParsedEvidence),
    (MOVEMENT_VIOLATION, MarkerContract::ParsedEvidence),
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
    (TELEPORT_ACK, MarkerContract::EnvironmentVariable),
    (TELEPORT_COHORT, MarkerContract::LogOnlyDiagnostic),
    (
        TELEPORT_GLOBAL_STAGE_DIAGNOSTIC,
        MarkerContract::LogOnlyDiagnostic,
    ),
    (TELEPORT_SETTLED, MarkerContract::ParsedEvidence),
    (STAGE_PROFILE, MarkerContract::EnvironmentVariable),
    (STAGE_PROFILE_FRAMES, MarkerContract::EnvironmentVariable),
    (BUILD_COMMIT, MarkerContract::EnvironmentVariable),
    (SOURCE_DIRTY, MarkerContract::EnvironmentVariable),
    (ACCEPTANCE_RUN_ID, MarkerContract::EnvironmentVariable),
    (ACCEPTANCE_ENDPOINT, MarkerContract::EnvironmentVariable),
    (ACCEPTANCE_CORE_SHA256, MarkerContract::EnvironmentVariable),
    (
        ACCEPTANCE_CORE_PROCESS_ID,
        MarkerContract::EnvironmentVariable,
    ),
    (
        ACCEPTANCE_BRIDGE_ENDPOINT,
        MarkerContract::EnvironmentVariable,
    ),
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
    (WORLD_SERVICE, MarkerContract::EnvironmentVariable),
];

pub fn cumulative_counter_delta(current: u64, previous: u64) -> u64 {
    current.checked_sub(previous).unwrap_or(current)
}

pub fn visibility_digest_marker_fields(
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

pub fn acceptance_runtime_metadata_marker(
    build_profile: &str,
    graphics: &render_model::GraphicsAdapterMetadata,
) -> String {
    format!(
        "{ACCEPTANCE_RUNTIME_METADATA}={}",
        serde_json::json!({
            "build_profile": build_profile,
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

pub fn world_publication_snapshot_marker(
    stats: chunk_pipeline::WorldStreamStats,
    upload_queue_items: usize,
    upload_queue_bytes: u64,
    gpu_upload_bytes: u64,
    visibility: render_model::VisibilityDiagnosticSnapshot,
    build_profile: &str,
    graphics: &render_model::GraphicsAdapterMetadata,
) -> String {
    let milliseconds = |duration: Duration| duration.as_secs_f64() * 1_000.0;
    let visibility_valid = visibility.frame_generation != 0;
    let mut snapshot = serde_json::json!({
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
        "frame_generation": visibility_valid.then_some(visibility.frame_generation),
        "pose_generation": visibility_valid.then_some(visibility.pose_generation.get()),
        "view_generation": visibility_valid.then_some(visibility.view_generation),
        "draw_mode": visibility_valid.then(|| format!("{:?}", visibility.draw_mode)),
        "build_profile": build_profile,
        "requested_present_mode": graphics.requested_present_mode,
        "effective_present_mode": graphics.effective_present_mode,
        "present_mode_proven": graphics.present_mode_proven,
        "backend": graphics.backend,
        "adapter": graphics.adapter,
        "driver": graphics.driver,
        "driver_info": graphics.driver_info,
    });
    if !visibility_valid {
        snapshot["visibility_snapshot_valid"] = serde_json::json!(false);
    }
    format!("{WORLD_PUBLICATION_SNAPSHOT}={snapshot}")
}

pub fn visibility_delta_marker_fields(prefix: &str, delta: Option<VisibilityKeyDelta>) -> String {
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
    /// Every owned marker has one expectation, including startup environment inputs.
    fn expectation_table_is_unique_and_covers_every_owned_marker() {
        let names = EXPECTATIONS
            .iter()
            .map(|(name, _)| *name)
            .collect::<BTreeSet<_>>();
        assert_eq!(names.len(), EXPECTATIONS.len());
        let protocol_prefix = concat!("CINNABAR", "_");
        let declarations = [
            include_str!("markers.rs"),
            include_str!("../../../crates/client-ui/src/diagnostic_markers.rs"),
            include_str!("../../../crates/assets/src/carriers/startup/markers.rs"),
        ]
        .into_iter()
        .flat_map(|source| source.split("#[cfg(test)]").next().unwrap().split('"'))
        .filter(|value| value.starts_with(protocol_prefix))
        .collect::<BTreeSet<_>>();
        assert_eq!(names, declarations);
        assert!(EXPECTATIONS.contains(&(CRAFT_OBSERVATION, MarkerContract::EnvironmentVariable)));
        assert!(EXPECTATIONS.contains(&(FORM_SHAPE_PROBE, MarkerContract::EnvironmentVariable)));
        assert!(names.iter().all(|name| name.starts_with(protocol_prefix)));
    }
}

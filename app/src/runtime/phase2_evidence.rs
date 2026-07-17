use client_world::{
    BuildProfileIdentity, CohortManifestIdentity, Phase2PresentationSnapshot,
    Phase2PublicationSnapshot, PresentModeIdentity, PublicationStageCounters, StageDurations,
    SubChunkOutcomeCounters,
};
use protocol::BlobCacheStats;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CombinedPhase2Snapshot {
    pub(crate) publication: Phase2PublicationSnapshot,
    pub(crate) presentation: Phase2PresentationSnapshot,
    pub(crate) present_mode_proven: bool,
    pub(crate) client_blob_cache_enabled: bool,
    pub(crate) client_blob_cache: BlobCacheStats,
}

pub(crate) fn phase2_publication_line_if_changed(
    previous: &mut Option<CombinedPhase2Snapshot>,
    current: CombinedPhase2Snapshot,
) -> Option<String> {
    if previous.as_ref() == Some(&current) {
        return None;
    }
    *previous = Some(current);
    Some(format!(
        "PHASE2_PUBLICATION={}",
        combined_snapshot_json(current)
    ))
}

fn combined_snapshot_json(snapshot: CombinedPhase2Snapshot) -> Value {
    let publication = snapshot.publication;
    let presentation = snapshot.presentation;
    json!({
        "client_blob_cache_enabled": snapshot.client_blob_cache_enabled,
        "client_blob_cache": blob_cache_json(snapshot.client_blob_cache),
        "publication": {
            "session_generation": publication.session_generation,
            "player_column": {
                "dimension": publication.player_column.dimension,
                "x": publication.player_column.x,
                "z": publication.player_column.z,
            },
            "publisher_radius_chunks": publication.publisher_radius_chunks,
            "required_cohort_hash": format!("{:016x}", publication.required_cohort_hash),
            "required_columns": publication.required_columns,
            "loaded_required_columns": publication.loaded_required_columns,
            "stages": stage_counters_json(publication.stages),
            "outcomes": outcomes_json(publication.outcomes),
            "max_queue_wait_us": durations_json(publication.max_queue_wait),
            "max_worker_time_us": durations_json(publication.max_worker_time),
        },
        "presentation": {
            "build_profile": build_profile_name(presentation.build_profile),
            "graphics_identity_sha256": lower_hex(&presentation.graphics_identity_sha256),
            "requested_present_mode": present_mode_name(presentation.requested_present_mode),
            "effective_present_mode": present_mode_name(presentation.effective_present_mode),
            "present_mode_proven": snapshot.present_mode_proven,
            "assets_manifest_sha256": lower_hex(&presentation.assets_manifest_sha256),
            "publisher_disk": cohort_json(presentation.publisher_disk),
            "resident": cohort_json(presentation.resident),
            "allocation": cohort_json(presentation.allocation),
            "visible": cohort_json(presentation.visible),
            "submitted": cohort_json(presentation.submitted),
            "gpu_presented": cohort_json(presentation.gpu_presented),
        }
    })
}

fn blob_cache_json(stats: BlobCacheStats) -> Value {
    json!({
        "hashes_classified": stats.hashes_classified,
        "hits": stats.hits,
        "misses": stats.misses,
        "admitted_blobs": stats.admitted_blobs,
        "rejected_blobs": stats.rejected_blobs,
        "evictions": stats.evictions,
        "pending_transactions": stats.pending_transactions,
        "pending_bytes": stats.pending_bytes,
        "pending_resets": stats.pending_resets,
        "reconstructed_level_chunks": stats.reconstructed_level_chunks,
        "reconstructed_sub_chunks": stats.reconstructed_sub_chunks,
    })
}

fn stage_counters_json(stages: PublicationStageCounters) -> Value {
    json!({
        "requests_constructed": stages.requests_constructed,
        "requests_sent": stages.requests_sent,
        "responses_admitted": stages.responses_admitted,
        "decode_jobs_dispatched": stages.decode_jobs_dispatched,
        "decode_jobs_completed": stages.decode_jobs_completed,
        "subchunks_committed": stages.subchunks_committed,
        "light_jobs_dispatched": stages.light_jobs_dispatched,
        "light_jobs_completed": stages.light_jobs_completed,
        "mesh_jobs_dispatched": stages.mesh_jobs_dispatched,
        "mesh_jobs_completed": stages.mesh_jobs_completed,
        "mesh_changes_queued": stages.mesh_changes_queued,
        "mesh_changes_dequeued": stages.mesh_changes_dequeued,
        "mesh_uploads_acknowledged": stages.mesh_uploads_acknowledged,
        "requests_ready": stages.requests_ready,
        "subchunks_awaiting_response": stages.subchunks_awaiting_response,
        "decode_jobs_queued": stages.decode_jobs_queued,
        "decode_jobs_in_flight": stages.decode_jobs_in_flight,
        "light_jobs_queued": stages.light_jobs_queued,
        "light_jobs_in_flight": stages.light_jobs_in_flight,
        "mesh_jobs_queued": stages.mesh_jobs_queued,
        "mesh_jobs_in_flight": stages.mesh_jobs_in_flight,
        "mesh_changes_pending": stages.mesh_changes_pending,
        "mesh_uploads_unacknowledged": stages.mesh_uploads_unacknowledged,
    })
}

fn outcomes_json(outcomes: SubChunkOutcomeCounters) -> Value {
    json!({
        "success": outcomes.success,
        "all_air": outcomes.all_air,
        "unavailable": outcomes.unavailable,
        "malformed": outcomes.malformed,
        "stale": outcomes.stale,
        "timed_out": outcomes.timed_out,
    })
}

fn durations_json(durations: StageDurations) -> Value {
    json!({
        "decode": duration_micros(durations.decode),
        "lighting": duration_micros(durations.lighting),
        "meshing": duration_micros(durations.meshing),
    })
}

fn duration_micros(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn cohort_json(identity: CohortManifestIdentity) -> Value {
    json!({
        "session_generation": identity.session_generation,
        "required_cohort_hash": format!("{:016x}", identity.required_cohort_hash),
        "generation_manifest_hash": format!("{:016x}", identity.generation_manifest_hash),
        "entry_count": identity.entry_count,
    })
}

pub(crate) const fn build_profile_identity(profile: &str) -> BuildProfileIdentity {
    match profile.as_bytes() {
        b"debug" => BuildProfileIdentity::Debug,
        b"release" => BuildProfileIdentity::Release,
        _ => BuildProfileIdentity::Unknown,
    }
}

pub(crate) fn present_mode_identity(mode: &str) -> PresentModeIdentity {
    if mode.eq_ignore_ascii_case("fifo") {
        PresentModeIdentity::Fifo
    } else if mode.eq_ignore_ascii_case("immediate") {
        PresentModeIdentity::Immediate
    } else {
        PresentModeIdentity::Unknown
    }
}

pub(crate) fn cohort_identity(
    session_generation: u64,
    required_cohort_hash: u64,
    stage_generation: u64,
    digest: Option<render::VisibilityKeyDigest>,
) -> CohortManifestIdentity {
    digest.map_or(
        CohortManifestIdentity {
            session_generation,
            required_cohort_hash,
            generation_manifest_hash: 0,
            entry_count: 0,
        },
        |digest| CohortManifestIdentity {
            session_generation,
            required_cohort_hash,
            generation_manifest_hash: stage_generation ^ digest.hash,
            entry_count: usize::try_from(digest.count).unwrap_or(usize::MAX),
        },
    )
}

pub(crate) fn generation_manifest_identity(
    session_generation: u64,
    required_cohort_hash: u64,
    manifest: &[(world::SubChunkKey, u64)],
) -> CohortManifestIdentity {
    CohortManifestIdentity {
        session_generation,
        required_cohort_hash,
        generation_manifest_hash: crate::metrics::deterministic_manifest_hash(manifest),
        entry_count: manifest.len(),
    }
}

pub(crate) fn sha256_identity_from_hex_or_text(value: &str) -> [u8; 32] {
    if value.len() == 64 {
        let mut bytes = [0_u8; 32];
        let mut valid = true;
        for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
            let pair = std::str::from_utf8(chunk).ok();
            if let Some(parsed) = pair.and_then(|pair| u8::from_str_radix(pair, 16).ok()) {
                bytes[index] = parsed;
            } else {
                valid = false;
                break;
            }
        }
        if valid {
            return bytes;
        }
    }
    Sha256::digest(value.as_bytes()).into()
}

pub(crate) fn graphics_identity_sha256(graphics: &render::GraphicsAdapterMetadata) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in [
        graphics.backend.as_str(),
        graphics.adapter.as_str(),
        graphics.driver.as_str(),
        graphics.driver_info.as_str(),
        graphics.requested_present_mode.as_str(),
        graphics.effective_present_mode.as_str(),
    ] {
        hasher.update(u64::try_from(part.len()).unwrap_or(u64::MAX).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    hasher.update([u8::from(graphics.present_mode_proven)]);
    hasher.finalize().into()
}

fn build_profile_name(identity: BuildProfileIdentity) -> &'static str {
    match identity {
        BuildProfileIdentity::Unknown => "unknown",
        BuildProfileIdentity::Debug => "debug",
        BuildProfileIdentity::Release => "release",
    }
}

fn present_mode_name(identity: PresentModeIdentity) -> &'static str {
    match identity {
        PresentModeIdentity::Unknown => "unknown",
        PresentModeIdentity::Fifo => "fifo",
        PresentModeIdentity::Immediate => "immediate",
    }
}

pub(crate) fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut result = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(result, "{byte:02x}");
    }
    result
}

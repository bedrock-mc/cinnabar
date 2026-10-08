//! Bounded, rate-limited blob-cache pressure telemetry for the play pump.

use super::*;

pub(super) fn try_emit_blob_cache_telemetry<S: NetworkSession, P>(
    session: &S,
    control_event_tx: &mpsc::Sender<NetworkControlEvent<P>>,
    last_stats: &mut Option<BlobCacheStats>,
) {
    if !session.blob_cache_enabled() {
        return;
    }
    let stats = session.blob_cache_stats();
    if *last_stats == Some(stats) {
        return;
    }
    if control_event_tx
        .try_send(NetworkControlEvent::BlobCacheTelemetry {
            enabled: true,
            stats,
        })
        .is_ok()
    {
        emit_bounded_blob_cache_warning(last_stats.unwrap_or_default(), stats);
        emit_blob_cache_telemetry(stats);
        *last_stats = Some(stats);
    }
}

pub(super) async fn send_final_blob_cache_telemetry<S: NetworkSession, P>(
    session: &S,
    control_event_tx: &mpsc::Sender<NetworkControlEvent<P>>,
) -> bool {
    if !session.blob_cache_enabled() {
        return true;
    }
    let stats = session.blob_cache_stats();
    emit_blob_cache_telemetry(stats);
    matches!(
        tokio::time::timeout(
            FINAL_CONTROL_FLUSH_TIMEOUT,
            control_event_tx.send(NetworkControlEvent::BlobCacheTelemetry {
                enabled: true,
                stats,
            }),
        )
        .await,
        Ok(Ok(()))
    )
}

pub(super) fn emit_blob_cache_telemetry(stats: BlobCacheStats) {
    tracing::info!(
        target: "bedrock_client::blob_cache",
        hashes_classified = stats.hashes_classified,
        hits = stats.hits,
        misses = stats.misses,
        redundant_missing_requests = stats.redundant_missing_requests,
        admitted_blobs = stats.admitted_blobs,
        rejected_blobs = stats.rejected_blobs,
        evictions = stats.evictions,
        pending_transactions = stats.pending_transactions,
        pending_bytes = stats.pending_bytes,
        retained_cached_transactions = stats.retained_cached_transactions,
        ordinary_ready_events = stats.ordinary_ready_events,
        ordinary_ready_bytes = stats.ordinary_ready_bytes,
        recovery_ready_events = stats.recovery_ready_events,
        recovery_ready_bytes = stats.recovery_ready_bytes,
        pending_resets = stats.pending_resets,
        skipped_packets = stats.skipped_packets,
        skipped_world_events = stats.skipped_world_events,
        skipped_cached_packets = stats.skipped_cached_packets,
        skipped_miss_responses = stats.skipped_miss_responses,
        empty_miss_responses = stats.empty_miss_responses,
        cached_packet_semantic_shape = stats.cached_packet_semantic_shape,
        cached_packet_transaction_pressure = stats.cached_packet_transaction_pressure,
        cached_packet_pending_pressure = stats.cached_packet_pending_pressure,
        cached_packet_staged_pressure = stats.cached_packet_staged_pressure,
        cached_packet_reconstruction_pressure = stats.cached_packet_reconstruction_pressure,
        cached_packet_ready_pressure = stats.cached_packet_ready_pressure,
        miss_response_unsolicited = stats.miss_response_unsolicited,
        miss_response_integrity_rejection = stats.miss_response_integrity_rejection,
        miss_response_cache_pressure = stats.miss_response_cache_pressure,
        abandoned_cached_transactions = stats.abandoned_cached_transactions,
        recovery_requests = stats.recovery_requests,
        ordinary_backpressure = stats.ordinary_backpressure,
        reconstructed_level_chunks = stats.reconstructed_level_chunks,
        reconstructed_sub_chunks = stats.reconstructed_sub_chunks,
        "client blob cache counters"
    );
}

fn emit_bounded_blob_cache_warning(previous: BlobCacheStats, current: BlobCacheStats) {
    let cached_packet_due = bounded_counter_log_due(
        previous.skipped_cached_packets,
        current.skipped_cached_packets,
    );
    let miss_response_due = bounded_counter_log_due(
        previous.skipped_miss_responses,
        current.skipped_miss_responses,
    );
    if cached_packet_due || miss_response_due {
        tracing::warn!(
            target: "bedrock_client::blob_cache",
            skipped_cached_packets = current.skipped_cached_packets,
            skipped_miss_responses = current.skipped_miss_responses,
            cached_packet_semantic_shape = current.cached_packet_semantic_shape,
            cached_packet_transaction_pressure = current.cached_packet_transaction_pressure,
            cached_packet_pending_pressure = current.cached_packet_pending_pressure,
            cached_packet_staged_pressure = current.cached_packet_staged_pressure,
            cached_packet_reconstruction_pressure = current.cached_packet_reconstruction_pressure,
            cached_packet_ready_pressure = current.cached_packet_ready_pressure,
            miss_response_unsolicited = current.miss_response_unsolicited,
            miss_response_integrity_rejection = current.miss_response_integrity_rejection,
            miss_response_cache_pressure = current.miss_response_cache_pressure,
            "skipped semantically invalid client blob-cache packet"
        );
    }
}

pub(super) fn bounded_counter_log_due(previous: u64, current: u64) -> bool {
    current != 0 && current > previous && (previous == 0 || current.ilog2() > previous.ilog2())
}

//! Runtime metrics and shared diagnostic marker contracts.
pub mod bounded_file;
pub mod markers;
pub mod metrics;

/// Build identity recorded alongside runtime measurements.
#[derive(bevy::prelude::Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptanceRuntimeConfig {
    pub build_profile: &'static str,
}

/// Writes one diagnostic marker and flushes it immediately.
pub fn write_stdout_marker(stdout: &mut impl std::io::Write, marker: &str) {
    let _ = writeln!(stdout, "{marker}");
    let _ = stdout.flush();
}

/// Requested radius used by deterministic world-publication evidence.
pub const PHASE0_REQUESTED_RADIUS_CHUNKS: i32 = 16;

/// Reports a newly presented transparent generation when its reference count is known.
pub fn transparent_sort_committed_marker(
    last_presented_generation: u64,
    snapshot: metrics::TransparentSortMetricsSnapshot,
) -> Option<String> {
    (snapshot.presented_generation > last_presented_generation
        && snapshot.presented_generation == snapshot.committed_generation
        && snapshot.ref_count > 0)
        .then(|| {
            format!(
                "{marker} generation={} ref_count={}",
                snapshot.presented_generation,
                snapshot.ref_count,
                marker = markers::TRANSPARENT_SORT_COMMITTED
            )
        })
}

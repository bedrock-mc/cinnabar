//! Runtime metrics, process logging and shared diagnostic marker contracts.
pub mod bounded_file;
pub mod console;
#[cfg(not(target_arch = "wasm32"))]
pub mod logging;
pub mod markers;
pub mod metrics;
pub mod rate;
pub mod secret;
pub mod session;
#[cfg(not(target_arch = "wasm32"))]
pub mod session_log;

/// Writes one diagnostic marker and flushes it immediately.
pub fn write_stdout_marker(stdout: &mut impl std::io::Write, marker: &str) {
    let _ = writeln!(stdout, "{marker}");
    let _ = stdout.flush();
}

/// Requested radius used by deterministic world-publication evidence.
pub const EVIDENCE_REQUESTED_RADIUS_CHUNKS: i32 = 16;

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

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

/// Writes one diagnostic line through the console queue and its optional session file.
#[macro_export]
macro_rules! log_stderr {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!($crate::console::stderr(), $($arg)*);
    }};
}

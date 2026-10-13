//! Worker CPU counters available only to the headless benchmark fixtures.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static REQUESTS: AtomicU64 = AtomicU64::new(0);
static HITS: AtomicU64 = AtomicU64::new(0);
static BIOME_NS: AtomicU64 = AtomicU64::new(0);
static MESH_NS: AtomicU64 = AtomicU64::new(0);

/// Cumulative worker costs; take differences after fixture warm-up.
#[derive(Clone, Copy, Debug, Default)]
pub struct CpuCounters {
    pub requests: u64,
    pub cache_hits: u64,
    pub biome_ns: u64,
    pub mesh_ns: u64,
}

/// Reads completed worker costs without resetting other concurrent jobs.
pub fn cpu_counters() -> CpuCounters {
    CpuCounters {
        requests: REQUESTS.load(Ordering::Relaxed),
        cache_hits: HITS.load(Ordering::Relaxed),
        biome_ns: BIOME_NS.load(Ordering::Relaxed),
        mesh_ns: MESH_NS.load(Ordering::Relaxed),
    }
}

/// Counts complete record requests, including construction or cache lookup.
pub(in crate::stream) fn observe_record(duration: Duration) {
    REQUESTS.fetch_add(1, Ordering::Relaxed);
    BIOME_NS.fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
}

/// Counts complete mesh jobs, which include record construction.
pub(in crate::stream) fn observe_mesh(duration: Duration) {
    MESH_NS.fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);
}

/// Counts record requests satisfied without constructing a new record.
pub(in crate::stream) fn observe_cache_hit() {
    HITS.fetch_add(1, Ordering::Relaxed);
}

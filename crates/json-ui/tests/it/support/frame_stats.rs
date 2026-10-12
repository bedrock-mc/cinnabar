//! Per-operation wall time and Rust allocation counts for sequential benchmarks.

use std::time::{Duration, Instant};

/// Samples of wall time, Rust allocation calls and requested bytes.
#[derive(Default)]
pub struct FrameStats {
    samples: Vec<(Duration, usize, usize)>,
}

impl FrameStats {
    /// Measure one operation, excluding sample storage and reporting allocations.
    pub fn measure<T>(&mut self, operation: impl FnOnce() -> T) -> T {
        let ((result, elapsed), allocations) = test_alloc::measure(|| {
            let started = Instant::now();
            let result = operation();
            (result, started.elapsed())
        });
        self.samples
            .push((elapsed, allocations.calls, allocations.bytes));
        result
    }

    /// Report the cold first operation separately from warm nearest-rank percentiles.
    pub fn report(&mut self, name: &str) {
        let cold = self.samples.remove(0);
        self.samples.sort_unstable_by_key(|sample| sample.0);
        let rank = |percent: usize| (self.samples.len() * percent).div_ceil(100) - 1;
        let median = self.samples[rank(50)];
        let p99 = self.samples[rank(99)];
        let mut calls: Vec<_> = self.samples.iter().map(|sample| sample.1).collect();
        calls.sort_unstable();
        let mut bytes: Vec<_> = self.samples.iter().map(|sample| sample.2).collect();
        bytes.sort_unstable();
        eprintln!(
            "FRAME_DISTRIBUTION {name}: n={} cold_ms={:.3} median_ms={:.3} p99_ms={:.3} median_allocs={} median_bytes={}",
            self.samples.len(),
            cold.0.as_secs_f64() * 1e3,
            median.0.as_secs_f64() * 1e3,
            p99.0.as_secs_f64() * 1e3,
            calls[rank(50)],
            bytes[rank(50)],
        );
    }
}

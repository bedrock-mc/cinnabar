//! Per-operation wall time and Rust allocation counts for sequential benchmarks.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::{Duration, Instant},
};

thread_local! {
    static COUNTS: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

/// System allocator with opt-in counters confined to the benchmark thread.
pub struct CountedAllocator;

/// Count successful allocation requests only on the thread being measured.
fn count(bytes: usize) {
    let _ = COUNTS.try_with(|counts| {
        if let Some((calls, total)) = counts.get() {
            counts.set(Some((calls + 1, total + bytes)));
        }
    });
}

// SAFETY: Every allocation and deallocation delegates unchanged to System.
unsafe impl GlobalAlloc for CountedAllocator {
    /// Allocate using System and record the successful request.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            count(layout.size());
        }
        pointer
    }

    /// Allocate zeroed memory using System and record the successful request.
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            count(layout.size());
        }
        pointer
    }

    /// Resize using System and count the resulting allocation request.
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, size) };
        if !pointer.is_null() {
            count(size);
        }
        pointer
    }

    /// Free memory using the same allocator that produced it.
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }
}

/// Disable counting even if the measured operation panics.
struct Counting;

impl Drop for Counting {
    fn drop(&mut self) {
        COUNTS.with(|counts| counts.set(None));
    }
}

/// Samples of wall time, successful Rust allocation calls and requested bytes.
#[derive(Default)]
pub struct FrameStats {
    samples: Vec<(Duration, usize, usize)>,
}

impl FrameStats {
    /// Measure one operation, excluding sample storage and reporting allocations.
    pub fn measure<T>(&mut self, operation: impl FnOnce() -> T) -> T {
        COUNTS.with(|counts| {
            assert!(
                counts.get().is_none(),
                "allocation measurements cannot nest"
            );
            counts.set(Some((0, 0)));
        });
        let guard = Counting;
        let started = Instant::now();
        let result = operation();
        let elapsed = started.elapsed();
        let (calls, bytes) = COUNTS.with(|counts| counts.get().unwrap());
        drop(guard);
        self.samples.push((elapsed, calls, bytes));
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

//! Allocation request counters for tests and opt-in runtime diagnostics.
//! Thread counters isolate tests; `global-stats` also counts requests across all threads.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

/// System allocator that counts this thread's allocation requests.
pub struct CountingAllocator;

/// Counts requests while forwarding every operation to the wrapped allocator.
pub struct TrackingAllocator<A>(pub A);

#[cfg(feature = "global-stats")]
static GLOBAL_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(feature = "global-stats")]
static GLOBAL_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Process-wide requested allocations and bytes; concurrent updates may straddle a snapshot.
#[cfg(feature = "global-stats")]
pub fn global_allocations() -> Allocations {
    use std::sync::atomic::Ordering;
    Allocations {
        calls: GLOBAL_CALLS.load(Ordering::Relaxed) as usize,
        bytes: GLOBAL_BYTES.load(Ordering::Relaxed) as usize,
    }
}

thread_local! {
    static CALLS: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
}

/// Records a request without allocating; thread counters stop after their locals are torn down.
fn record(bytes: usize) {
    let _ = CALLS.try_with(|calls| calls.set(calls.get().wrapping_add(1)));
    #[cfg(feature = "global-stats")]
    {
        use std::sync::atomic::Ordering;
        GLOBAL_CALLS.fetch_add(1, Ordering::Relaxed);
        GLOBAL_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    }
    let _ = BYTES.try_with(|total| total.set(total.get().saturating_add(bytes as u64)));
}

// SAFETY: every call forwards the caller's layout and ownership to the wrapped allocator unchanged.
unsafe impl<A: GlobalAlloc> GlobalAlloc for TrackingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { self.0.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: as above.
        unsafe { self.0.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        // SAFETY: the caller owns `pointer` with `layout` and supplies a valid new size.
        unsafe { self.0.realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller returns a live block with its original layout.
        unsafe { self.0.dealloc(pointer, layout) }
    }
}

// SAFETY: operations delegate to the counting wrapper around System with unchanged arguments.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller supplies a valid allocation layout.
        unsafe { TrackingAllocator(System).alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller supplies a valid allocation layout.
        unsafe { TrackingAllocator(System).alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: the caller owns the allocation and supplies a valid new size.
        unsafe { TrackingAllocator(System).realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller returns a live allocation with its original layout.
        unsafe { TrackingAllocator(System).dealloc(pointer, layout) }
    }
}

/// Allocations, zeroed allocations and reallocations this thread has requested so far.
pub fn thread_allocations() -> u64 {
    CALLS.with(Cell::get)
}

/// Bytes this thread has requested so far, counting each reallocation at its new size.
pub fn thread_allocated_bytes() -> u64 {
    BYTES.with(Cell::get)
}

/// Requests made on the calling thread during one [`measure`]d operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Allocations {
    pub calls: usize,
    pub bytes: usize,
}

/// Runs `work` and returns what it allocated on this thread; nested measurements each see
/// their own work.
pub fn measure<T>(work: impl FnOnce() -> T) -> (T, Allocations) {
    let (calls, bytes) = (thread_allocations(), thread_allocated_bytes());
    let result = work();
    let allocations = Allocations {
        calls: (thread_allocations() - calls) as usize,
        bytes: (thread_allocated_bytes() - bytes) as usize,
    };
    (result, allocations)
}

/// Runs `work` and returns how many allocation requests it made on this thread.
pub fn count<T>(work: impl FnOnce() -> T) -> (T, usize) {
    let (result, allocations) = measure(work);
    (result, allocations.calls)
}

#[cfg(test)]
mod tests {
    #[global_allocator]
    static ALLOCATOR: super::CountingAllocator = super::CountingAllocator;

    /// Counts only the measured thread's requests, with reallocations at their new size.
    #[test]
    fn measure_counts_calls_and_bytes_on_this_thread() {
        const OTHER_THREAD: usize = 1 << 20;
        let (_, spawn) = super::measure(|| std::thread::spawn(|| vec![1_u8; OTHER_THREAD]).join());
        assert!(spawn.bytes < OTHER_THREAD);
        let (_, grown) = super::measure(|| {
            let mut bytes = Vec::<u8>::with_capacity(16);
            bytes.reserve_exact(48);
            bytes
        });
        assert_eq!(
            grown,
            super::Allocations {
                calls: 2,
                bytes: 16 + 48
            }
        );
        let ((_, inner), outer) = super::count(|| super::count(|| Box::new(1_u8)));
        assert_eq!((inner, outer), (1, 1));
    }
}

#[cfg(test)]
mod wrapper_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// An independent allocator that records forwarding and preserves bytes through System.
    #[derive(Default)]
    struct Probe {
        zeroed: AtomicUsize,
        resized: AtomicUsize,
        freed: AtomicUsize,
    }

    // SAFETY: layouts, pointers and ownership are passed to System unchanged.
    unsafe impl GlobalAlloc for Probe {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            // SAFETY: the caller supplies a valid layout.
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            self.zeroed.store(layout.size(), Ordering::Relaxed);
            // SAFETY: the caller supplies a valid layout.
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            self.resized.store(size, Ordering::Relaxed);
            // SAFETY: the caller owns the live block and supplies its layout and new size.
            unsafe { System.realloc(pointer, layout, size) }
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            self.freed.store(layout.size(), Ordering::Relaxed);
            // SAFETY: the caller returns a live block with its original layout.
            unsafe { System.dealloc(pointer, layout) }
        }
    }

    /// The wrapper forwards zeroing, alignment, resizing and freeing to its chosen allocator.
    #[test]
    fn wrapper_preserves_allocator_contract_and_counts_requests() {
        let allocator = TrackingAllocator(Probe::default());
        let small = Layout::from_size_align(16, 8).unwrap();
        let large = Layout::from_size_align(32, 8).unwrap();
        let (_, sample) = measure(|| {
            // SAFETY: the pointer is checked before access and freed once with its new layout.
            unsafe {
                let pointer = allocator.alloc_zeroed(small);
                assert!(!pointer.is_null());
                assert_eq!(pointer as usize % small.align(), 0);
                assert_eq!(std::slice::from_raw_parts(pointer, small.size()), &[0; 16]);
                pointer.write(42);
                let pointer = allocator.realloc(pointer, small, large.size());
                assert!(!pointer.is_null());
                assert_eq!(pointer.read(), 42);
                allocator.dealloc(pointer, large);
            }
        });
        assert_eq!(
            sample,
            Allocations {
                calls: 2,
                bytes: 48
            }
        );
        assert_eq!(allocator.0.zeroed.load(Ordering::Relaxed), small.size());
        assert_eq!(allocator.0.resized.load(Ordering::Relaxed), large.size());
        assert_eq!(allocator.0.freed.load(Ordering::Relaxed), large.size());
    }

    /// Process counters include work that thread-local measurements deliberately exclude.
    #[cfg(feature = "global-stats")]
    #[test]
    fn process_totals_include_other_threads() {
        let before = global_allocations();
        let (_, local) = measure(|| {
            std::thread::spawn(|| std::hint::black_box(vec![1_u8; 1024 * 1024]))
                .join()
                .unwrap()
        });
        let after = global_allocations();
        assert!(after.bytes - before.bytes >= 1024 * 1024);
        assert!(local.bytes < 1024 * 1024);
    }
}

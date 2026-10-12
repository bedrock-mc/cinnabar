//! Test-only allocation counting. Each test or bench binary installs [`CountingAllocator`] with
//! its own `#[global_allocator]` static; counters are per thread, so parallel tests stay isolated.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

/// System allocator that counts this thread's allocation requests.
pub struct CountingAllocator;

thread_local! {
    static CALLS: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
}

/// Records one request without allocating; ignored once the thread's locals are torn down.
fn record(bytes: usize) {
    let _ = CALLS.try_with(|calls| calls.set(calls.get() + 1));
    let _ = BYTES.try_with(|total| total.set(total.get().saturating_add(bytes as u64)));
}

// SAFETY: every call forwards the caller's layout and ownership to System unchanged.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        // SAFETY: the caller owns `pointer` with `layout` and supplies a valid new size.
        unsafe { System.realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller returns a live block with its original layout.
        unsafe { System.dealloc(pointer, layout) }
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

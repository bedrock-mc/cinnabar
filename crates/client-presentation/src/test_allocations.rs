//! Thread-local allocation counts for presentation hot-path regression tests.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

struct Allocator;

thread_local! {
    static COUNT: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
}

/// Records an allocation request of `size` bytes without allocating in the observer itself.
fn record(size: usize) {
    let _ = COUNT.try_with(|count| count.set(count.get() + 1));
    let _ = BYTES.try_with(|bytes| bytes.set(bytes.get().saturating_add(size as u64)));
}

// SAFETY: allocation ownership, layout, and alignment are forwarded unchanged to System.
unsafe impl GlobalAlloc for Allocator {
    /// Counts the request and forwards the allocator contract unchanged.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: the caller supplies the valid layout required by GlobalAlloc.
        unsafe { System.alloc(layout) }
    }

    /// Counts zeroed allocations under the same contract as System.
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: the caller supplies the valid layout required by GlobalAlloc.
        unsafe { System.alloc_zeroed(layout) }
    }

    /// Counts reallocations while preserving the original block's ownership contract.
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        // SAFETY: the caller owns pointer with this layout and provides a valid new size.
        unsafe { System.realloc(pointer, layout, size) }
    }

    /// Returns the block to the same allocator that created it.
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller supplies an allocated block with its original layout.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

/// Returns the calling thread's cumulative allocation and reallocation requests.
pub(crate) fn count() -> u64 {
    COUNT.with(Cell::get)
}

/// Returns the bytes the calling thread has requested, counting each reallocation's new size.
pub(crate) fn bytes() -> u64 {
    BYTES.with(Cell::get)
}

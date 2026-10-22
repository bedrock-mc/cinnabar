//! Heap admission for compiler hosts, including platforms with large shared-library mappings.

use std::{
    alloc::{GlobalAlloc, Layout},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// Live-byte accounting shared by the compiler host's global allocator.
struct Accounting {
    active: AtomicBool,
    installed: AtomicBool,
    bytes: AtomicUsize,
}

impl Accounting {
    /// Starts accounting before a worker has enabled its admission limit.
    const fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            installed: AtomicBool::new(false),
            bytes: AtomicUsize::new(0),
        }
    }

    /// Reserves bytes atomically, refusing growth beyond the enabled heap budget.
    fn reserve(&self, bytes: usize) -> bool {
        self.installed.store(true, Ordering::Relaxed);
        self.bytes
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                let next = current.checked_add(bytes)?;
                (!self.active.load(Ordering::Relaxed) || next <= super::MEMORY_LIMIT as usize)
                    .then_some(next)
            })
            .is_ok()
    }

    /// Returns the credit of a freed block or failed allocation.
    fn release(&self, bytes: usize) {
        self.bytes.fetch_sub(bytes, Ordering::Relaxed);
    }
}

static ACCOUNTING: Accounting = Accounting::new();

/// Wraps a compiler executable's allocator; its budget is enabled only in worker mode.
/// The host must use this as its sole global allocator, including before worker startup.
pub struct BudgetAllocator<A>(pub A);

/// Enables the heap budget only in an executable that installed the accounting allocator.
pub(super) fn enable() -> bool {
    ACCOUNTING.active.store(true, Ordering::Relaxed);
    // Some runtimes enter main without allocating. Probe the global allocator explicitly.
    let layout = Layout::new::<u8>();
    // SAFETY: the probe uses a valid nonzero layout and is released with the same layout.
    unsafe {
        let pointer = std::alloc::alloc(layout);
        if pointer.is_null() {
            return false;
        }
        std::alloc::dealloc(pointer, layout);
    }
    ACCOUNTING.installed.load(Ordering::Relaxed)
}

// SAFETY: every operation preserves the wrapped allocator's layouts and ownership. Refusing
// growth returns null, as GlobalAlloc permits; failed requests return their reservation.
unsafe impl<A: GlobalAlloc> GlobalAlloc for BudgetAllocator<A> {
    /// Reserves the requested bytes before forwarding the allocation.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !ACCOUNTING.reserve(layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller supplies a valid allocation layout.
        let pointer = unsafe { self.0.alloc(layout) };
        if pointer.is_null() {
            ACCOUNTING.release(layout.size());
        }
        pointer
    }

    /// Reserves zeroed storage under the same live-byte budget.
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !ACCOUNTING.reserve(layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller supplies a valid allocation layout.
        let pointer = unsafe { self.0.alloc_zeroed(layout) };
        if pointer.is_null() {
            ACCOUNTING.release(layout.size());
        }
        pointer
    }

    /// Charges only growth, and keeps the old reservation when reallocation fails.
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let growth = size.saturating_sub(layout.size());
        if !ACCOUNTING.reserve(growth) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller owns this block with the supplied layout and valid new size.
        let result = unsafe { self.0.realloc(pointer, layout, size) };
        if result.is_null() {
            ACCOUNTING.release(growth);
        } else if size < layout.size() {
            ACCOUNTING.release(layout.size() - size);
        }
        result
    }

    /// Releases live-byte credit after returning the block to its allocator.
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller returns this live block with its original allocation layout.
        unsafe {
            self.0.dealloc(pointer, layout);
        }
        ACCOUNTING.release(layout.size());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_heap_credit_cannot_exceed_the_enabled_budget() {
        let accounting = Accounting::new();
        accounting.active.store(true, Ordering::Relaxed);
        assert!(accounting.reserve(super::super::MEMORY_LIMIT as usize));
        assert!(!accounting.reserve(1));
        accounting.release(17);
        assert!(accounting.reserve(17));
        assert!(!accounting.reserve(1));
    }

    /// Allocates real blocks but refuses every reallocation without releasing the old block.
    struct RefuseGrowth;

    // SAFETY: successful operations forward to System unchanged; failed realloc keeps ownership.
    unsafe impl GlobalAlloc for RefuseGrowth {
        /// Forwards a valid allocation layout to System.
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            // SAFETY: the caller supplies the required layout.
            unsafe { std::alloc::System.alloc(layout) }
        }
        /// Leaves the original block live when growth is refused.
        unsafe fn realloc(&self, _pointer: *mut u8, _layout: Layout, _size: usize) -> *mut u8 {
            std::ptr::null_mut()
        }
        /// Returns a System block with its original layout.
        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            // SAFETY: the caller returns the block this allocator created.
            unsafe {
                std::alloc::System.dealloc(pointer, layout);
            }
        }
    }

    #[test]
    fn failed_growth_keeps_only_the_original_allocation_charged() {
        let allocator = BudgetAllocator(RefuseGrowth);
        let baseline = ACCOUNTING.bytes.load(Ordering::Relaxed);
        let layout = Layout::from_size_align(8, 8).unwrap();
        // SAFETY: valid layout, one live allocation, and exactly one matching deallocation.
        unsafe {
            let pointer = allocator.alloc(layout);
            assert!(!pointer.is_null());
            assert!(allocator.realloc(pointer, layout, 16).is_null());
            assert_eq!(
                ACCOUNTING.bytes.load(Ordering::Relaxed),
                baseline + layout.size()
            );
            allocator.dealloc(pointer, layout);
        }
        assert_eq!(ACCOUNTING.bytes.load(Ordering::Relaxed), baseline);
    }
}

//! Heap admission for compiler hosts, including platforms with large shared-library mappings.

use std::{
    alloc::{GlobalAlloc, Layout},
    sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
};

const STARTUP: u8 = 0;
const WORKER: u8 = 1;
const HOST: u8 = 2;

/// Live-byte accounting shared by the compiler host's global allocator.
struct Accounting {
    mode: AtomicU8,
    installed: AtomicBool,
    bytes: AtomicUsize,
}

impl Accounting {
    /// Starts accounting before a worker has enabled its admission limit.
    const fn new() -> Self {
        Self {
            mode: AtomicU8::new(STARTUP),
            installed: AtomicBool::new(false),
            bytes: AtomicUsize::new(0),
        }
    }

    /// Returns whether a reservation was tracked, or refuses growth beyond the worker budget.
    fn reserve(&self, bytes: usize) -> Option<bool> {
        let mode = self.mode.load(Ordering::Relaxed);
        match mode {
            HOST => return Some(false),
            STARTUP | WORKER => {}
            _ => return None,
        }
        self.installed.store(true, Ordering::Relaxed);
        self.bytes
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                let next = current.checked_add(bytes)?;
                (mode != WORKER || next <= super::MEMORY_LIMIT as usize).then_some(next)
            })
            .ok()
            .map(|_| true)
    }

    /// Selects an ordinary host permanently; it can never later run an unbounded compiler.
    fn host(&self) {
        let _ = self
            .mode
            .compare_exchange(STARTUP, HOST, Ordering::AcqRel, Ordering::Acquire);
    }

    /// Enters bounded worker mode only before the host chose ordinary asset preparation.
    fn worker(&self) -> bool {
        self.mode
            .compare_exchange(STARTUP, WORKER, Ordering::AcqRel, Ordering::Acquire)
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
    if !ACCOUNTING.worker() {
        return false;
    }
    // Some runtimes enter main without allocating. Probe the global allocator explicitly.
    let layout = Layout::new::<u8>();
    // SAFETY: the probe uses a valid nonzero layout and is released with the same layout.
    unsafe {
        // Keep the pointer observable so optimization cannot remove the allocator probe.
        let pointer = std::hint::black_box(std::alloc::alloc(layout));
        if pointer.is_null() {
            return false;
        }
        std::alloc::dealloc(pointer, layout);
    }
    ACCOUNTING.installed.load(Ordering::Relaxed)
}

/// Disables accounting during ordinary asset preparation, before the host starts its normal CLI.
/// Startup allocations remain harmless credits; this process can no longer enter worker mode.
pub fn disable_for_host() {
    ACCOUNTING.host();
}

// SAFETY: every operation preserves the wrapped allocator's layouts and ownership. Refusing
// growth returns null, as GlobalAlloc permits; failed requests return their reservation.
unsafe impl<A: GlobalAlloc> GlobalAlloc for BudgetAllocator<A> {
    /// Reserves the requested bytes before forwarding the allocation.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let Some(tracked) = ACCOUNTING.reserve(layout.size()) else {
            return std::ptr::null_mut();
        };
        // SAFETY: the caller supplies a valid allocation layout.
        let pointer = unsafe { self.0.alloc(layout) };
        if pointer.is_null() && tracked {
            ACCOUNTING.release(layout.size());
        }
        pointer
    }

    /// Reserves zeroed storage under the same live-byte budget.
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let Some(tracked) = ACCOUNTING.reserve(layout.size()) else {
            return std::ptr::null_mut();
        };
        // SAFETY: the caller supplies a valid allocation layout.
        let pointer = unsafe { self.0.alloc_zeroed(layout) };
        if pointer.is_null() && tracked {
            ACCOUNTING.release(layout.size());
        }
        pointer
    }

    /// Charges only growth, and keeps the old reservation when reallocation fails.
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let growth = size.saturating_sub(layout.size());
        let Some(tracked) = ACCOUNTING.reserve(growth) else {
            return std::ptr::null_mut();
        };
        // SAFETY: the caller owns this block with the supplied layout and valid new size.
        let result = unsafe { self.0.realloc(pointer, layout, size) };
        if tracked {
            if result.is_null() {
                ACCOUNTING.release(growth);
            } else if size < layout.size() {
                ACCOUNTING.release(layout.size() - size);
            }
        }
        result
    }

    /// Releases live-byte credit after returning the block to its allocator.
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller returns this live block with its original allocation layout.
        unsafe {
            self.0.dealloc(pointer, layout);
        }
        if ACCOUNTING.mode.load(Ordering::Relaxed) != HOST {
            ACCOUNTING.release(layout.size());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_heap_credit_cannot_exceed_the_enabled_budget() {
        let accounting = Accounting::new();
        assert!(accounting.worker());
        assert_eq!(
            accounting.reserve(super::super::MEMORY_LIMIT as usize),
            Some(true)
        );
        assert_eq!(accounting.reserve(1), None);
        accounting.release(17);
        assert_eq!(accounting.reserve(17), Some(true));
        assert_eq!(accounting.reserve(1), None);
    }

    #[test]
    fn ordinary_asset_preparation_bypasses_tracking_and_cannot_enable_a_worker() {
        let accounting = Accounting::new();
        accounting.host();
        assert_eq!(
            accounting.reserve(super::super::MEMORY_LIMIT as usize * 2),
            Some(false)
        );
        assert_eq!(accounting.bytes.load(Ordering::Relaxed), 0);
        assert!(!accounting.worker());
        let worker = Accounting::new();
        assert!(worker.worker());
        worker.host();
        assert_eq!(
            worker.reserve(super::super::MEMORY_LIMIT as usize + 1),
            None
        );
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

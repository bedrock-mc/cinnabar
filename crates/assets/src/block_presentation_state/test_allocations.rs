use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! { static COUNT: Cell<Option<usize>> = const { Cell::new(None) }; }

struct CountingAllocator;

/// Counts only allocations on the test thread inside an explicit measured scope.
fn count() {
    COUNT.with(|value| {
        if let Some(n) = value.get() {
            value.set(Some(n + 1));
        }
    });
}

// SAFETY: every operation forwards the caller's layout and ownership to System.
unsafe impl GlobalAlloc for CountingAllocator {
    /// Counts allocation requests while preserving the system allocator contract.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }
    /// Counts zeroed allocations without changing their ownership or layout.
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc_zeroed(layout) }
    }
    /// Counts resizing while leaving memory ownership with the system allocator.
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(pointer, layout, size) }
    }
    /// Returns memory to the allocator that created it.
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

struct Scope;
impl Drop for Scope {
    /// Stops measuring even when the observed operation unwinds.
    fn drop(&mut self) {
        COUNT.with(|value| value.set(None));
    }
}

/// Returns the allocation count without counting fixture construction or assertions.
pub fn measure<T>(run: impl FnOnce() -> T) -> (T, usize) {
    COUNT.with(|value| value.set(Some(0)));
    let scope = Scope;
    let result = run();
    let count = COUNT.with(|value| value.get().unwrap());
    drop(scope);
    (result, count)
}

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static COUNTS: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

pub struct CountedAllocator;

fn count(bytes: usize) {
    let _ = COUNTS.try_with(|counts| {
        if let Some((calls, total)) = counts.get() {
            counts.set(Some((calls + 1, total + bytes)));
        }
    });
}

// SAFETY: Pointers and layouts are forwarded unchanged to the system allocator.
unsafe impl GlobalAlloc for CountedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            count(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            count(layout.size());
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, size) };
        if !pointer.is_null() {
            count(size);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }
}

struct Counting;

impl Drop for Counting {
    fn drop(&mut self) {
        COUNTS.with(|counts| counts.set(None));
    }
}

/// Counts successful allocation requests, including reallocations at their new size.
pub fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize, usize) {
    COUNTS.with(|counts| {
        assert!(counts.get().is_none());
        counts.set(Some((0, 0)));
    });
    let guard = Counting;
    let result = operation();
    let (calls, bytes) = COUNTS.with(|counts| counts.get().unwrap());
    drop(guard);
    (result, calls, bytes)
}

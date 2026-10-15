use std::alloc::{GlobalAlloc, Layout, System};
use test_alloc::{Allocations, TrackingAllocator};

const INITIAL_BYTES: usize = 16;
const GROWN_BYTES: usize = 32;
const ALIGNMENT: usize = 8;
const VALUE: u8 = 42;

/// Exercises zeroing, growth, content preservation and release through one chosen backend.
fn exercise(allocator: impl GlobalAlloc) -> Allocations {
    let initial = Layout::from_size_align(INITIAL_BYTES, ALIGNMENT).unwrap();
    let grown = Layout::from_size_align(GROWN_BYTES, ALIGNMENT).unwrap();
    let (_, allocations) = test_alloc::measure(|| {
        // SAFETY: each non-null block is accessed within its size and freed with its new layout.
        unsafe {
            let pointer = allocator.alloc_zeroed(initial);
            assert!(!pointer.is_null());
            assert_eq!(pointer as usize % ALIGNMENT, 0);
            assert_eq!(
                std::slice::from_raw_parts(pointer, INITIAL_BYTES),
                &[0; INITIAL_BYTES]
            );
            pointer.write(VALUE);
            let pointer = allocator.realloc(pointer, initial, GROWN_BYTES);
            assert!(!pointer.is_null());
            assert_eq!(pointer.read(), VALUE);
            allocator.dealloc(pointer, grown);
        }
    });
    allocations
}

/// Both client allocator choices preserve memory contents and report requested sizes.
#[test]
fn system_and_mimalloc_count_the_same_requests() {
    let expected = Allocations {
        calls: 2,
        bytes: INITIAL_BYTES + GROWN_BYTES,
    };
    assert_eq!(exercise(TrackingAllocator(System)), expected);
    assert_eq!(exercise(TrackingAllocator(mimalloc::MiMalloc)), expected);
}

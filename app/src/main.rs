#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod entry;

#[cfg(all(feature = "mimalloc", not(feature = "alloc-stats")))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(all(feature = "alloc-stats", feature = "mimalloc"))]
#[global_allocator]
static ALLOCATOR: test_alloc::TrackingAllocator<mimalloc::MiMalloc> =
    test_alloc::TrackingAllocator(mimalloc::MiMalloc);

#[cfg(all(feature = "alloc-stats", not(feature = "mimalloc")))]
#[global_allocator]
static ALLOCATOR: test_alloc::TrackingAllocator<std::alloc::System> =
    test_alloc::TrackingAllocator(std::alloc::System);

/// Runs the client through its shared startup and exit dispatch.
fn main() {
    cinnabar::lifecycle::record_launch();
    entry::dispatch();
}

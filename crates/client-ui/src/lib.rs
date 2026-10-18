//! Retained client UI and presentation, driven synchronously by app adapters.

#[cfg(any(target_arch = "wasm32", test))]
pub mod browser_jobs;
pub mod ui_runtime;

pub mod block_cracks;
pub mod diagnostic_markers;
#[cfg(not(target_arch = "wasm32"))]
pub mod experience_session;
pub mod sound_requests;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

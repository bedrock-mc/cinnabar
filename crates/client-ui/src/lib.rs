//! Retained client UI and presentation, driven synchronously by app adapters.

/// Asset-independent OreUI colours, typography and geometry for bootstrap screens.
pub mod oreui_theme;
pub mod ui_runtime;
pub use launcher::{global_resources, install_layout, local_worlds, menu};

pub mod block_cracks;
pub mod diagnostic_markers;
pub mod experience_session;
pub mod sound_requests;
pub mod store;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

#[cfg(test)]
mod allocation_count;

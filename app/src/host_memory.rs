//! Physical-memory observations supplied by the platform host.
use bevy::prelude::Resource;

/// Available device memory; zero selects the conservative tier when it is unknown.
#[derive(Resource)]
pub(crate) struct HostMemory(pub(crate) u64);

impl Default for HostMemory {
    /// Queries native memory without assuming that browsers expose a hardware measurement.
    fn default() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        return Self(launcher_host::memory::physical_bytes());
        #[cfg(target_arch = "wasm32")]
        Self(0)
    }
}

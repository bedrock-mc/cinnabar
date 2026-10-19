//! Optional request-to-terrain join timings. Shipping builds leave `join-profile` disabled.
//! Handles belong to one attempt; finishing an old handle never closes a newer attempt.

/// Whether this build includes join timing instrumentation.
pub const ENABLED: bool = cfg!(feature = "join-profile");

/// First startup and terrain results, without logging every packet or chunk.
#[derive(Clone, Copy)]
pub enum Milestone {
    ChunkDecoded,
    LightingReady,
    MeshReady,
    MeshUploaded,
    ChunkRadiusReply,
}

impl Milestone {
    /// Returns the stable stage name used in diagnostic logs.
    pub const fn name(self) -> &'static str {
        match self {
            Self::ChunkDecoded => "first_chunk_decoded",
            Self::LightingReady => "first_lighting_ready",
            Self::MeshReady => "first_mesh_ready",
            Self::MeshUploaded => "first_mesh_uploaded",
            Self::ChunkRadiusReply => "chunk_radius_reply",
        }
    }
}

#[cfg(feature = "join-profile")]
mod enabled;
#[cfg(feature = "join-profile")]
pub use enabled::{Binding, JoinProfile, Stage};

#[cfg(not(feature = "join-profile"))]
mod disabled {
    /// Inert join handle when diagnostic instrumentation is not compiled.
    #[derive(Clone, Copy, Default)]
    pub struct JoinProfile;
    /// Inert timed section when diagnostic instrumentation is not compiled.
    pub struct Stage;
    /// Inert worker binding in shipping builds.
    pub struct Binding;
    impl JoinProfile {
        /// Starts a diagnostic attempt, or does nothing in a shipping build.
        pub fn begin() -> Self {
            Self
        }
        /// Returns the current diagnostic attempt, or an inert handle.
        pub fn current() -> Self {
            Self
        }
        /// Runs a future without diagnostic context in shipping builds.
        pub async fn in_scope<F: std::future::Future>(&self, future: F) -> F::Output {
            future.await
        }
        /// Runs a worker without diagnostic context in shipping builds.
        pub fn run<T>(&self, work: impl FnOnce() -> T) -> T {
            work()
        }
        /// Returns an inert worker binding in shipping builds.
        pub fn enter(&self) -> Binding {
            Binding
        }
        /// Returns the optional diagnostic identifier sent only to the local core.
        pub fn id(&self) -> Option<u64> {
            None
        }
        /// Ignores presentation readiness in shipping builds.
        pub fn ready_for_presentation(&self) {}
        /// Shipping builds have no pending timing observation.
        pub fn presentation_pending(&self) -> bool {
            false
        }
        /// Ignores startup and terrain observations in shipping builds.
        pub fn mark_once(&self, _milestone: super::Milestone) {}
        /// Records an instantaneous milestone when instrumentation is compiled.
        pub fn mark(&self, _stage: &'static str) {}
        /// Times one section when instrumentation is compiled.
        pub fn stage(&self, _stage: &'static str) -> Stage {
            Stage
        }
        /// Completes this attempt without affecting another one.
        pub fn finish(&self, _stage: &'static str) {}
    }
}
#[cfg(not(feature = "join-profile"))]
pub use disabled::{Binding, JoinProfile, Stage};

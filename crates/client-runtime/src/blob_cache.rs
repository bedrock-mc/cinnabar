use bevy::prelude::Resource;

/// The verified blob cache shared by every network session in this process.
#[derive(Debug, Clone, Default, Resource)]
pub struct ClientBlobCacheOwner(protocol::ClientBlobCache);

impl ClientBlobCacheOwner {
    /// Returns a shared handle to the process-lifetime verified blob cache.
    pub fn cache(&self) -> protocol::ClientBlobCache {
        self.0.clone()
    }

    /// Whether cores spawned for sessions backed by this owner may advertise
    /// upstream client-cache capability (`-upstream-client-cache`). True
    /// because every session receives [`Self::cache`] and answers
    /// LoginSuccess with cache-enabled status downstream, and the two
    /// advertisements must stay coupled; a future cache-less flow must report
    /// `false` here so the core keeps its default-disabled wire bytes.
    pub fn enables_upstream_client_cache(&self) -> bool {
        true
    }
}

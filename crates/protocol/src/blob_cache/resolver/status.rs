use super::*;

/// A complete, immutable partition of one cached packet's unique blob references.
///
/// The classified sets cannot be shortened after classification:
///
/// ```compile_fail
/// fn omit_reference(status: &mut protocol::BlobCacheStatus) {
///     status.missing.clear();
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobCacheStatus {
    missing: Vec<u64>,
    have: Vec<u64>,
    pub recovery: Option<ChunkResyncEvent>,
    classified_hashes: usize,
    staged_bytes: usize,
    _classified: ClassifiedStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClassifiedStatus;

impl BlobCacheStatus {
    pub(super) fn classify<T: ReferencedBlobHashes>(
        cache: &ClientBlobCache,
        packet: &T,
        recovery: Option<ChunkResyncEvent>,
        pin: bool,
    ) -> Self {
        let referenced_hashes = stable_unique(&packet.referenced_blob_hashes());
        let (have, missing, staged_bytes) = cache.classify(&referenced_hashes, pin);
        let classified_hashes = have.len() + missing.len();
        debug_assert_eq!(classified_hashes, referenced_hashes.len());
        Self {
            missing,
            have,
            recovery,
            classified_hashes,
            staged_bytes,
            _classified: ClassifiedStatus,
        }
    }

    /// Unique referenced hashes absent from the cache when classification completed.
    #[must_use]
    pub fn missing(&self) -> &[u64] {
        &self.missing
    }

    /// Unique referenced hashes present in the cache when classification completed.
    #[must_use]
    pub fn have(&self) -> &[u64] {
        &self.have
    }

    /// Number of unique referenced hashes partitioned into `missing` and `have`.
    #[must_use]
    pub const fn classified_hashes(&self) -> usize {
        self.classified_hashes
    }

    pub(super) const fn staged_bytes(&self) -> usize {
        self.staged_bytes
    }

    pub(super) fn set_recovery(&mut self, recovery: Option<ChunkResyncEvent>) {
        self.recovery = recovery;
    }

    #[must_use]
    pub fn take_recovery(&mut self) -> Option<ChunkResyncEvent> {
        self.recovery.take()
    }

    /// Splits the two protocol sets without omitting or reclassifying any referenced hash.
    #[must_use]
    pub fn into_packets(self) -> Vec<ClientCacheBlobStatusPacket> {
        let mut missing = self.missing.into_iter().peekable();
        let mut have = self.have.into_iter().peekable();
        let mut packets = Vec::new();
        while missing.peek().is_some() || have.peek().is_some() {
            let mut packet = ClientCacheBlobStatusPacket::default();
            while packet.missing.len() + packet.have.len() < MAX_CLIENT_BLOB_HASHES_PER_PACKET {
                if let Some(hash) = missing.next() {
                    packet.missing.push(hash);
                } else if let Some(hash) = have.next() {
                    packet.have.push(hash);
                } else {
                    break;
                }
            }
            packets.push(packet);
        }
        packets
    }
}

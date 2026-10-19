//! Content digests for admitted archives and their decoded entries.

use sha2::{Digest, Sha256};
use std::sync::OnceLock;

/// Pack identity and archive digest in lowest-to-highest precedence order.
pub type StackFingerprint = Vec<(String, String, String, [u8; 32])>;

/// Hashes the supplied bytes consistently for both archives and decoded files.
pub(crate) fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// A digest belongs to the immutable archive bytes, including across pack clones.
#[derive(Default)]
pub(crate) struct ContentHash {
    digest: OnceLock<[u8; 32]>,
    #[cfg(test)]
    computations: std::sync::atomic::AtomicUsize,
}

impl ContentHash {
    /// Hashes an archive once; concurrent readers share the completed digest.
    pub(crate) fn get(&self, bytes: &[u8]) -> [u8; 32] {
        *self.digest.get_or_init(|| {
            #[cfg(test)]
            self.computations
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            hash_bytes(bytes)
        })
    }

    /// Reports actual archive digest computations for admission work-count tests.
    #[cfg(test)]
    pub(crate) fn computations(&self) -> usize {
        self.computations.load(std::sync::atomic::Ordering::Relaxed)
    }
}

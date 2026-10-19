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

impl crate::ValidatedPack {
    /// Identifies archive bytes, admitted paths and decryption bindings without reading files.
    pub fn compile_identity(&self) -> [u8; 32] {
        /// Frames variable-length paths so adjacent index entries cannot alias.
        fn part(hash: &mut Sha256, bytes: &[u8]) {
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
        let mut hash = Sha256::new();
        hash.update(self.content_hash.get(&self.archive_bytes()));
        hash.update((self.archive_bytes as u64).to_le_bytes());
        hash.update(self.declared_bytes.to_le_bytes());
        hash.update((self.physical_entry_count as u64).to_le_bytes());
        hash.update((self.skipped_entries as u64).to_le_bytes());
        hash.update((self.files.len() as u64).to_le_bytes());
        let mut files: Vec<_> = self.files.iter().collect();
        files.sort_unstable_by_key(|(path, _)| *path);
        for (path, entry) in files {
            part(&mut hash, path.as_bytes());
            hash.update((entry.archive_index as u64).to_le_bytes());
            hash.update(entry.uncompressed_size.to_le_bytes());
            hash.update([u8::from(entry.key.is_some())]);
            if let Some(key) = entry.key {
                hash.update(self.keys[key].digest());
            }
        }
        hash.update((self.file_order.len() as u64).to_le_bytes());
        for path in &self.file_order {
            part(&mut hash, path.as_bytes());
        }
        let folded: std::collections::BTreeMap<_, _> = self.folded.iter().collect();
        hash.update((folded.len() as u64).to_le_bytes());
        for (name, path) in folded {
            part(&mut hash, name.as_bytes());
            part(&mut hash, path.as_bytes());
        }
        hash.finalize().into()
    }
}

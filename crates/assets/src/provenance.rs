//! Shared carrier provenance identity helpers.
//!
//! Every compiled carrier embeds the exact source identities it was built
//! from; startup rejects carriers whose embedded identities disagree with the
//! checkout-pinned expectations. The canonical manifest digest here is the one
//! implementation shared by the compiler binary and startup validation so the
//! identities match regardless of checkout `autocrlf` line endings.

use sha2::{Digest, Sha256};

/// SHA-256 of a tracked source manifest with CRLF line endings canonicalized
/// to LF, matching the compiler-side carrier identity regardless of checkout
/// `autocrlf`. A lone CR or bare LF disables canonicalization and hashes the
/// bytes verbatim; compilers refuse such manifests outright, so no valid
/// carrier can carry that identity and any mismatch fails closed.
#[must_use]
pub fn canonical_source_manifest_sha256(source: &[u8]) -> [u8; 32] {
    if !source.contains(&b'\r') {
        return Sha256::digest(source).into();
    }
    let mut canonical = Vec::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        match source[index] {
            b'\r' if source.get(index + 1) == Some(&b'\n') => {
                canonical.push(b'\n');
                index += 2;
            }
            b'\r' | b'\n' => return Sha256::digest(source).into(),
            byte => {
                canonical.push(byte);
                index += 1;
            }
        }
    }
    Sha256::digest(canonical).into()
}

/// The exact source identities bound into one compiled world blob header:
/// the canonical vanilla source manifest plus each registry input consumed by
/// the compiler. Decode rejects incomplete identity, so a blob can never
/// silently claim to be unbound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlobProvenance {
    pub source_manifest_sha256: [u8; 32],
    pub block_registry_sha256: [u8; 32],
    pub light_registry_sha256: [u8; 32],
    pub biome_registry_sha256: [u8; 32],
}

impl BlobProvenance {
    /// The unbound identity carried only before real inputs are bound (the
    /// diagnostic runtime) or by library-level compilation before the
    /// compiler command overwrites it with the exact input hashes. Decode
    /// rejects this identity, so it can never reach gameplay.
    pub const ZEROED: Self = Self {
        source_manifest_sha256: [0; 32],
        block_registry_sha256: [0; 32],
        light_registry_sha256: [0; 32],
        biome_registry_sha256: [0; 32],
    };

    /// True when every identity slot carries a non-zero digest.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        !is_zero(&self.source_manifest_sha256)
            && !is_zero(&self.block_registry_sha256)
            && !is_zero(&self.light_registry_sha256)
            && !is_zero(&self.biome_registry_sha256)
    }
}

const fn is_zero(bytes: &[u8; 32]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{BlobProvenance, canonical_source_manifest_sha256};

    #[test]
    fn canonical_manifest_hash_is_line_ending_invariant() {
        assert_eq!(
            canonical_source_manifest_sha256(b"{\r\n  \"schema\": 1\r\n}\r\n"),
            canonical_source_manifest_sha256(b"{\n  \"schema\": 1\n}\n")
        );
    }

    #[test]
    fn mixed_source_manifest_line_endings_do_not_match_the_canonical_pin() {
        assert_ne!(
            canonical_source_manifest_sha256(b"{\r\n  \"schema\": 1\n}\r\n"),
            canonical_source_manifest_sha256(b"{\n  \"schema\": 1\n}\n")
        );
        assert_eq!(
            canonical_source_manifest_sha256(b"{\r  \"schema\": 1\r}"),
            canonical_source_manifest_sha256(b"{\r  \"schema\": 1\r}")
        );
    }

    #[test]
    fn zeroed_provenance_is_incomplete_and_real_digests_are_complete() {
        assert!(!BlobProvenance::ZEROED.is_complete());
        let complete = BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        };
        assert!(complete.is_complete());
        let mut one_zero = complete;
        one_zero.light_registry_sha256 = [0; 32];
        assert!(!one_zero.is_complete());
    }
}

//! Shared carrier provenance identity helpers.
//!
//! Every compiled carrier embeds the exact source identities it was built
//! from; startup rejects carriers whose embedded identities disagree with the
//! checkout-pinned expectations. The canonical manifest digest here is the one
//! implementation shared by the compiler binary and startup validation so the
//! identities match regardless of checkout `autocrlf` line endings.

use std::sync::OnceLock;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The tracked `assets/vanilla-source.json`: the one definition of the pinned
/// Mojang bedrock-samples pack that every compiler and runtime pin reads.
pub const VANILLA_SOURCE_MANIFEST: &str = include_str!("../../../assets/vanilla-source.json");

/// Identity of the pinned Mojang bedrock-samples pack.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VanillaSource {
    pub schema: u32,
    pub tag: Box<str>,
    pub commit: Box<str>,
    pub archive: Box<str>,
    pub url: Box<str>,
    pub sha256: Box<str>,
    pub artifact_policy: Box<str>,
    /// Workspace-relative extraction directory, always below `.local/`.
    pub cache_dir: Box<str>,
}

impl VanillaSource {
    /// Workspace-relative extracted `resource_pack` directory.
    #[must_use]
    pub fn resource_pack_dir(&self) -> String {
        format!("{}/resource_pack", self.cache_dir)
    }

    /// The pack path below an installed layout's resource root, which mirrors `.local/`.
    #[must_use]
    pub fn installed_pack_dir(&self, pack: &str) -> String {
        let cache = self
            .cache_dir
            .strip_prefix(".local/")
            .unwrap_or(&self.cache_dir);
        format!("{cache}/{pack}")
    }
}

/// The pinned pack parsed from [`VANILLA_SOURCE_MANIFEST`].
#[must_use]
pub fn vanilla_source() -> &'static VanillaSource {
    static SOURCE: OnceLock<VanillaSource> = OnceLock::new();
    SOURCE.get_or_init(|| {
        serde_json::from_str(VANILLA_SOURCE_MANIFEST).expect("tracked vanilla source manifest")
    })
}

/// Canonical digest of [`VANILLA_SOURCE_MANIFEST`]; carriers built from the pin embed it.
#[must_use]
pub fn vanilla_source_manifest_sha256() -> [u8; 32] {
    canonical_source_manifest_sha256(VANILLA_SOURCE_MANIFEST.as_bytes())
}

/// SHA-256 of a tracked source manifest with CRLF line endings canonicalized
/// to LF, matching the compiler-side carrier identity regardless of checkout
/// `autocrlf`. A lone CR or bare LF disables canonicalization and hashes the
/// bytes verbatim; such a checkout hashes differently from its canonical LF
/// form, so a carrier built from it fails startup validation against the
/// canonical pin and the mismatch fails closed.
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
    use super::{
        BlobProvenance, canonical_source_manifest_sha256, vanilla_source,
        vanilla_source_manifest_sha256,
    };

    #[test]
    fn tracked_vanilla_source_is_a_local_only_mojang_release_pin() {
        let source = vanilla_source();
        assert_eq!(source.schema, 1);
        assert_eq!(source.artifact_policy.as_ref(), "local-only");
        assert_eq!(
            source.url.as_ref(),
            format!(
                "https://github.com/Mojang/bedrock-samples/releases/download/{}/{}",
                source.tag, source.archive
            )
        );
        assert!(
            source
                .cache_dir
                .starts_with(".local/assets/bedrock-samples/")
        );
        assert_ne!(vanilla_source_manifest_sha256(), [0; 32]);
    }

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

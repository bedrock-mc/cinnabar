//! Checkout-pinned world-carrier identity expectations.
//!
//! Startup validates every compiled world blob against these expectations so
//! structurally valid carriers built from stale or foreign sources fail
//! closed instead of reaching gameplay. The registry bytes are embedded at
//! compile time from the checked-in protocol-1001 inputs, exactly like the
//! collision consumer, so validation never depends on the process working
//! directory or installed layout.

use std::path::Path;
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

use assets::{
    BlobProvenance, RuntimeAssets, RuntimeAtmosphereAssets, canonical_source_manifest_sha256,
};

use super::{ATMOSPHERE_COMPILE_COMMAND, AssetStartupError, COMPILE_COMMAND, format_sha256};

const VANILLA_SOURCE_JSON: &str = include_str!("../../../assets/vanilla-source.json");
const BLOCK_REGISTRY_BYTES: &[u8] =
    include_bytes!("../../../crates/assets/data/block-registry-v1001.bin");
const LIGHT_REGISTRY_BYTES: &[u8] =
    include_bytes!("../../../crates/assets/data/block-light-registry-v1001.bin");
const BIOME_REGISTRY_BYTES: &[u8] =
    include_bytes!("../../../crates/assets/data/biome-registry-v1001.bin");

/// The checked-in protocol-1001 block registry, shared with the collision
/// consumer so one embed feeds both physics and provenance validation.
pub(crate) const fn pinned_block_registry_bytes() -> &'static [u8] {
    BLOCK_REGISTRY_BYTES
}

/// The exact world-carrier identity this checkout pins: the canonical
/// vanilla source manifest plus each consumed protocol-1001 registry input.
#[must_use]
pub fn pinned_world_provenance() -> &'static BlobProvenance {
    static PINNED: OnceLock<BlobProvenance> = OnceLock::new();
    PINNED.get_or_init(|| BlobProvenance {
        source_manifest_sha256: canonical_source_manifest_sha256(VANILLA_SOURCE_JSON.as_bytes()),
        block_registry_sha256: Sha256::digest(BLOCK_REGISTRY_BYTES).into(),
        light_registry_sha256: Sha256::digest(LIGHT_REGISTRY_BYTES).into(),
        biome_registry_sha256: Sha256::digest(BIOME_REGISTRY_BYTES).into(),
    })
}

/// Fails closed unless the decoded world carrier was compiled from exactly
/// the checkout-pinned manifest and registry inputs.
pub(crate) fn verify_world_carrier(
    path: &Path,
    runtime: &RuntimeAssets,
) -> Result<(), AssetStartupError> {
    let expected = pinned_world_provenance();
    let actual = runtime.provenance();
    for (component, expected, actual) in [
        (
            "source manifest",
            expected.source_manifest_sha256,
            actual.source_manifest_sha256,
        ),
        (
            "block registry",
            expected.block_registry_sha256,
            actual.block_registry_sha256,
        ),
        (
            "light registry",
            expected.light_registry_sha256,
            actual.light_registry_sha256,
        ),
        (
            "biome registry",
            expected.biome_registry_sha256,
            actual.biome_registry_sha256,
        ),
    ] {
        if expected != actual {
            return Err(AssetStartupError::WorldAssetsProvenance {
                path: path.to_path_buf(),
                component,
                expected: format_sha256(expected),
                actual: format_sha256(actual),
                rebuild_command: COMPILE_COMMAND,
            });
        }
    }
    Ok(())
}

/// Fails closed unless the decoded atmosphere carrier was compiled from the
/// checkout-pinned vanilla source manifest.
pub(crate) fn verify_atmosphere_carrier(
    path: &Path,
    runtime: &RuntimeAtmosphereAssets,
) -> Result<(), AssetStartupError> {
    let expected = canonical_source_manifest_sha256(VANILLA_SOURCE_JSON.as_bytes());
    let actual = runtime.source_manifest_sha256();
    if actual != expected {
        return Err(AssetStartupError::AtmosphereAssetsProvenance {
            path: path.to_path_buf(),
            expected: format_sha256(expected),
            actual: format_sha256(actual),
            rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::pinned_world_provenance;

    #[test]
    fn pinned_world_identity_is_complete_and_deterministic() {
        let pinned = pinned_world_provenance();
        assert!(pinned.is_complete(), "every identity slot must be bound");
        assert_eq!(pinned, pinned_world_provenance());
    }
}

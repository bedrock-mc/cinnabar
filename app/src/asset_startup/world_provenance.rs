//! Checkout-pinned world-carrier identity expectations.
//!
//! Startup validates every compiled world blob against these expectations so
//! structurally valid carriers built from stale or foreign sources fail
//! closed instead of reaching gameplay. The registry bytes are embedded at
//! compile time from the checked-in protocol-2168 inputs, exactly like the
//! collision consumer, so validation never depends on the process working
//! directory or installed layout.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use assets::{
    BlobProvenance, RuntimeAssets, RuntimeAtmosphereAssets, canonical_source_manifest_sha256,
    registry_header_protocol,
};

use super::{ATMOSPHERE_COMPILE_COMMAND, AssetStartupError, COMPILE_COMMAND, format_sha256};

const BEDROCK_TARGET_JSON: &str = include_str!("../../../assets/bedrock-target.json");

#[derive(Deserialize)]
struct BedrockTarget {
    wire_protocol: u32,
    hashes: BTreeMap<Box<str>, Box<str>>,
}

/// The one checkout-wide authority for the active content registry wire
/// protocol, decoded from the cross-language target manifest.
///
/// Every production startup gate that consumes a content registry artifact
/// derives its expectation from this single value: the world-carrier
/// provenance gate below verifies that the pinned registry inputs stamp it,
/// and the collision binding (`movement`'s
/// `PhysicsCollisionRegistries::bind_coherent_assets`) rejects any installed
/// physics registry whose stamped header protocol differs. Raising this
/// constant therefore fails startup closed on both gates until the matching
/// carrier set is regenerated together, so a partial version flip can never
/// recreate the cross-carrier block-identity aliasing mechanism under zero
/// decode errors.
/// The active content registry protocol every startup gate binds to.
pub(crate) fn active_content_registry_protocol() -> u32 {
    static TARGET: OnceLock<BedrockTarget> = OnceLock::new();
    TARGET
        .get_or_init(|| {
            let target: BedrockTarget =
                serde_json::from_str(BEDROCK_TARGET_JSON).expect("valid Bedrock target manifest");
            for (name, bytes) in [
                ("block_registry", BLOCK_REGISTRY_BYTES),
                ("light_registry", LIGHT_REGISTRY_BYTES),
                ("biome_registry", BIOME_REGISTRY_BYTES),
            ] {
                let actual = format!("{:x}", Sha256::digest(bytes));
                assert_eq!(
                    target.hashes.get(name).map(AsRef::as_ref),
                    Some(actual.as_str())
                );
            }
            target
        })
        .wire_protocol
}

const VANILLA_SOURCE_JSON: &str = include_str!("../../../assets/vanilla-source.json");
const BLOCK_REGISTRY_BYTES: &[u8] =
    include_bytes!("../../../crates/assets/data/block-registry-v2168.bin");
const LIGHT_REGISTRY_BYTES: &[u8] =
    include_bytes!("../../../crates/assets/data/block-light-registry-v2168.bin");
const BIOME_REGISTRY_BYTES: &[u8] =
    include_bytes!("../../../crates/assets/data/biome-registry-v2168.bin");

/// The checked-in protocol-2168 block registry, shared with the collision
/// consumer so one embed feeds both physics and provenance validation.
pub(crate) const fn pinned_block_registry_bytes() -> &'static [u8] {
    BLOCK_REGISTRY_BYTES
}

/// The exact world-carrier identity this checkout pins: the canonical
/// vanilla source manifest plus each consumed protocol-2168 registry input.
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
    verify_pinned_registries_bind(active_content_registry_protocol())?;
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

/// Derives the provenance gate's protocol expectation from the shared
/// authority instead of assuming the embedded pins already match it.
///
/// The byte-level pins below enforce carrier identity transitively (a
/// carrier compiled from any other protocol's registries carries different
/// input hashes), but only this check ties those pins to
/// the Bedrock target manifest explicitly: a future authority bump
/// with stale embedded registry inputs fails here, naming both protocols,
/// before any carrier comparison can misattribute the mismatch.
fn verify_pinned_registries_bind(authority_protocol: u32) -> Result<(), AssetStartupError> {
    match registry_header_protocol(BLOCK_REGISTRY_BYTES) {
        Ok(stamped) if stamped == authority_protocol => Ok(()),
        Ok(stamped) => Err(AssetStartupError::PinnedRegistryProtocolMismatch {
            expected: authority_protocol,
            actual: stamped,
        }),
        Err(source) => Err(AssetStartupError::PinnedRegistryHeader {
            source: Box::new(source),
        }),
    }
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
    use super::{
        active_content_registry_protocol, pinned_world_provenance, verify_pinned_registries_bind,
    };

    #[test]
    fn pinned_world_identity_is_complete_and_deterministic() {
        let pinned = pinned_world_provenance();
        assert!(pinned.is_complete(), "every identity slot must be bound");
        assert_eq!(pinned, pinned_world_provenance());
    }

    /// The active authority stays exactly on the protocolgen target.
    #[test]
    fn active_content_authority_is_protocol_2168() {
        assert_eq!(active_content_registry_protocol(), 2168);
    }

    /// Consolidation witness: driving the gate with a mutated authority value
    /// flips its decision on the identical embedded pins, so the world gate's
    /// expectation provably hangs off the one shared knob.
    #[test]
    fn mutating_the_authority_flips_the_pinned_registry_expectation() {
        verify_pinned_registries_bind(active_content_registry_protocol())
            .expect("the checked-in pins must satisfy the shipped authority");

        let error = verify_pinned_registries_bind(1001)
            .expect_err("a legacy authority must reject the protocol-2168 pins");
        assert!(
            matches!(
                error,
                crate::asset_startup::AssetStartupError::PinnedRegistryProtocolMismatch {
                    expected: 1001,
                    actual: 2168
                }
            ),
            "unexpected error {error:?}"
        );
    }
}

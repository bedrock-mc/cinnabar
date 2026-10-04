//! Checkout-pinned world-carrier identity expectations.
//!
//! Startup validates every compiled world blob against these expectations so
//! structurally valid carriers built from stale or foreign sources fail
//! closed instead of reaching gameplay. The registry bytes are embedded at
//! compile time from the checked-in protocol-2193 inputs, exactly like the
//! collision consumer, so validation never depends on the process working
//! directory or installed layout.

use std::path::Path;

use assets::{
    RuntimeAssets, RuntimeAtmosphereAssets, canonical_source_manifest_sha256,
    registry_header_protocol,
};

use super::{ATMOSPHERE_COMPILE_COMMAND, AssetStartupError, COMPILE_COMMAND, format_sha256};

pub use assets::pinned_world_provenance;
pub(crate) use assets::{active_content_registry_protocol, pinned_block_registry_bytes};
const VANILLA_SOURCE_JSON: &str = assets::VANILLA_SOURCE_MANIFEST;

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
                rebuild_command: COMPILE_COMMAND.as_str(),
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
    match registry_header_protocol(pinned_block_registry_bytes()) {
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
mod offline_tests;

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

    /// Consolidation witness: driving the gate with a mutated authority value
    /// flips its decision on the identical embedded pins, so the world gate's
    /// expectation provably hangs off the one shared knob.
    #[test]
    fn mutating_the_authority_flips_the_pinned_registry_expectation() {
        verify_pinned_registries_bind(active_content_registry_protocol())
            .expect("the checked-in pins must satisfy the shipped authority");

        let error = verify_pinned_registries_bind(1001)
            .expect_err("a legacy authority must reject the protocol-2193 pins");
        assert!(
            matches!(
                error,
                crate::asset_startup::AssetStartupError::PinnedRegistryProtocolMismatch {
                    expected: 1001,
                    actual: 2193
                }
            ),
            "unexpected error {error:?}"
        );
    }
}

//! Checkout-pinned registry bytes and content identity shared by startup and observations.
use crate::{BlobProvenance, canonical_source_manifest_sha256};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::OnceLock};

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
pub fn active_content_registry_protocol() -> u32 {
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

const VANILLA_SOURCE_JSON: &str = crate::VANILLA_SOURCE_MANIFEST;
const BLOCK_REGISTRY_BYTES: &[u8] = include_bytes!("../data/block-registry-v2193.bin");
const LIGHT_REGISTRY_BYTES: &[u8] = include_bytes!("../data/block-light-registry-v2193.bin");
const BIOME_REGISTRY_BYTES: &[u8] = include_bytes!("../data/biome-registry-v2193.bin");

/// The checked-in protocol-2193 block registry, shared with the collision
/// consumer so one embed feeds both physics and provenance validation.
pub const fn pinned_block_registry_bytes() -> &'static [u8] {
    BLOCK_REGISTRY_BYTES
}

/// Resolves pinned identities when the optional world carrier is unavailable.
pub fn pinned_block_sequential_id(network_hash: u32) -> Option<u32> {
    static IDS: OnceLock<std::collections::HashMap<u32, u32>> = OnceLock::new();
    IDS.get_or_init(|| {
        crate::read_registry_for_protocol(BLOCK_REGISTRY_BYTES, active_content_registry_protocol())
            .expect("validated pinned block registry")
            .iter()
            .map(|record| (record.network_hash, record.sequential_id))
            .collect()
    })
    .get(&network_hash)
    .copied()
}

/// The exact world-carrier identity this checkout pins: the canonical
/// vanilla source manifest plus each consumed protocol-2193 registry input.
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

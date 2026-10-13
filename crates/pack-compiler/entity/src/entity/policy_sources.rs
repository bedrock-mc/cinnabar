//! Shared policy records admitted into every entity catalog.
use assets::EntityAssetSource;
use sha2::{Digest, Sha256};

const SOURCES: &[fn() -> EntityAssetSource] = &[placement, interactions];
pub(super) const SOURCE_COUNT: usize = SOURCES.len();

/// Builds the policy records whose slots and bytes pack admission reserves.
pub(super) fn records() -> impl Iterator<Item = EntityAssetSource> {
    SOURCES.iter().map(|build| build())
}

/// Identifies the placement rules used by every catalog's item presentation.
fn placement() -> EntityAssetSource {
    EntityAssetSource {
        path: assets::placement::SOURCE_PATH.into(),
        source_bytes: assets::placement::SOURCE_BYTES.len() as u32,
        source_sha256: Sha256::digest(assets::placement::SOURCE_BYTES).into(),
    }
}

/// Identifies the interaction traits compiled for every admitted actor.
fn interactions() -> EntityAssetSource {
    let (source_bytes, source_sha256) = assets::interactions::source_identity();
    EntityAssetSource {
        path: assets::interactions::SOURCE_PATH.into(),
        source_bytes,
        source_sha256,
    }
}

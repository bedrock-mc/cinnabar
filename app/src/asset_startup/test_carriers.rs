//! Minimal valid startup carriers pinned to the checkout's vanilla manifest.

use assets::{
    AtmosphereRole, AtmosphereTexture, CompiledAtmosphereAssets, CompiledEntityAssets,
    EntityAssetKind, EntityAssetSource, EntityAssetSymbol,
};
use sha2::{Digest, Sha256};

fn manifest() -> [u8; 32] {
    super::canonical_source_manifest_sha256(super::VANILLA_SOURCE_JSON)
}

/// An entity carrier with one source; `seed` varies its identity.
pub(crate) fn synthetic_entity_blob(seed: u8) -> Box<[u8]> {
    assets::encode_entity_blob(&CompiledEntityAssets {
        source_manifest_sha256: manifest(),
        block_visual_count: 0,
        sources: vec![EntityAssetSource {
            path: "entity/allay.entity.json".into(),
            source_bytes: 1,
            source_sha256: [seed.wrapping_add(1); 32],
        }]
        .into(),
        symbols: vec![EntityAssetSymbol {
            kind: EntityAssetKind::Entity,
            identifier: "minecraft:allay".into(),
            source_index: 0,
            dependencies: Box::new([]),
        }]
        .into(),
        geometries: Box::new([]),
        animation_clips: Box::new([]),
        animation_channels: Box::new([]),
        animation_keyframes: Box::new([]),
        molang_symbols: Box::new([]),
        molang_expressions: Box::new([]),
        molang_ops: Box::new([]),
        molang_collections: Box::new([]),
        molang_collection_items: Box::new([]),
        controllers: Box::new([]),
        controller_states: Box::new([]),
        controller_animations: Box::new([]),
        controller_transitions: Box::new([]),
        rig_bindings: Box::new([]),
        rig_geometries: Box::new([]),
        rig_animations: Box::new([]),
        rig_controllers: Box::new([]),
        item_visuals: Box::new([]),
        item_visual_aliases: Box::new([]),
        render: Default::default(),
    })
    .unwrap()
}

/// An atmosphere carrier with flat sun, moon and cloud textures.
pub(crate) fn synthetic_atmosphere_blob() -> Box<[u8]> {
    let textures = [
        (AtmosphereRole::Sun, "textures/environment/sun.png", 32, 32),
        (
            AtmosphereRole::MoonPhases,
            "textures/environment/moon_phases.png",
            128,
            64,
        ),
        (
            AtmosphereRole::Clouds,
            "textures/environment/clouds.png",
            256,
            256,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (role, source_path, width, height))| {
        let rgba8 = vec![index as u8; (width * height * 4) as usize];
        AtmosphereTexture {
            role,
            source_path: source_path.into(),
            source_bytes: 1,
            source_sha256: [index as u8 + 1; 32],
            pixels_sha256: Sha256::digest(&rgba8).into(),
            width,
            height,
            rgba8: rgba8.into_boxed_slice(),
        }
    })
    .collect();
    assets::encode_atmosphere_blob(&CompiledAtmosphereAssets {
        source_manifest_sha256: manifest(),
        textures,
        biome_profiles: Box::new([]),
        fog_profiles: Box::new([]),
    })
    .unwrap()
}

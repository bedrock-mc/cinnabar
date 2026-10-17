//! Prepares sun, moon, cloud, fog and numbered sky resources on the reload worker.

use assets::atmosphere::AtmosphereTexture;
use sha2::{Digest, Sha256};

use super::*;

/// Overlays only the environment components the renderer already consumes.
pub(super) fn prepare_atmosphere(
    view: &LayeredPackView,
    base: &render_scene::AtmosphereTextureAssets,
) -> render_scene::AtmosphereTextureAssets {
    let Some(runtime) = base.runtime() else {
        return base.clone();
    };
    let mut textures = Vec::new();
    let mut digest = Sha256::new();
    digest.update(b"pack atmosphere\0");
    digest.update(base.identity());
    let skyboxes = pack_runtime::pack_skybox::prepare_pack_skyboxes(view);
    let has_skyboxes = skyboxes.iter().any(|cube| cube.face_mask() != 0);
    if has_skyboxes {
        digest.update(b"numbered pack skybox\0");
        for (dimension, cube) in assets::SkyboxDimension::ALL.into_iter().zip(&skyboxes) {
            digest.update([dimension as u8]);
            digest.update(cube.face_mask().to_le_bytes());
            for (face, image) in cube.faces.iter().enumerate() {
                if let Some(image) = image {
                    digest.update([face as u8]);
                    digest.update(image.width.to_le_bytes());
                    digest.update(image.height.to_le_bytes());
                    hash_field(&mut digest, &image.rgba8);
                }
            }
        }
    }
    for texture in runtime.textures() {
        let Some(decoded) = decode_pack_texture(view, &texture.source_path) else {
            continue;
        };
        if !supports_texture_dimensions(texture.role, decoded.width, decoded.height) {
            bevy::log::warn!(
                path = %texture.source_path,
                width = decoded.width,
                height = decoded.height,
                "optional cloud texture exceeds the current mesh contract; retaining base clouds"
            );
            continue;
        }
        digest.update(b"texture\0");
        digest.update((texture.role as u32).to_le_bytes());
        hash_field(&mut digest, texture.source_path.as_bytes());
        digest.update(decoded.width.to_le_bytes());
        digest.update(decoded.height.to_le_bytes());
        hash_field(&mut digest, &decoded.rgba8);
        textures.push(AtmosphereTexture {
            width: decoded.width,
            height: decoded.height,
            pixels_sha256: Sha256::digest(&decoded.rgba8).into(),
            rgba8: decoded.rgba8,
            ..texture.clone()
        });
    }
    let mut fogs: BTreeMap<_, _> = runtime
        .fog_profiles()
        .iter()
        .map(|profile| (profile.identifier.clone(), profile.clone()))
        .collect();
    let mut biomes: BTreeMap<_, _> = runtime
        .biome_profiles()
        .iter()
        .map(|profile| (profile.biome_identifier.clone(), profile.clone()))
        .collect();
    for (path, bytes) in environment_files(view) {
        let Some(root) = parse_pack_json(&bytes) else {
            continue;
        };
        digest.update(b"environment json\0");
        hash_field(&mut digest, path.as_bytes());
        hash_field(&mut digest, &bytes);
        if path.starts_with("fogs/") {
            if let Some(profile) = fog_profile(&root)
                && (fogs.len() < assets::atmosphere::MAX_ENVIRONMENT_PROFILES
                    || fogs.contains_key(&profile.identifier))
            {
                fogs.insert(profile.identifier.clone(), profile);
            }
        } else {
            overlay_biome_profile(&root, &mut biomes, &fogs);
        }
    }
    if textures.is_empty()
        && fogs.values().eq(runtime.fog_profiles())
        && biomes.values().eq(runtime.biome_profiles())
    {
        return if has_skyboxes {
            base.clone()
                .with_skyboxes(skyboxes, digest.finalize().into())
        } else {
            base.clone()
        };
    }
    let prepared = match runtime.with_resource_pack_overrides(
        &textures,
        &biomes.into_values().collect::<Vec<_>>(),
        &fogs.into_values().collect::<Vec<_>>(),
    ) {
        Ok(runtime) => render_scene::AtmosphereTextureAssets::new(
            Arc::new(runtime),
            digest.clone().finalize().into(),
        ),
        Err(error) => {
            bevy::log::warn!(%error, "optional pack atmosphere ignored");
            base.clone()
        }
    };
    if has_skyboxes {
        prepared.with_skyboxes(skyboxes, digest.finalize().into())
    } else {
        prepared
    }
}

/// Frames variable-length fields so adjacent metadata and payloads have one interpretation.
fn hash_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}

/// Optional packs must not publish cloud masks the fixed-size mesher cannot consume.
pub(super) fn supports_texture_dimensions(
    role: assets::atmosphere::AtmosphereRole,
    width: u32,
    height: u32,
) -> bool {
    role != assets::atmosphere::AtmosphereRole::Clouds
        || (width == meshing::CLOUD_MASK_SIZE && height == meshing::CLOUD_MASK_SIZE)
}

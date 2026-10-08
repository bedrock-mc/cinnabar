//! Cinnabar extension: prepare optional environment layers for a live pack swap.

use super::resource_packs::{decode_pack_texture, parse_pack_json};
use assets::{AtmosphereTexture, BiomeVisualProfile};
use bevy::prelude::Resource;
use resource_pack::LayeredPackView;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

mod biomes;
mod fog;
use fog::{fog_profile, parse_rgb};
mod particles;
pub(super) use biomes::apply_biome_overlay;

/// Original carriers retained so removing every optional layer restores their resources.
#[derive(Resource, Clone)]
pub(crate) struct EnvironmentBase {
    atmosphere: render::AtmosphereTextureAssets,
    particles: Option<Arc<assets::RuntimeParticleAssets>>,
}

impl EnvironmentBase {
    /// Retains startup carrier resources before any optional stack is applied.
    pub(crate) fn new(
        atmosphere: render::AtmosphereTextureAssets,
        particles: Option<Arc<assets::RuntimeParticleAssets>>,
    ) -> Self {
        Self {
            atmosphere,
            particles,
        }
    }
}

pub(super) struct PreparedEnvironment {
    pub(super) atmosphere: Option<render::AtmosphereTextureAssets>,
    pub(super) particles: Option<::particles::ParticleSystem>,
    pub(super) dependencies: super::pack_reload_diff::Dependencies,
}

/// Decodes and builds environment resources entirely on the pack reload worker.
pub(super) fn prepare_environment(
    view: &LayeredPackView,
    base: &EnvironmentBase,
    atmosphere_changed: bool,
    particles_changed: bool,
) -> PreparedEnvironment {
    use super::pack_reload_diff::{Subscriber, compile};
    let stack = view.shared_stack();
    let mut dependencies = Default::default();
    let atmosphere = atmosphere_changed.then(|| {
        compile(Subscriber::Atmosphere, &stack, &mut dependencies, |view| {
            prepare_atmosphere(view, &base.atmosphere)
        })
    });
    let particles = particles_changed.then(|| {
        compile(Subscriber::Particles, &stack, &mut dependencies, |view| {
            particles::prepare_particles(view, base.particles.as_deref())
        })
    });
    PreparedEnvironment {
        atmosphere,
        particles,
        dependencies,
    }
}

/// Overlays only the environment components the renderer already consumes.
fn prepare_atmosphere(
    view: &LayeredPackView,
    base: &render::AtmosphereTextureAssets,
) -> render::AtmosphereTextureAssets {
    let Some(runtime) = base.runtime() else {
        return base.clone();
    };
    let mut textures = Vec::new();
    let mut digest = Sha256::new();
    digest.update(base.identity());
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
        digest.update(&decoded.rgba8);
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
        digest.update(path.as_bytes());
        digest.update(&bytes);
        if path.starts_with("fogs/") {
            if let Some(profile) = fog_profile(&root)
                && (fogs.len() < assets::MAX_ENVIRONMENT_PROFILES
                    || fogs.contains_key(&profile.identifier))
            {
                fogs.insert(profile.identifier.clone(), profile);
            }
        } else {
            overlay_biome_profile(&root, &mut biomes);
        }
    }
    if textures.is_empty()
        && fogs.values().eq(runtime.fog_profiles())
        && biomes.values().eq(runtime.biome_profiles())
    {
        return base.clone();
    }
    match runtime.with_resource_pack_overrides(
        &textures,
        &biomes.into_values().collect::<Vec<_>>(),
        &fogs.into_values().collect::<Vec<_>>(),
    ) {
        Ok(runtime) => {
            render::AtmosphereTextureAssets::new(Arc::new(runtime), digest.finalize().into())
        }
        Err(error) => {
            bevy::log::warn!(%error, "optional pack atmosphere ignored");
            base.clone()
        }
    }
}

/// Optional packs must not publish cloud masks the fixed-size mesher cannot consume.
fn supports_texture_dimensions(role: assets::AtmosphereRole, width: u32, height: u32) -> bool {
    role != assets::AtmosphereRole::Clouds
        || (width == meshing::CLOUD_MASK_SIZE && height == meshing::CLOUD_MASK_SIZE)
}

/// Preserves pack priority when definitions in different files reuse an identifier.
fn environment_files(view: &LayeredPackView) -> Vec<(String, Box<[u8]>)> {
    let mut files = layered_json(view, "fogs/");
    files.extend(layered_json(view, "biomes/"));
    files
}

/// Reads bounded JSON layers, allowing the higher pack to replace a named definition.
fn layered_json(view: &LayeredPackView, prefix: &str) -> Vec<(String, Box<[u8]>)> {
    let mut result = Vec::new();
    let mut total = 0;
    for pack in view.layers() {
        for path in pack
            .files_under(prefix)
            .iter()
            .filter(|path| path.ends_with(".json"))
        {
            let Some(bytes) = pack.read_file(path).ok().flatten() else {
                continue;
            };
            total += bytes.len();
            if total > resource_pack::MAX_WINNING_BYTES
                || result.len() >= resource_pack::MAX_WINNING_FILES
            {
                return result;
            }
            result.push(((*path).to_owned(), bytes));
        }
    }
    result
}

/// Updates known client-biome environment components, retaining absent base values.
fn overlay_biome_profile(root: &Value, profiles: &mut BTreeMap<Box<str>, BiomeVisualProfile>) {
    let biome = &root["minecraft:client_biome"];
    let Some(identifier) = biome["description"]["identifier"].as_str() else {
        return;
    };
    let Some(profile) = profiles.get_mut(identifier) else {
        return;
    };
    let components = &biome["components"];
    for (key, field, target) in [
        (
            "minecraft:fog_appearance",
            "fog_identifier",
            &mut profile.fog_identifier,
        ),
        (
            "minecraft:atmosphere_identifier",
            "atmosphere_identifier",
            &mut profile.atmosphere_identifier,
        ),
        (
            "minecraft:lighting_identifier",
            "lighting_identifier",
            &mut profile.lighting_identifier,
        ),
    ] {
        if let Some(value) = components[key][field].as_str().filter(|value| {
            !value.is_empty() && value.len() <= assets::MAX_ENVIRONMENT_IDENTIFIER_BYTES
        }) {
            *target = value.into();
        }
    }
    if let Some(rgb) = parse_rgb(&components["minecraft:sky_color"]["sky_color"]) {
        profile.sky_rgb8 = Some(rgb);
    }
}

#[cfg(test)]
mod tests;

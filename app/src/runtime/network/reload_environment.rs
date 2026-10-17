//! Cinnabar extension: prepare optional environment layers for a live pack swap.

use assets::atmosphere::BiomeVisualProfile;
use bevy::prelude::Resource;
use pack_runtime::pack_textures::{decode_pack_texture, parse_pack_json};
use resource_pack::LayeredPackView;
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

mod atmosphere;
use atmosphere::prepare_atmosphere;
#[cfg(test)]
use atmosphere::supports_texture_dimensions;
mod biomes;
mod fog;
use fog::{fog_profile, parse_rgb};
mod images;
mod particles;
mod publication;
pub(super) use biomes::apply_biome_overlay;
pub(super) use publication::EnvironmentPublication;

/// Original carriers retained so removing every optional layer restores their resources.
#[derive(Resource, Clone)]
pub(crate) struct EnvironmentBase {
    atmosphere: render_scene::AtmosphereTextureAssets,
    particles: Option<Arc<assets::RuntimeParticleAssets>>,
    weather: render_scene::WeatherTextureAssets,
    screen_overlays: Option<Arc<render_scene::ScreenOverlayTextures>>,
}

impl EnvironmentBase {
    /// Retains startup carrier resources before any optional stack is applied.
    pub(crate) fn new(
        atmosphere: render_scene::AtmosphereTextureAssets,
        particles: Option<Arc<assets::RuntimeParticleAssets>>,
        weather: render_scene::WeatherTextureAssets,
    ) -> Self {
        Self {
            atmosphere,
            particles,
            weather,
            screen_overlays: None,
        }
    }
}

pub(super) struct PreparedEnvironment {
    atmosphere: Option<PreparedAtmosphere>,
    pub(super) particles: Option<::particles::ParticleSystem>,
    pub(super) dependencies: pack_runtime::reload_diff::Dependencies,
}

/// Environment images prepared together under the atmosphere subscriber's dependencies.
struct PreparedAtmosphere {
    atmosphere: render_scene::AtmosphereTextureAssets,
    weather: render_scene::WeatherTextureAssets,
    screen_overlays: Option<Arc<render_scene::ScreenOverlayTextures>>,
}

/// Decodes and builds environment resources entirely on the pack reload worker.
pub(super) fn prepare_environment(
    view: &LayeredPackView,
    base: &EnvironmentBase,
    atmosphere_changed: bool,
    particles_changed: bool,
) -> PreparedEnvironment {
    use pack_runtime::reload_diff::{Subscriber, compile};
    let stack = view.shared_stack();
    let mut dependencies = Default::default();
    let atmosphere = atmosphere_changed.then(|| {
        compile(Subscriber::Atmosphere, &stack, &mut dependencies, |view| {
            PreparedAtmosphere {
                atmosphere: prepare_atmosphere(view, &base.atmosphere),
                weather: images::prepare_weather(view, &base.weather),
                screen_overlays: images::prepare_screen_overlays(
                    view,
                    base.screen_overlays.as_ref(),
                ),
            }
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

/// Collects fog definitions before biome routes while preserving pack priority.
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

/// Applies client-biome components only when their fog route resolves, retaining absent values.
fn overlay_biome_profile(
    root: &Value,
    profiles: &mut BTreeMap<Box<str>, BiomeVisualProfile>,
    fogs: &BTreeMap<Box<str>, assets::atmosphere::FogProfile>,
) {
    let biome = &root["minecraft:client_biome"];
    let Some(identifier) = biome["description"]["identifier"].as_str() else {
        return;
    };
    if identifier.is_empty()
        || identifier.len() > assets::atmosphere::MAX_ENVIRONMENT_IDENTIFIER_BYTES
    {
        return;
    }
    let mut profile = match profiles.get(identifier) {
        Some(profile) => profile.clone(),
        None => {
            if profiles.len() >= assets::atmosphere::MAX_ENVIRONMENT_PROFILES {
                return;
            }
            let Some(mut fallback) = profiles
                .get(assets::facts::dimension_fallback_biome(0).expect("pinned overworld biome"))
                .cloned()
            else {
                return;
            };
            fallback.biome_identifier = identifier.into();
            fallback
        }
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
            !value.is_empty() && value.len() <= assets::atmosphere::MAX_ENVIRONMENT_IDENTIFIER_BYTES
        }) {
            *target = value.into();
        }
    }
    if let Some(rgb) = parse_rgb(&components["minecraft:sky_color"]["sky_color"]) {
        profile.sky_rgb8 = Some(rgb);
    }
    if !fogs.contains_key(&profile.fog_identifier) {
        bevy::log::warn!(
            biome = identifier,
            fog = %profile.fog_identifier,
            "optional client-biome profile has an unresolved fog route; skipping entry"
        );
        return;
    }
    profiles.insert(identifier.into(), profile);
}

#[cfg(test)]
mod tests;

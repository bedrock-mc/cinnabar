//! Converts local Java sky resources to the renderer's existing texture contract.

use super::JavaTexture;
use crate::{AtmosphereRole, AtmosphereTexture};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, error::Error};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Derives the moon atlas order from authored keys rather than a second phase table.
pub(super) fn atmosphere_textures(
    textures: &BTreeMap<Box<str>, JavaTexture>,
    jsons: &BTreeMap<String, Value>,
) -> Result<[AtmosphereTexture; AtmosphereRole::ALL.len()]> {
    let texture = |path: &str| {
        textures
            .get(path)
            .ok_or_else(|| format!("missing Java sky texture {path}"))
    };
    let sun_path = "assets/minecraft/textures/environment/celestial/sun.png";
    let cloud_path = "assets/minecraft/textures/environment/clouds.png";
    let moon = jsons
        .get("data/minecraft/timeline/moon.json")
        .ok_or("missing Java moon timeline")?;
    let keys = moon["tracks"]["minecraft:visual/moon_phase"]["keyframes"]
        .as_array()
        .filter(|keys| keys.len() == 8)
        .ok_or("invalid Java moon phase timeline")?;
    let period = moon["period_ticks"]
        .as_u64()
        .filter(|period| *period > 0 && *period % keys.len() as u64 == 0)
        .ok_or("invalid Java moon timeline period")?;
    let mut atlas = JavaTexture {
        width: 128,
        height: 64,
        rgba: vec![0; 128 * 64 * 4].into_boxed_slice(),
    };
    for (phase, key) in keys.iter().enumerate() {
        if key["ticks"].as_u64() != Some(phase as u64 * (period / keys.len() as u64)) {
            return Err("unexpected Java moon phase time".into());
        }
        let name = key["value"]
            .as_str()
            .filter(|name| {
                name.bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
            })
            .ok_or("invalid Java moon phase name")?;
        let path = format!("assets/minecraft/textures/environment/celestial/moon/{name}.png");
        let image = texture(&path)?;
        if image.width != 32 || image.height != 32 {
            return Err("unexpected Java moon texture size".into());
        }
        for row in 0..32 {
            let destination = ((phase / 4 * 32 + row) * 128 + phase % 4 * 32) * 4;
            atlas.rgba[destination..destination + 32 * 4]
                .copy_from_slice(&image.rgba[row * 32 * 4..(row + 1) * 32 * 4]);
        }
    }
    Ok([
        prepared(AtmosphereRole::Sun, sun_path, texture(sun_path)?),
        prepared(AtmosphereRole::MoonPhases, "java:moon_phases", &atlas),
        prepared(AtmosphereRole::Clouds, cloud_path, texture(cloud_path)?),
    ])
}

/// Produces an immutable upload record from already authenticated local pixels.
fn prepared(role: AtmosphereRole, path: &str, image: &JavaTexture) -> AtmosphereTexture {
    let digest: [u8; 32] = Sha256::digest(&image.rgba).into();
    AtmosphereTexture {
        role,
        source_path: path.into(),
        source_bytes: image.rgba.len() as u32,
        source_sha256: digest,
        pixels_sha256: digest,
        width: image.width,
        height: image.height,
        rgba8: image.rgba.clone(),
    }
}

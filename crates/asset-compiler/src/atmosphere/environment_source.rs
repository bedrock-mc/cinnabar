use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use assets::{AssetError, MAX_ENVIRONMENT_IDENTIFIER_BYTES, MAX_ENVIRONMENT_PROFILES};
use serde::Deserialize;

use super::invalid;

const MAX_ENVIRONMENT_JSON_BYTES: usize = 16 * 1024 * 1024;

#[derive(Deserialize)]
pub(super) struct ClientBiomeDocument {
    #[serde(rename = "minecraft:client_biome")]
    pub(super) biome: ClientBiome,
}

#[derive(Deserialize)]
pub(super) struct ClientBiome {
    pub(super) description: EnvironmentDescription,
    pub(super) components: ClientBiomeEnvironmentComponents,
}

#[derive(Deserialize)]
pub(super) struct EnvironmentDescription {
    pub(super) identifier: String,
}

#[derive(Deserialize)]
pub(super) struct ClientBiomeEnvironmentComponents {
    #[serde(rename = "minecraft:fog_appearance")]
    pub(super) fog: FogAppearance,
    #[serde(rename = "minecraft:atmosphere_identifier")]
    pub(super) atmosphere: AtmosphereIdentifier,
    #[serde(rename = "minecraft:lighting_identifier")]
    pub(super) lighting: LightingIdentifier,
    #[serde(rename = "minecraft:sky_color")]
    pub(super) sky: Option<SkyColor>,
}

#[derive(Deserialize)]
pub(super) struct FogAppearance {
    pub(super) fog_identifier: String,
}

#[derive(Deserialize)]
pub(super) struct AtmosphereIdentifier {
    pub(super) atmosphere_identifier: String,
}

#[derive(Deserialize)]
pub(super) struct LightingIdentifier {
    pub(super) lighting_identifier: String,
}

#[derive(Deserialize)]
pub(super) struct SkyColor {
    pub(super) sky_color: String,
}

#[derive(Deserialize)]
pub(super) struct FogSettingsDocument {
    #[serde(rename = "minecraft:fog_settings")]
    pub(super) settings: FogSettings,
}

#[derive(Deserialize)]
pub(super) struct FogSettings {
    pub(super) description: EnvironmentDescription,
    pub(super) distance: BTreeMap<String, FogDistanceSource>,
}

#[derive(Deserialize)]
pub(super) struct FogDistanceSource {
    pub(super) fog_start: f32,
    pub(super) fog_end: f32,
    pub(super) fog_color: String,
    pub(super) render_distance_type: String,
}

pub(super) fn sorted_environment_files(
    path: &Path,
    suffix: &str,
) -> Result<Vec<PathBuf>, AssetError> {
    let entries = fs::read_dir(path).map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let file_type = entry.file_type().map_err(|source| AssetError::Io {
            path: entry.path(),
            source,
        })?;
        if file_type.is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(suffix))
        {
            files.push(entry.path());
        }
    }
    files.sort();
    if files.len() > MAX_ENVIRONMENT_PROFILES {
        return Err(invalid(format!(
            "environment directory has {} files, exceeding {MAX_ENVIRONMENT_PROFILES}",
            files.len()
        )));
    }
    Ok(files)
}

pub(super) fn read_environment_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
) -> Result<T, AssetError> {
    let file = File::open(path).map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_ENVIRONMENT_JSON_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > MAX_ENVIRONMENT_JSON_BYTES {
        return Err(AssetError::JsonTooLarge {
            path: path.to_path_buf(),
            size: bytes.len(),
            max: MAX_ENVIRONMENT_JSON_BYTES,
        });
    }
    serde_json::from_slice(&bytes).map_err(|source| AssetError::Json {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn validate_environment_identifier(identifier: &str) -> Result<(), AssetError> {
    if identifier.is_empty() || identifier.len() > MAX_ENVIRONMENT_IDENTIFIER_BYTES {
        return Err(invalid(format!(
            "environment identifier length {} is outside 1..={MAX_ENVIRONMENT_IDENTIFIER_BYTES}",
            identifier.len()
        )));
    }
    Ok(())
}

pub(super) fn parse_environment_rgb(value: &str) -> Result<u32, AssetError> {
    let digits = value
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6)
        .ok_or_else(|| invalid(format!("invalid environment RGB colour {value}")))?;
    u32::from_str_radix(digits, 16)
        .map_err(|_| invalid(format!("invalid environment RGB colour {value}")))
}

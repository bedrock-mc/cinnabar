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
    // Vanilla leaves an absent identifier component unset and renders the
    // biome with the default atmosphere and lighting settings.
    #[serde(rename = "minecraft:atmosphere_identifier")]
    pub(super) atmosphere: Option<AtmosphereIdentifier>,
    #[serde(rename = "minecraft:lighting_identifier")]
    pub(super) lighting: Option<LightingIdentifier>,
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
    pub(super) transition_fog: Option<FogTransitionSource>,
}

#[derive(Deserialize)]
pub(super) struct FogTransitionSource {
    pub(super) init_fog: Box<FogDistanceSource>,
    pub(super) min_percent: f32,
    pub(super) mid_seconds: f32,
    pub(super) mid_percent: f32,
    pub(super) max_seconds: f32,
}

/// Retains the initial setting and timing fields from a classic fog transition.
pub(super) fn compile_transition(
    source: FogTransitionSource,
) -> Result<assets::FogTransition, AssetError> {
    let initial = source.init_fog;
    let transition = assets::FogTransition {
        mode: assets::FogDistanceMode::from_source_name(&initial.render_distance_type)
            .ok_or_else(|| invalid("unsupported initial fog distance mode"))?,
        start_bits: initial.fog_start.to_bits(),
        end_bits: initial.fog_end.to_bits(),
        rgb8: parse_environment_rgb(&initial.fog_color)?,
        min_percent_bits: source.min_percent.to_bits(),
        mid_seconds_bits: source.mid_seconds.to_bits(),
        mid_percent_bits: source.mid_percent.to_bits(),
        max_seconds_bits: source.max_seconds.to_bits(),
    };
    if initial.transition_fog.is_some() || !transition.is_valid() {
        return Err(invalid("invalid fog transition"));
    }
    Ok(transition)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_retains_initial_color_distance_and_profile_timing() {
        let source: FogDistanceSource = serde_json::from_str(
            r##"{
            "fog_start": 0, "fog_end": 60, "fog_color": "#44AFF5",
            "render_distance_type": "fixed", "transition_fog": {
                "init_fog": {"fog_start": 2, "fog_end": 3,
                    "fog_color": "#123456", "render_distance_type": "render"},
                "min_percent": 0.1, "mid_seconds": 4,
                "mid_percent": 0.7, "max_seconds": 12
            }}"##,
        )
        .unwrap();
        let transition = compile_transition(source.transition_fog.unwrap()).unwrap();
        assert_eq!(transition.mode, assets::FogDistanceMode::RenderRelative);
        assert_eq!(f32::from_bits(transition.start_bits), 2.0);
        assert_eq!(f32::from_bits(transition.end_bits), 3.0);
        assert_eq!(transition.rgb8, 0x123456);
        assert_eq!(f32::from_bits(transition.min_percent_bits), 0.1);
        assert_eq!(f32::from_bits(transition.mid_seconds_bits), 4.0);
        assert_eq!(f32::from_bits(transition.mid_percent_bits), 0.7);
        assert_eq!(f32::from_bits(transition.max_seconds_bits), 12.0);
        let bytes = serde_json::to_vec(&transition).unwrap();
        assert_eq!(
            serde_json::from_slice::<assets::FogTransition>(&bytes).unwrap(),
            transition
        );
        assert!(
            !assets::FogTransition {
                max_seconds_bits: 1.0_f32.to_bits(),
                ..transition
            }
            .is_valid()
        );
    }
}

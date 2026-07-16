use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use image::{ImageFormat, ImageReader, Limits};
use serde::Deserialize;
use serde_json::Value;

use assets::{
    AssetError, BIOME_RULE_FLAG_GRASS_SHADED, BiomeRegistryRecord, BiomeRule, CompiledBiomeAssets,
    MAX_BIOME_NAME_BYTES, MAX_BIOME_RULES, TINT_MAP_BYTES, TINT_MAP_SIZE, TintMapId, TintSource,
};

const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_COLORMAP_FILE_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
struct ClientBiomeDocument {
    #[serde(rename = "minecraft:client_biome")]
    biome: ClientBiome,
}

#[derive(Deserialize)]
struct ClientBiome {
    description: BiomeDescription,
    #[serde(default)]
    components: ClientBiomeComponents,
}

#[derive(Deserialize)]
struct BiomeDescription {
    identifier: String,
}

#[derive(Default, Deserialize)]
struct ClientBiomeComponents {
    #[serde(rename = "minecraft:grass_appearance")]
    grass: Option<GrassAppearance>,
    #[serde(rename = "minecraft:foliage_appearance")]
    foliage: Option<ColourAppearance>,
    #[serde(rename = "minecraft:dry_foliage_color")]
    dry_foliage: Option<ColourAppearance>,
    #[serde(rename = "minecraft:water_appearance")]
    water: Option<WaterAppearance>,
}

#[derive(Deserialize)]
struct GrassAppearance {
    color: Option<Value>,
    #[serde(default)]
    grass_is_shaded: bool,
}

#[derive(Deserialize)]
struct ColourAppearance {
    color: Value,
}

#[derive(Deserialize)]
struct WaterAppearance {
    surface_color: Option<Value>,
}

#[derive(Deserialize)]
struct BehaviourBiomeDocument {
    #[serde(rename = "minecraft:biome")]
    biome: BehaviourBiome,
}

#[derive(Deserialize)]
struct BehaviourBiome {
    description: BiomeDescription,
    components: BehaviourBiomeComponents,
}

#[derive(Deserialize)]
struct BehaviourBiomeComponents {
    #[serde(rename = "minecraft:climate")]
    climate: Climate,
}

#[derive(Clone, Copy, Deserialize)]
struct Climate {
    temperature: f32,
    downfall: f32,
}

/// Compiles modern client-biome rules, behaviour climates, and all tint maps.
pub fn compile_biome_assets(
    resource_pack: &Path,
    behavior_pack: &Path,
    registry: &[BiomeRegistryRecord],
) -> Result<CompiledBiomeAssets, AssetError> {
    if registry.len() > MAX_BIOME_RULES {
        return Err(invalid("biome registry exceeds compiler bounds"));
    }
    let client = read_client_biomes(&resource_pack.join("biomes"))?;
    let climates = read_behaviour_biomes(&behavior_pack.join("biomes"))?;
    let registry_names = registry
        .iter()
        .map(|record| record.name.as_ref())
        .collect::<BTreeSet<_>>();
    let client_names = client.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let climate_names = climates.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if registry_names != client_names {
        return Err(invalid(
            "modern client-biome identifiers do not exactly match BIOREG01",
        ));
    }
    if registry_names != climate_names {
        return Err(invalid(
            "behaviour biome identifiers do not exactly match BIOREG01",
        ));
    }

    let tint_maps_rgb8 = read_tint_maps(resource_pack)?;
    let mut rules = Vec::with_capacity(registry.len());
    for record in registry {
        let appearance = &client[record.name.as_ref()];
        let climate = climates[record.name.as_ref()];
        validate_climate(climate.temperature, climate.downfall)?;
        let grass = appearance
            .grass
            .as_ref()
            .and_then(|appearance| appearance.color.as_ref())
            .map(|value| parse_colour_source(value, ColourDomain::Grass))
            .transpose()?
            .unwrap_or_else(|| TintSource::map(TintMapId::Grass));
        let foliage = appearance
            .foliage
            .as_ref()
            .map(|appearance| parse_colour_source(&appearance.color, ColourDomain::Foliage))
            .transpose()?
            .unwrap_or_else(|| TintSource::map(TintMapId::Foliage));
        let dry_foliage = appearance
            .dry_foliage
            .as_ref()
            .map(|appearance| parse_colour_source(&appearance.color, ColourDomain::DryFoliage))
            .transpose()?
            .unwrap_or_else(|| TintSource::map(TintMapId::DryFoliage));
        let water = appearance
            .water
            .as_ref()
            .and_then(|appearance| appearance.surface_color.as_ref())
            .map(parse_direct_colour)
            .transpose()?
            .map(TintSource::direct)
            .unwrap_or_else(|| TintSource::direct(0x44_aff5));
        let grass_is_shaded = appearance
            .grass
            .as_ref()
            .is_some_and(|appearance| appearance.grass_is_shaded);
        let flags = if grass_is_shaded {
            BIOME_RULE_FLAG_GRASS_SHADED
        } else {
            0
        };
        rules.push(BiomeRule {
            id: record.id,
            name: record.name.clone(),
            flags,
            grass,
            foliage,
            dry_foliage,
            water,
            temperature_bits: climate.temperature.to_bits(),
            downfall_bits: climate.downfall.to_bits(),
        });
    }
    Ok(CompiledBiomeAssets {
        tint_maps_rgb8,
        rules: rules.into_boxed_slice(),
    })
}

fn read_client_biomes(path: &Path) -> Result<BTreeMap<String, ClientBiomeComponents>, AssetError> {
    let mut output = BTreeMap::new();
    for source in sorted_files(path, ".client_biome.json")? {
        let document: ClientBiomeDocument = read_json(&source)?;
        let name = document.biome.description.identifier;
        validate_biome_name(&name)?;
        if output
            .insert(name.clone(), document.biome.components)
            .is_some()
        {
            return Err(invalid(format!("duplicate modern client biome {name}")));
        }
    }
    if output.is_empty() {
        return Err(invalid(
            "modern resource_pack/biomes contains no client biome files",
        ));
    }
    Ok(output)
}

fn read_behaviour_biomes(path: &Path) -> Result<BTreeMap<String, Climate>, AssetError> {
    let mut output = BTreeMap::new();
    for source in sorted_files(path, ".biome.json")? {
        let document: BehaviourBiomeDocument = read_json(&source)?;
        let name = document.biome.description.identifier;
        validate_biome_name(&name)?;
        if output
            .insert(name.clone(), document.biome.components.climate)
            .is_some()
        {
            return Err(invalid(format!("duplicate behaviour biome {name}")));
        }
    }
    if output.is_empty() {
        return Err(invalid("behavior_pack/biomes contains no biome files"));
    }
    Ok(output)
}

fn sorted_files(path: &Path, suffix: &str) -> Result<Vec<PathBuf>, AssetError> {
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
    if files.len() > MAX_BIOME_RULES {
        return Err(invalid(format!(
            "biome directory has {} files, exceeding {MAX_BIOME_RULES}",
            files.len()
        )));
    }
    Ok(files)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, AssetError> {
    let file = File::open(path).map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_JSON_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err(AssetError::JsonTooLarge {
            path: path.to_path_buf(),
            size: bytes.len(),
            max: MAX_JSON_BYTES,
        });
    }
    serde_json::from_slice(&bytes).map_err(|source| AssetError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn read_tint_maps(resource_pack: &Path) -> Result<Box<[u8]>, AssetError> {
    let mut output = Vec::with_capacity(TINT_MAP_BYTES);
    for map in TintMapId::ALL {
        let path = resource_pack
            .join("textures")
            .join("colormap")
            .join(format!("{}.png", map.source_name()));
        let file = File::open(&path).map_err(|source| AssetError::TextureIo {
            key: map.source_name().into(),
            path: path.clone(),
            source,
        })?;
        let mut bytes = Vec::new();
        file.take((MAX_COLORMAP_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|source| AssetError::TextureIo {
                key: map.source_name().into(),
                path: path.clone(),
                source,
            })?;
        if bytes.len() > MAX_COLORMAP_FILE_BYTES {
            return Err(AssetError::TextureTooLarge {
                key: map.source_name().into(),
                path,
                size: bytes.len(),
                max: MAX_COLORMAP_FILE_BYTES,
            });
        }
        let dimensions = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png)
            .into_dimensions()
            .map_err(|source| AssetError::TextureDecode {
                key: map.source_name().into(),
                path: path.clone(),
                source: Box::new(source),
            })?;
        if dimensions != (TINT_MAP_SIZE, TINT_MAP_SIZE) {
            return Err(invalid(format!(
                "tint map {} is {}x{}, expected 256x256",
                map.source_name(),
                dimensions.0,
                dimensions.1
            )));
        }
        let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
        let mut limits = Limits::default();
        limits.max_image_width = Some(TINT_MAP_SIZE);
        limits.max_image_height = Some(TINT_MAP_SIZE);
        limits.max_alloc = Some(512 * 1024);
        reader.limits(limits);
        let rgba = reader
            .decode()
            .map_err(|source| AssetError::TextureDecode {
                key: map.source_name().into(),
                path: path.clone(),
                source: Box::new(source),
            })?
            .into_rgba8();
        for pixel in rgba.pixels() {
            if pixel[3] != u8::MAX {
                return Err(invalid(format!(
                    "tint map {} contains non-opaque pixels",
                    map.source_name()
                )));
            }
            output.extend_from_slice(&pixel.0[..3]);
        }
    }
    debug_assert_eq!(output.len(), TINT_MAP_BYTES);
    Ok(output.into_boxed_slice())
}

enum ColourDomain {
    Grass,
    Foliage,
    DryFoliage,
}

fn parse_colour_source(value: &Value, domain: ColourDomain) -> Result<TintSource, AssetError> {
    if let Some(object) = value.as_object() {
        if object.len() != 1 {
            return Err(invalid(
                "tint colour-map object must contain only color_map",
            ));
        }
        let name = object
            .get("color_map")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("tint color_map must be a string"))?;
        let map = TintMapId::from_source_name(name)
            .ok_or_else(|| invalid(format!("unknown tint colormap {name}")))?;
        let valid = match domain {
            ColourDomain::Grass => matches!(map, TintMapId::Grass | TintMapId::SwampGrass),
            ColourDomain::Foliage => matches!(
                map,
                TintMapId::Foliage
                    | TintMapId::Birch
                    | TintMapId::Evergreen
                    | TintMapId::SwampFoliage
                    | TintMapId::MangroveSwampFoliage
                    | TintMapId::DryFoliage
            ),
            ColourDomain::DryFoliage => map == TintMapId::DryFoliage,
        };
        if !valid {
            return Err(invalid(format!(
                "colormap {name} is invalid for this tint domain"
            )));
        }
        return Ok(TintSource::map(map));
    }
    Ok(TintSource::direct(parse_direct_colour(value)?))
}

fn parse_direct_colour(value: &Value) -> Result<u32, AssetError> {
    if let Some(value) = value.as_str() {
        let digits = value
            .strip_prefix('#')
            .filter(|digits| digits.len() == 6)
            .ok_or_else(|| invalid(format!("invalid RGB colour {value}")))?;
        return u32::from_str_radix(digits, 16)
            .map_err(|_| invalid(format!("invalid RGB colour {value}")));
    }
    if let Some(channels) = value.as_array().filter(|channels| channels.len() == 3) {
        let mut rgb = 0_u32;
        for channel in channels {
            let channel = channel
                .as_u64()
                .filter(|channel| *channel <= u64::from(u8::MAX))
                .ok_or_else(|| invalid("RGB array channels must be integers in 0..=255"))?;
            rgb = (rgb << 8) | channel as u32;
        }
        return Ok(rgb);
    }
    Err(invalid("RGB colour must be #RRGGBB or a three-byte array"))
}

fn validate_biome_name(name: &str) -> Result<(), AssetError> {
    if name.is_empty() || name.len() > MAX_BIOME_NAME_BYTES {
        return Err(invalid(format!(
            "biome identifier length {} is outside 1..={MAX_BIOME_NAME_BYTES}",
            name.len()
        )));
    }
    Ok(())
}

fn validate_climate(temperature: f32, downfall: f32) -> Result<(), AssetError> {
    if !temperature.is_finite() || !downfall.is_finite() {
        return Err(invalid("biome climate contains a non-finite value"));
    }
    Ok(())
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

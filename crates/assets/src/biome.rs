use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Cursor, Read},
    path::{Path, PathBuf},
    str,
};

use image::{ImageFormat, ImageReader, Limits};
use serde::Deserialize;
use serde_json::Value;

use crate::AssetError;

pub const BIOME_REGISTRY_MAGIC: [u8; 8] = *b"BIOREG01";
pub const MAX_BIOME_RULES: usize = 1_024;
pub const MAX_BIOME_NAME_BYTES: usize = 256;
pub const MAX_BIOME_NAMES_BYTES: usize = 256 * 1_024;
pub const TINT_MAP_SIZE: u32 = 256;
pub const TINT_MAP_COUNT: usize = 8;
pub const TINT_MAP_BYTES: usize = TINT_MAP_COUNT * 256 * 256 * 3;
pub const BIOME_RULE_FLAG_GRASS_SHADED: u16 = 1;
pub const BIOME_RULE_FLAGS_MASK: u16 = BIOME_RULE_FLAG_GRASS_SHADED;
pub const RAW_BIOME_ID_COUNT: usize = u16::MAX as usize + 1;
pub const MISSING_BIOME_DENSE_INDEX: u32 = 0;

const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_COLORMAP_FILE_BYTES: usize = 1024 * 1024;

/// The eight built-in Bedrock tint maps in their stable v3 blob order.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TintMapId {
    Grass = 0,
    Foliage = 1,
    Birch = 2,
    Evergreen = 3,
    SwampGrass = 4,
    SwampFoliage = 5,
    MangroveSwampFoliage = 6,
    DryFoliage = 7,
}

impl TintMapId {
    pub const ALL: [Self; TINT_MAP_COUNT] = [
        Self::Grass,
        Self::Foliage,
        Self::Birch,
        Self::Evergreen,
        Self::SwampGrass,
        Self::SwampFoliage,
        Self::MangroveSwampFoliage,
        Self::DryFoliage,
    ];

    pub const fn source_name(self) -> &'static str {
        match self {
            Self::Grass => "grass",
            Self::Foliage => "foliage",
            Self::Birch => "birch",
            Self::Evergreen => "evergreen",
            Self::SwampGrass => "swamp_grass",
            Self::SwampFoliage => "swamp_foliage",
            Self::MangroveSwampFoliage => "mangrove_swamp_foliage",
            Self::DryFoliage => "dry_foliage",
        }
    }

    fn from_source_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.source_name() == name)
    }

    fn from_raw(value: u8) -> Option<Self> {
        Self::ALL.get(value as usize).copied()
    }
}

/// One stable Dragonfly biome-palette ID and canonical name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BiomeRegistryRecord {
    pub id: u32,
    pub name: Box<str>,
}

/// Reads the deterministic `BIOREG01` registry used by Bedrock biome palettes.
pub fn read_biome_registry(bytes: &[u8]) -> Result<Box<[BiomeRegistryRecord]>, AssetError> {
    let mut reader = Reader::new(bytes);
    if reader.read_exact(8, "biome registry magic")? != BIOME_REGISTRY_MAGIC {
        return Err(invalid("invalid BIOREG01 biome registry magic"));
    }
    let count = reader.read_u32("biome registry record count")? as usize;
    if count > MAX_BIOME_RULES {
        return Err(invalid(format!(
            "biome registry has {count} records, exceeding the limit of {MAX_BIOME_RULES}"
        )));
    }
    let mut records = Vec::with_capacity(count);
    let mut names = BTreeSet::new();
    let mut previous = None;
    for _ in 0..count {
        let id = reader.read_u32("biome registry ID")?;
        if id > u32::from(u16::MAX) {
            return Err(invalid(format!("biome registry ID {id} exceeds 65535")));
        }
        if previous.is_some_and(|previous| previous >= id) {
            return Err(invalid("biome registry IDs are not strictly increasing"));
        }
        let length = reader.read_u16("biome registry name length")? as usize;
        if length == 0 || length > MAX_BIOME_NAME_BYTES {
            return Err(invalid(format!(
                "biome registry name length {length} is outside 1..={MAX_BIOME_NAME_BYTES}"
            )));
        }
        let name = str::from_utf8(reader.read_exact(length, "biome registry name")?).map_err(
            |source| AssetError::InvalidRegistryUtf8 {
                field: "biome name",
                source,
            },
        )?;
        validate_biome_name(name)?;
        if !names.insert(name.to_owned()) {
            return Err(invalid(format!("duplicate biome registry name {name}")));
        }
        records.push(BiomeRegistryRecord {
            id,
            name: name.into(),
        });
        previous = Some(id);
    }
    if reader.remaining() != 0 {
        return Err(invalid(format!(
            "biome registry has {} trailing bytes",
            reader.remaining()
        )));
    }
    Ok(records.into_boxed_slice())
}

/// Compact direct-colour or colormap reference stored in a biome rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TintSource(u32);

impl TintSource {
    const DIRECT_TAG: u32 = 0;
    const MAP_TAG: u32 = 1;

    pub const fn direct(rgb: u32) -> Self {
        Self(rgb & 0x00ff_ffff)
    }

    pub const fn map(map: TintMapId) -> Self {
        Self((Self::MAP_TAG << 24) | map as u32)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub fn from_raw(raw: u32) -> Result<Self, AssetError> {
        let source = Self(raw);
        source.validate()?;
        Ok(source)
    }

    fn validate(self) -> Result<(), AssetError> {
        let tag = self.0 >> 24;
        match tag {
            Self::DIRECT_TAG => Ok(()),
            Self::MAP_TAG if self.0 & 0x00ff_ff00 == 0 => TintMapId::from_raw(self.0 as u8)
                .map(|_| ())
                .ok_or_else(|| invalid(format!("invalid tint map ID {}", self.0 as u8))),
            _ => Err(invalid(format!("invalid tint source {:#010x}", self.0))),
        }
    }

    fn direct_rgb(self) -> Option<u32> {
        (self.0 >> 24 == Self::DIRECT_TAG).then_some(self.0 & 0x00ff_ffff)
    }

    fn map_id(self) -> Option<TintMapId> {
        (self.0 >> 24 == Self::MAP_TAG)
            .then(|| TintMapId::from_raw(self.0 as u8))
            .flatten()
    }
}

/// One v3 biome colour rule keyed by stable palette ID and canonical name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BiomeRule {
    pub id: u32,
    pub name: Box<str>,
    pub flags: u16,
    pub grass: TintSource,
    pub foliage: TintSource,
    pub dry_foliage: TintSource,
    pub water: TintSource,
    pub temperature_bits: u32,
    pub downfall_bits: u32,
}

impl BiomeRule {
    pub fn temperature(&self) -> f32 {
        f32::from_bits(self.temperature_bits)
    }

    pub fn downfall(&self) -> f32 {
        f32::from_bits(self.downfall_bits)
    }
}

/// Runtime-independent biome payload serialized in `MCBEAS03`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledBiomeAssets {
    pub tint_maps_rgb8: Box<[u8]>,
    pub rules: Box<[BiomeRule]>,
}

impl CompiledBiomeAssets {
    /// Empty synthetic payload for focused block/material tests.
    pub fn diagnostic() -> Self {
        Self {
            tint_maps_rgb8: vec![u8::MAX; TINT_MAP_BYTES].into_boxed_slice(),
            rules: Box::new([]),
        }
    }
}

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
                source,
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
                source,
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

/// Protocol-independent live biome definition input.
#[derive(Clone, Copy, Debug)]
pub struct LiveBiomeDefinition<'a> {
    pub name: &'a str,
    pub biome_id: Option<u16>,
    pub temperature: f32,
    pub downfall: f32,
    /// Bedrock ARGB map-water colour.
    pub map_water_argb: u32,
}

/// One dense, linear-colour biome tint record ready for GPU packing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearBiomeTints {
    pub raw_id: u32,
    pub flags: u32,
    pub grass: [f32; 4],
    pub foliage: [f32; 4],
    pub birch: [f32; 4],
    pub evergreen: [f32; 4],
    pub water: [f32; 4],
    pub dry_foliage: [f32; 4],
}

/// Deterministic dense tint records and direct raw-palette-ID lookup.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedBiomeTints {
    pub records: Box<[LinearBiomeTints]>,
    pub raw_id_to_dense: Box<[u32]>,
}

impl ResolvedBiomeTints {
    /// Returns the dense tint-record index for one raw palette ID, or fallback slot zero.
    #[must_use]
    pub fn dense_index(&self, raw_id: u32) -> u32 {
        usize::try_from(raw_id)
            .ok()
            .and_then(|index| self.raw_id_to_dense.get(index))
            .copied()
            .unwrap_or(MISSING_BIOME_DENSE_INDEX)
    }
}

impl CompiledBiomeAssets {
    /// Resolves static and live definitions without depending on the protocol crate.
    pub fn resolve_live(
        &self,
        live: &[LiveBiomeDefinition<'_>],
    ) -> Result<ResolvedBiomeTints, AssetError> {
        validate_biome_assets(self)?;
        let by_name = self
            .rules
            .iter()
            .map(|rule| (rule.name.as_ref(), rule))
            .collect::<BTreeMap<_, _>>();
        let mut climates = BTreeMap::<u32, (f32, f32)>::new();
        let mut custom = BTreeMap::<u32, LiveBiomeDefinition<'_>>::new();
        for definition in live {
            if let Some(rule) = by_name.get(definition.name) {
                validate_biome_name(definition.name)?;
                validate_climate(definition.temperature, definition.downfall)?;
                if climates
                    .insert(rule.id, (definition.temperature, definition.downfall))
                    .is_some()
                {
                    return Err(invalid(format!(
                        "duplicate live biome definition {}",
                        definition.name
                    )));
                }
            } else if let Some(id) = definition.biome_id {
                validate_biome_name(definition.name)?;
                validate_climate(definition.temperature, definition.downfall)?;
                let id = u32::from(id);
                if by_name.values().any(|rule| rule.id == id)
                    || custom.insert(id, *definition).is_some()
                {
                    return Err(invalid(format!("duplicate custom biome ID {id}")));
                }
            }
        }

        let fallback = BiomeRule {
            id: u32::MAX,
            name: "fallback".into(),
            flags: 0,
            grass: TintSource::map(TintMapId::Grass),
            foliage: TintSource::map(TintMapId::Foliage),
            dry_foliage: TintSource::map(TintMapId::DryFoliage),
            water: TintSource::direct(0x44_aff5),
            temperature_bits: 0.8_f32.to_bits(),
            downfall_bits: 0.4_f32.to_bits(),
        };
        let mut records = Vec::with_capacity(self.rules.len() + custom.len() + 1);
        records.push(self.resolve_rule(&fallback, 0.8, 0.4)?);
        for rule in &self.rules {
            let (temperature, downfall) = climates
                .get(&rule.id)
                .copied()
                .unwrap_or((rule.temperature(), rule.downfall()));
            records.push(self.resolve_rule(rule, temperature, downfall)?);
        }
        for (id, definition) in custom {
            let fallback = BiomeRule {
                id,
                name: definition.name.into(),
                flags: 0,
                grass: TintSource::map(TintMapId::Grass),
                foliage: TintSource::map(TintMapId::Foliage),
                dry_foliage: TintSource::map(TintMapId::DryFoliage),
                water: TintSource::direct(definition.map_water_argb & 0x00ff_ffff),
                temperature_bits: definition.temperature.to_bits(),
                downfall_bits: definition.downfall.to_bits(),
            };
            records.push(self.resolve_rule(
                &fallback,
                definition.temperature,
                definition.downfall,
            )?);
        }
        records[1..].sort_unstable_by_key(|record| record.raw_id);
        let mut raw_id_to_dense = vec![MISSING_BIOME_DENSE_INDEX; RAW_BIOME_ID_COUNT];
        for (dense, record) in records.iter().enumerate().skip(1) {
            raw_id_to_dense[record.raw_id as usize] = dense as u32;
        }
        Ok(ResolvedBiomeTints {
            records: records.into_boxed_slice(),
            raw_id_to_dense: raw_id_to_dense.into_boxed_slice(),
        })
    }

    fn resolve_rule(
        &self,
        rule: &BiomeRule,
        temperature: f32,
        downfall: f32,
    ) -> Result<LinearBiomeTints, AssetError> {
        Ok(LinearBiomeTints {
            raw_id: rule.id,
            flags: u32::from(rule.flags),
            grass: self.resolve_source(rule.grass, temperature, downfall)?,
            foliage: self.resolve_source(rule.foliage, temperature, downfall)?,
            birch: self.resolve_source(TintSource::map(TintMapId::Birch), temperature, downfall)?,
            evergreen: self.resolve_source(
                TintSource::map(TintMapId::Evergreen),
                temperature,
                downfall,
            )?,
            water: self.resolve_source(rule.water, temperature, downfall)?,
            dry_foliage: self.resolve_source(rule.dry_foliage, temperature, downfall)?,
        })
    }

    fn resolve_source(
        &self,
        source: TintSource,
        temperature: f32,
        downfall: f32,
    ) -> Result<[f32; 4], AssetError> {
        let rgb = if let Some(rgb) = source.direct_rgb() {
            rgb
        } else {
            let map = source
                .map_id()
                .ok_or_else(|| invalid("invalid tint source at runtime"))?;
            let [x, y] = colormap_coordinate(temperature, downfall);
            let pixel = ((map as usize * 256 * 256) + (y as usize * 256 + x as usize)) * 3;
            let channels = self
                .tint_maps_rgb8
                .get(pixel..pixel + 3)
                .ok_or_else(|| invalid("tint map lookup exceeds validated storage"))?;
            (u32::from(channels[0]) << 16) | (u32::from(channels[1]) << 8) | u32::from(channels[2])
        };
        Ok(rgb_to_linear(rgb))
    }
}

pub fn colormap_coordinate(temperature: f32, downfall: f32) -> [u8; 2] {
    let temperature = temperature.clamp(0.0, 1.0);
    let humidity = downfall.clamp(0.0, 1.0) * temperature;
    [
        ((1.0 - temperature) * 255.0).floor() as u8,
        ((1.0 - humidity) * 255.0).floor() as u8,
    ]
}

fn rgb_to_linear(rgb: u32) -> [f32; 4] {
    let channel = |shift: u32| {
        let value = ((rgb >> shift) & 0xff_u32) as f32 / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    [channel(16), channel(8), channel(0), 1.0]
}

pub(crate) fn validate_biome_assets(assets: &CompiledBiomeAssets) -> Result<(), AssetError> {
    if assets.tint_maps_rgb8.len() != TINT_MAP_BYTES {
        return Err(invalid(format!(
            "tint map section has {} bytes, expected {TINT_MAP_BYTES}",
            assets.tint_maps_rgb8.len()
        )));
    }
    if assets.rules.len() > MAX_BIOME_RULES {
        return Err(invalid(format!(
            "biome rule count {} exceeds {MAX_BIOME_RULES}",
            assets.rules.len()
        )));
    }
    let mut previous = None;
    let mut names = BTreeSet::new();
    let mut name_bytes = 0_usize;
    for rule in &assets.rules {
        if rule.id > u32::from(u16::MAX) {
            return Err(invalid(format!("biome rule ID {} exceeds 65535", rule.id)));
        }
        if previous.is_some_and(|previous| previous >= rule.id) {
            return Err(invalid("biome rules are not strictly ordered by ID"));
        }
        validate_biome_name(&rule.name)?;
        if !names.insert(rule.name.as_ref()) {
            return Err(invalid(format!("duplicate biome rule name {}", rule.name)));
        }
        name_bytes =
            name_bytes
                .checked_add(rule.name.len())
                .ok_or(AssetError::BlobSizeOverflow {
                    section: "biome names",
                })?;
        if rule.flags & !BIOME_RULE_FLAGS_MASK != 0 {
            return Err(invalid(format!(
                "biome rule {} has unsupported flags {:#06x}",
                rule.id, rule.flags
            )));
        }
        for source in [rule.grass, rule.foliage, rule.dry_foliage, rule.water] {
            source.validate()?;
        }
        if rule.water.direct_rgb().is_none() {
            return Err(invalid(format!(
                "biome rule {} water is not direct RGB",
                rule.id
            )));
        }
        validate_climate(rule.temperature(), rule.downfall())?;
        previous = Some(rule.id);
    }
    if name_bytes > MAX_BIOME_NAMES_BYTES {
        return Err(invalid(format!(
            "biome names occupy {name_bytes} bytes, exceeding {MAX_BIOME_NAMES_BYTES}"
        )));
    }
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    const fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn read_exact(&mut self, count: usize, context: &'static str) -> Result<&'a [u8], AssetError> {
        if self.remaining() < count {
            return Err(AssetError::UnexpectedEof {
                context,
                needed: count,
                remaining: self.remaining(),
            });
        }
        let start = self.position;
        self.position += count;
        Ok(&self.bytes[start..self.position])
    }

    fn read_u16(&mut self, context: &'static str) -> Result<u16, AssetError> {
        Ok(u16::from_le_bytes(
            self.read_exact(2, context)?.try_into().expect("two bytes"),
        ))
    }

    fn read_u32(&mut self, context: &'static str) -> Result<u32, AssetError> {
        Ok(u32::from_le_bytes(
            self.read_exact(4, context)?.try_into().expect("four bytes"),
        ))
    }
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

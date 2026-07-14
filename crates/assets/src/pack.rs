use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::File,
    io::Read,
    marker::PhantomData,
    path::{Component, Path},
    str,
};

use serde::{
    Deserialize,
    de::{DeserializeOwned, IgnoredAny, MapAccess, Visitor},
};
use serde_json::{Map, Value};

use crate::{AssetError, RegistryRecord};

const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_TEXTURE_KEYS: usize = 8_192;
const MAX_TEXTURE_VARIANTS: usize = 256;
const MAX_TEXTURE_PATH_BYTES: usize = 4 * 1024;

/// Maximum number of flipbook selectors accepted from one resource pack.
pub const MAX_FLIPBOOKS: usize = MAX_TEXTURE_KEYS;
/// Maximum number of explicit frame indices retained for one flipbook.
pub const MAX_FLIPBOOK_FRAMES: usize = 4_096;

struct BoundedUniqueMap<V, const MAX: usize> {
    entries: BTreeMap<String, V>,
    issue: Option<BoundedMapIssue>,
}

enum BoundedMapIssue {
    Duplicate(Box<str>),
    TooMany { count: usize },
}

impl<'de, V, const MAX: usize> Deserialize<'de> for BoundedUniqueMap<V, MAX>
where
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(BoundedUniqueMapVisitor::<V, MAX>(PhantomData))
    }
}

struct BoundedUniqueMapVisitor<V, const MAX: usize>(PhantomData<V>);

impl<'de, V, const MAX: usize> Visitor<'de> for BoundedUniqueMapVisitor<V, MAX>
where
    V: Deserialize<'de>,
{
    type Value = BoundedUniqueMap<V, MAX>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object with unique bounded keys")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut entries = BTreeMap::new();
        let mut issue = None;
        let mut count = 0;
        while let Some(key) = map.next_key::<String>()? {
            count += 1;
            if issue.is_some() {
                map.next_value::<IgnoredAny>()?;
                continue;
            }
            if entries.contains_key(&key) {
                map.next_value::<IgnoredAny>()?;
                issue = Some(BoundedMapIssue::Duplicate(key.into_boxed_str()));
                continue;
            }
            if count > MAX {
                map.next_value::<IgnoredAny>()?;
                issue = Some(BoundedMapIssue::TooMany { count });
                continue;
            }
            entries.insert(key, map.next_value()?);
        }
        if let Some(BoundedMapIssue::TooMany { count: issue_count }) = &mut issue {
            *issue_count = count;
        }
        Ok(BoundedUniqueMap { entries, issue })
    }
}

/// Bedrock block-face order, matching the packed renderer's face discriminants.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockFace {
    West = 0,
    East = 1,
    Down = 2,
    Up = 3,
    North = 4,
    South = 5,
}

impl BlockFace {
    pub const ALL: [Self; 6] = [
        Self::West,
        Self::East,
        Self::Down,
        Self::Up,
        Self::North,
        Self::South,
    ];

    const fn is_horizontal(self) -> bool {
        matches!(self, Self::West | Self::East | Self::North | Self::South)
    }
}

/// A source texture key and the pillar UV transform needed for the face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureKey {
    /// `None` selects the diagnostic material.
    pub key: Option<Box<str>>,
    /// Rotate UVs by one quarter turn for a horizontal pillar side.
    pub rotate_uv: bool,
}

impl TextureKey {
    fn diagnostic() -> Self {
        Self {
            key: None,
            rotate_uv: false,
        }
    }

    fn resolved(key: &str, rotate_uv: bool) -> Self {
        Self {
            key: Some(key.into()),
            rotate_uv,
        }
    }
}

/// Opaque block-name to texture-description map.
#[derive(Debug)]
pub struct BlockTextureMap {
    entries: BTreeMap<Box<str>, TextureValue>,
}

/// Bounded terrain path variants indexed by texture key.
#[derive(Debug)]
pub struct TerrainTextureMap {
    entries: BTreeMap<Box<str>, TerrainPaths>,
}

impl TerrainTextureMap {
    /// Returns the deterministic variant-zero path used for terrain arrays
    /// without a documented block-state selector.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(TerrainPaths::first)
    }

    /// Returns exact array indices 0 and 1 only when the terrain key has
    /// exactly two variants.
    #[must_use]
    pub fn get_exact_pair(&self, key: &str) -> Option<[&str; 2]> {
        match self.entries.get(key)? {
            TerrainPaths::Variants { paths, .. } if paths.len() == 2 => {
                Some([paths[0].as_ref(), paths[1].as_ref()])
            }
            TerrainPaths::Static { .. } | TerrainPaths::Variants { .. } => None,
        }
    }

    /// Returns an exact two-variant route only when neither source carries
    /// overlay tint metadata.
    #[must_use]
    pub(crate) fn get_exact_pair_no_tint(&self, key: &str) -> Option<[&str; 2]> {
        match self.entries.get(key)? {
            TerrainPaths::Variants {
                paths,
                requires_tint: false,
            } if paths.len() == 2 => Some([paths[0].as_ref(), paths[1].as_ref()]),
            TerrainPaths::Static { .. } | TerrainPaths::Variants { .. } => None,
        }
    }

    /// Returns an exact static route only when it carries no overlay tint
    /// metadata.
    #[must_use]
    pub(crate) fn get_exact_static_no_tint(&self, key: &str) -> Option<&str> {
        match self.entries.get(key)? {
            TerrainPaths::Static {
                path,
                requires_tint: false,
            } => Some(path),
            TerrainPaths::Static { .. } | TerrainPaths::Variants { .. } => None,
        }
    }

    pub(crate) fn requires_tint(&self, key: &str) -> bool {
        self.entries
            .get(key)
            .is_some_and(TerrainPaths::requires_tint)
    }

    pub(crate) fn get_for_record(&self, key: &str, record: &RegistryRecord) -> Option<&str> {
        let paths = self.entries.get(key)?;
        if !is_mushroom_face_key(key, &record.name) {
            return Some(paths.first());
        }
        let selected = mushroom_variant_index(record)?;
        match paths {
            TerrainPaths::Static { path, .. } => Some(path),
            TerrainPaths::Variants { paths, .. } if paths.len() == 16 => {
                paths.get(selected).map(AsRef::as_ref)
            }
            TerrainPaths::Variants { .. } => None,
        }
    }

    /// Resolves an immutable model material and records its selected terrain
    /// array index for deterministic state-to-variant inspection.
    pub(crate) fn get_for_model_record(
        &self,
        key: &str,
        record: &RegistryRecord,
    ) -> Option<(&str, u32)> {
        let paths = self.entries.get(key)?;
        match paths {
            TerrainPaths::Static { path, .. } => Some((path, 0)),
            TerrainPaths::Variants { paths, .. } => {
                let selected = if is_mushroom_face_key(key, &record.name) {
                    mushroom_variant_index(record).filter(|_| paths.len() == 16)?
                } else {
                    model_variant_index(key, record, paths.len())?
                };
                paths
                    .get(selected)
                    .map(|path| (path.as_ref(), selected as u32))
            }
        }
    }

    pub(crate) fn source_paths(&self) -> impl Iterator<Item = &str> {
        self.entries.values().flat_map(TerrainPaths::paths)
    }
}

impl BlockTextureMap {
    /// Returns a block texture key only when the source uses the scalar form.
    #[must_use]
    pub(crate) fn get_exact_scalar(&self, block_name: &str) -> Option<&str> {
        match self.entries.get(block_name)? {
            TextureValue::Key(key) => Some(key),
            TextureValue::Faces(_) => None,
        }
    }

    /// Returns six explicit face keys. Fallback `side` routing is deliberately
    /// excluded so exact model families cannot accept an underspecified map.
    pub(crate) fn get_exact_faces(&self, block_name: &str) -> Option<[&str; 6]> {
        let TextureValue::Faces(faces) = self.entries.get(block_name)? else {
            return None;
        };
        if faces.side.is_some() {
            return None;
        }
        Some([
            faces.west.as_deref()?,
            faces.east.as_deref()?,
            faces.down.as_deref()?,
            faces.up.as_deref()?,
            faces.north.as_deref()?,
            faces.south.as_deref()?,
        ])
    }
}

#[derive(Debug)]
enum TerrainPaths {
    Static {
        path: Box<str>,
        requires_tint: bool,
    },
    Variants {
        paths: Box<[Box<str>]>,
        requires_tint: bool,
    },
}

impl TerrainPaths {
    fn first(&self) -> &str {
        match self {
            Self::Static { path, .. } => path,
            Self::Variants { paths, .. } => &paths[0],
        }
    }

    const fn requires_tint(&self) -> bool {
        match self {
            Self::Static { requires_tint, .. } | Self::Variants { requires_tint, .. } => {
                *requires_tint
            }
        }
    }

    fn paths(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            Self::Static { path, .. } => Box::new(std::iter::once(path.as_ref())),
            Self::Variants { paths, .. } => Box::new(paths.iter().map(AsRef::as_ref)),
        }
    }
}

/// Flipbook metadata needed to keep animated strips out of static layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlipbookSource {
    pub texture_path: Box<str>,
    pub atlas_tile: Box<str>,
    pub ticks_per_frame: u32,
    pub frames: Box<[u32]>,
    pub atlas_index: u32,
    pub atlas_tile_variant: u32,
    pub replicate: u32,
    pub blend_frames: bool,
}

/// Parsed vanilla pack sources used by the later deterministic compiler.
#[derive(Debug)]
pub struct PackSources {
    pub blocks: BlockTextureMap,
    pub terrain: TerrainTextureMap,
    pub flipbooks: Box<[FlipbookSource]>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum TextureValue {
    Key(String),
    Faces(FaceKeys),
}

impl TextureValue {
    fn try_for_each_key<E>(&self, mut visitor: impl FnMut(&str) -> Result<(), E>) -> Result<(), E> {
        match self {
            Self::Key(key) => visitor(key),
            Self::Faces(faces) => {
                for key in faces.keys().into_iter().flatten() {
                    visitor(key)?;
                }
                Ok(())
            }
        }
    }

    fn has_keys(&self) -> bool {
        match self {
            Self::Key(key) => !key.is_empty(),
            Self::Faces(faces) => faces.keys().into_iter().flatten().next().is_some(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct FaceKeys {
    west: Option<String>,
    east: Option<String>,
    down: Option<String>,
    up: Option<String>,
    north: Option<String>,
    south: Option<String>,
    side: Option<String>,
}

impl FaceKeys {
    fn keys(&self) -> [Option<&str>; 7] {
        [
            self.west.as_deref(),
            self.east.as_deref(),
            self.down.as_deref(),
            self.up.as_deref(),
            self.north.as_deref(),
            self.south.as_deref(),
            self.side.as_deref(),
        ]
    }

    fn explicit(&self, face: BlockFace) -> Option<&str> {
        match face {
            BlockFace::West => self.west.as_deref(),
            BlockFace::East => self.east.as_deref(),
            BlockFace::Down => self.down.as_deref(),
            BlockFace::Up => self.up.as_deref(),
            BlockFace::North => self.north.as_deref(),
            BlockFace::South => self.south.as_deref(),
        }
    }

    fn resolve(&self, face: BlockFace) -> Option<&str> {
        self.explicit(face).or_else(|| {
            if face.is_horizontal() {
                self.side.as_deref()
            } else {
                None
            }
        })
    }
}

#[derive(Deserialize)]
struct BlockEntry {
    #[serde(default)]
    textures: Option<TextureValue>,
}

#[derive(Deserialize)]
struct TerrainDocument {
    texture_data: BoundedUniqueMap<TerrainEntry, MAX_TEXTURE_KEYS>,
}

#[derive(Deserialize)]
struct TerrainEntry {
    textures: TerrainValue,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TerrainValue {
    Path(String),
    Entry {
        path: String,
        overlay_color: Option<String>,
    },
    Variants(Vec<TerrainVariant>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TerrainVariant {
    Path(String),
    Entry {
        path: String,
        overlay_color: Option<String>,
    },
}

impl TerrainVariant {
    fn into_path_and_tint(self) -> (String, bool) {
        match self {
            Self::Path(path) => (path, false),
            Self::Entry {
                path,
                overlay_color,
            } => (path, overlay_color.is_some()),
        }
    }
}

#[derive(Deserialize)]
struct RawFlipbook {
    flipbook_texture: String,
    atlas_tile: String,
    #[serde(default)]
    ticks_per_frame: PresentValue,
    #[serde(default)]
    frames: PresentValue,
    #[serde(default)]
    atlas_index: PresentValue,
    #[serde(default)]
    atlas_tile_variant: PresentValue,
    #[serde(default)]
    replicate: PresentValue,
    #[serde(default)]
    blend_frames: PresentValue,
}

#[derive(Default)]
enum PresentValue {
    #[default]
    Missing,
    Present(Value),
}

impl<'de> Deserialize<'de> for PresentValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Value::deserialize(deserializer).map(Self::Present)
    }
}

/// Reads the bounded JSON source subset needed by the vanilla texture compiler.
pub fn read_pack(root: &Path) -> Result<PackSources, AssetError> {
    let blocks_path = root.join("blocks.json");
    let terrain_path = root.join("textures/terrain_texture.json");
    let flipbooks_path = root.join("textures/flipbook_textures.json");

    let blocks = read_blocks(&blocks_path)?;
    let terrain = read_terrain(&terrain_path)?;
    validate_block_keys(&blocks, &terrain)?;
    let flipbooks = read_flipbooks(&flipbooks_path, &terrain)?;

    Ok(PackSources {
        blocks,
        terrain,
        flipbooks,
    })
}

/// Resolves one block face to a vanilla terrain key or the diagnostic marker.
#[must_use]
pub fn resolve_texture_key(
    blocks: &BlockTextureMap,
    record: &RegistryRecord,
    face: BlockFace,
) -> TextureKey {
    let block_name = record
        .name
        .strip_prefix("minecraft:")
        .unwrap_or(&record.name);
    let resource_pack_name = if blocks.entries.contains_key(block_name) {
        block_name
    } else {
        legacy_resource_pack_block_alias(block_name).unwrap_or(block_name)
    };
    let Some(texture) = blocks.entries.get(resource_pack_name) else {
        return TextureKey::diagnostic();
    };

    match texture {
        TextureValue::Key(key) => TextureKey::resolved(key, false),
        TextureValue::Faces(faces) => {
            let (source_face, rotate_uv) = orient_face(face, state_axis(&record.canonical_state));
            faces
                .resolve(source_face)
                .map_or_else(TextureKey::diagnostic, |key| {
                    TextureKey::resolved(key, rotate_uv)
                })
        }
    }
}

fn legacy_resource_pack_block_alias(block_name: &str) -> Option<&'static str> {
    match block_name {
        "grass_block" => Some("grass"),
        "sea_lantern" => Some("seaLantern"),
        "dandelion" => Some("yellow_flower"),
        "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "red_tulip" | "orange_tulip"
        | "white_tulip" | "pink_tulip" | "oxeye_daisy" | "cornflower" | "lily_of_the_valley" => {
            Some("red_flower")
        }
        "oak_sapling" | "spruce_sapling" | "birch_sapling" | "jungle_sapling"
        | "acacia_sapling" | "dark_oak_sapling" => Some("sapling"),
        "hard_glass_pane" => Some("glass_pane"),
        "hard_black_stained_glass_pane" => Some("black_stained_glass_pane"),
        "hard_blue_stained_glass_pane" => Some("blue_stained_glass_pane"),
        "hard_brown_stained_glass_pane" => Some("brown_stained_glass_pane"),
        "hard_cyan_stained_glass_pane" => Some("cyan_stained_glass_pane"),
        "hard_gray_stained_glass_pane" => Some("gray_stained_glass_pane"),
        "hard_green_stained_glass_pane" => Some("green_stained_glass_pane"),
        "hard_light_blue_stained_glass_pane" => Some("light_blue_stained_glass_pane"),
        "hard_light_gray_stained_glass_pane" => Some("light_gray_stained_glass_pane"),
        "hard_lime_stained_glass_pane" => Some("lime_stained_glass_pane"),
        "hard_magenta_stained_glass_pane" => Some("magenta_stained_glass_pane"),
        "hard_orange_stained_glass_pane" => Some("orange_stained_glass_pane"),
        "hard_pink_stained_glass_pane" => Some("pink_stained_glass_pane"),
        "hard_purple_stained_glass_pane" => Some("purple_stained_glass_pane"),
        "hard_red_stained_glass_pane" => Some("red_stained_glass_pane"),
        "hard_white_stained_glass_pane" => Some("white_stained_glass_pane"),
        "hard_yellow_stained_glass_pane" => Some("yellow_stained_glass_pane"),
        _ => None,
    }
}

fn model_variant_index(key: &str, record: &RegistryRecord, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let name = record
        .name
        .strip_prefix("minecraft:")
        .unwrap_or(&record.name);
    let selected = match key {
        "door_lower" | "door_upper" => match name {
            "wooden_door" => 0,
            "spruce_door" => 1,
            "birch_door" => 2,
            "jungle_door" => 3,
            "acacia_door" => 4,
            "dark_oak_door" => 5,
            "iron_door" => 6,
            _ => return None,
        },
        "red_flower" => match name {
            "poppy" => 0,
            "blue_orchid" => 1,
            "allium" => 2,
            "azure_bluet" => 3,
            "red_tulip" => 4,
            "orange_tulip" => 5,
            "white_tulip" => 6,
            "pink_tulip" => 7,
            "oxeye_daisy" => 8,
            "cornflower" => 9,
            "lily_of_the_valley" => 10,
            _ => canonical_u32(&record.canonical_state, "flower_type")? as usize,
        },
        "sapling" => match name {
            "oak_sapling" => 0,
            "spruce_sapling" => 1,
            "birch_sapling" => 2,
            "jungle_sapling" => 3,
            "acacia_sapling" => 4,
            "dark_oak_sapling" => 5,
            _ => sapling_variant(canonical_string(&record.canonical_state, "sapling_type")?)?,
        },
        "wheat" => canonical_u32(&record.canonical_state, "growth")? as usize,
        "melon_stem" | "pumpkin_stem" => {
            usize::from(canonical_u32(&record.canonical_state, "facing_direction")? >= 2)
        }
        "carrots" | "potatoes" | "beetroot" => {
            const STAGES: [usize; 8] = [0, 0, 1, 1, 2, 2, 2, 3];
            *STAGES.get(canonical_u32(&record.canonical_state, "growth")? as usize)?
        }
        "torchflower_crop" => usize::from(canonical_u32(&record.canonical_state, "growth")? >= 4),
        _ => {
            let growth = canonical_u32(&record.canonical_state, "growth")
                .or_else(|| canonical_u32(&record.canonical_state, "growth_stage"))
                .or_else(|| canonical_u32(&record.canonical_state, "age"));
            growth.map_or(0, |growth| {
                if count >= 8 {
                    growth as usize
                } else {
                    (growth as usize).saturating_mul(count) / 8
                }
            })
        }
    };
    (selected < count).then_some(selected)
}

fn canonical_u32(state: &str, property: &str) -> Option<u32> {
    let document = serde_json::from_str::<Map<String, Value>>(state).ok()?;
    canonical_value(document.get(property)?)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
}

fn canonical_string<'a>(state: &'a str, property: &str) -> Option<&'a str> {
    let document = serde_json::from_str::<Map<String, Value>>(state).ok()?;
    let value = canonical_value(document.get(property)?)?
        .as_str()?
        .to_owned();
    // This helper cannot return data owned by the parsed document, so the only
    // legacy generic selector is handled by its stable canonical spelling.
    match value.as_str() {
        "oak" => Some("oak"),
        "spruce" => Some("spruce"),
        "birch" => Some("birch"),
        "jungle" => Some("jungle"),
        "acacia" => Some("acacia"),
        "dark_oak" => Some("dark_oak"),
        "roofed_oak" => Some("roofed_oak"),
        _ => None,
    }
}

fn canonical_value(value: &Value) -> Option<&Value> {
    value
        .as_object()
        .and_then(|object| object.get("value"))
        .or(Some(value))
}

fn sapling_variant(name: &str) -> Option<usize> {
    Some(match name {
        "oak" => 0,
        "spruce" => 1,
        "birch" => 2,
        "jungle" => 3,
        "acacia" => 4,
        "dark_oak" | "roofed_oak" => 5,
        _ => return None,
    })
}

fn read_blocks(path: &Path) -> Result<BlockTextureMap, AssetError> {
    let document: BoundedUniqueMap<Value, { MAX_TEXTURE_KEYS + 1 }> = read_json(path, false)?;
    let mut document = match document.issue {
        Some(BoundedMapIssue::Duplicate(key)) => {
            return Err(AssetError::DuplicateBlockKey {
                path: path.to_path_buf(),
                key,
            });
        }
        Some(BoundedMapIssue::TooMany { count }) => {
            return Err(AssetError::TooManyTextureKeys {
                count,
                max: MAX_TEXTURE_KEYS,
            });
        }
        None => document.entries,
    };
    document.remove("format_version");
    if document.len() > MAX_TEXTURE_KEYS {
        return Err(AssetError::TooManyTextureKeys {
            count: document.len(),
            max: MAX_TEXTURE_KEYS,
        });
    }

    let mut entries = BTreeMap::new();
    for (name, value) in document {
        let entry: BlockEntry =
            serde_json::from_value(value).map_err(|source| AssetError::InvalidBlockEntry {
                path: path.to_path_buf(),
                block: name.clone().into_boxed_str(),
                source,
            })?;
        let Some(textures) = entry.textures else {
            continue;
        };
        if !textures.has_keys() {
            return Err(AssetError::MissingBlockTextureKeys(name.into_boxed_str()));
        }
        entries.insert(name.into_boxed_str(), textures);
    }
    Ok(BlockTextureMap { entries })
}

fn read_terrain(path: &Path) -> Result<TerrainTextureMap, AssetError> {
    let document: TerrainDocument = read_json(path, true)?;
    let texture_data = match document.texture_data.issue {
        Some(BoundedMapIssue::Duplicate(key)) => {
            return Err(AssetError::DuplicateTerrainTextureKey {
                path: path.to_path_buf(),
                key,
            });
        }
        Some(BoundedMapIssue::TooMany { count }) => {
            return Err(AssetError::TooManyTextureKeys {
                count,
                max: MAX_TEXTURE_KEYS,
            });
        }
        None => document.texture_data.entries,
    };

    let mut entries = BTreeMap::new();
    for (key, entry) in texture_data {
        let variants = collect_terrain_paths(&key, entry.textures)?;
        entries.insert(key.into_boxed_str(), variants);
    }
    Ok(TerrainTextureMap { entries })
}

fn collect_terrain_paths(key: &str, value: TerrainValue) -> Result<TerrainPaths, AssetError> {
    match value {
        TerrainValue::Path(path) => {
            validate_texture_path(&path)?;
            Ok(TerrainPaths::Static {
                path: path.into_boxed_str(),
                requires_tint: false,
            })
        }
        TerrainValue::Entry {
            path,
            overlay_color,
        } => {
            validate_texture_path(&path)?;
            Ok(TerrainPaths::Static {
                path: path.into_boxed_str(),
                requires_tint: overlay_color.is_some(),
            })
        }
        TerrainValue::Variants(variants) => {
            if variants.len() > MAX_TEXTURE_VARIANTS {
                return Err(AssetError::TooManyTextureVariants {
                    key: key.into(),
                    count: variants.len(),
                    max: MAX_TEXTURE_VARIANTS,
                });
            }
            let mut paths = Vec::with_capacity(variants.len());
            let mut requires_tint = false;
            for variant in variants {
                let (path, variant_requires_tint) = variant.into_path_and_tint();
                validate_texture_path(&path)?;
                paths.push(path.into_boxed_str());
                requires_tint |= variant_requires_tint;
            }
            if paths.is_empty() {
                return Err(AssetError::EmptyTextureVariants(key.into()));
            }
            Ok(TerrainPaths::Variants {
                paths: paths.into_boxed_slice(),
                requires_tint,
            })
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HugeMushroomState {
    huge_mushroom_bits: HugeMushroomSelector,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HugeMushroomSelector {
    #[serde(rename = "type")]
    kind: Box<str>,
    value: u8,
}

fn mushroom_variant_index(record: &RegistryRecord) -> Option<usize> {
    let state = serde_json::from_str::<HugeMushroomState>(&record.canonical_state).ok()?;
    (state.huge_mushroom_bits.kind.as_ref() == "int")
        .then_some(usize::from(state.huge_mushroom_bits.value))
        .filter(|&bits| bits <= 15)
}

fn is_mushroom_face_key(key: &str, block_name: &str) -> bool {
    let prefix = match block_name {
        "minecraft:brown_mushroom_block" => "mushroom_brown_",
        "minecraft:red_mushroom_block" => "mushroom_red_",
        "minecraft:mushroom_stem" => "mushroom_stem_",
        _ => return false,
    };
    key.strip_prefix(prefix)
        .is_some_and(|face| matches!(face, "west" | "east" | "bottom" | "top" | "north" | "south"))
}

fn read_flipbooks(
    path: &Path,
    terrain: &TerrainTextureMap,
) -> Result<Box<[FlipbookSource]>, AssetError> {
    let raw: Vec<RawFlipbook> = read_json(path, true)?;
    if raw.len() > MAX_FLIPBOOKS {
        return Err(AssetError::TooManyFlipbooks {
            count: raw.len(),
            max: MAX_FLIPBOOKS,
        });
    }

    let mut flipbooks = Vec::with_capacity(raw.len());
    let mut selectors = BTreeSet::new();
    for (index, entry) in raw.into_iter().enumerate() {
        validate_texture_path(&entry.flipbook_texture)?;
        if !terrain.entries.contains_key(entry.atlas_tile.as_str()) {
            return Err(AssetError::MissingTerrainKey {
                block: "flipbook atlas_tile".into(),
                key: entry.atlas_tile.into_boxed_str(),
            });
        }

        let ticks_per_frame = flipbook_u32(entry.ticks_per_frame, 1, index, "ticks_per_frame")?;
        let atlas_index = flipbook_u32(entry.atlas_index, 0, index, "atlas_index")?;
        let atlas_tile_variant =
            flipbook_u32(entry.atlas_tile_variant, 0, index, "atlas_tile_variant")?;
        let replicate = flipbook_u32(entry.replicate, 1, index, "replicate")?;
        for (field, value) in [
            ("ticks_per_frame", ticks_per_frame),
            ("replicate", replicate),
        ] {
            if value == 0 {
                return Err(AssetError::ZeroFlipbookValue { index, field });
            }
        }
        let frames = flipbook_frames(entry.frames, index)?;
        let blend_frames = flipbook_bool(entry.blend_frames, false, index, "blend_frames")?;

        let frame_count =
            u32::try_from(frames.len().max(1)).expect("bounded flipbook frame count fits u32");
        if frame_count.checked_mul(ticks_per_frame).is_none() {
            return Err(AssetError::FlipbookTimelineOverflow { index });
        }

        let atlas_tile = entry.atlas_tile.into_boxed_str();
        if !selectors.insert((atlas_tile.clone(), atlas_index, atlas_tile_variant)) {
            return Err(AssetError::DuplicateFlipbookSelector {
                atlas_tile,
                atlas_index,
                atlas_tile_variant,
            });
        }
        flipbooks.push(FlipbookSource {
            texture_path: entry.flipbook_texture.into_boxed_str(),
            atlas_tile,
            ticks_per_frame,
            frames,
            atlas_index,
            atlas_tile_variant,
            replicate,
            blend_frames,
        });
    }
    Ok(flipbooks.into_boxed_slice())
}

fn flipbook_u32(
    value: PresentValue,
    default: u32,
    index: usize,
    field: &'static str,
) -> Result<u32, AssetError> {
    let value = match value {
        PresentValue::Missing => return Ok(default),
        PresentValue::Present(value) => value,
    };
    let Value::Number(number) = value else {
        return Err(AssetError::InvalidFlipbookFieldType {
            index,
            field,
            expected: "unsigned 32-bit integer",
        });
    };
    number
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(AssetError::InvalidFlipbookInteger {
            index,
            field,
            element: None,
        })
}

fn flipbook_frames(value: PresentValue, index: usize) -> Result<Box<[u32]>, AssetError> {
    let value = match value {
        PresentValue::Missing => return Ok(Box::default()),
        PresentValue::Present(value) => value,
    };
    let Value::Array(values) = value else {
        return Err(AssetError::InvalidFlipbookFieldType {
            index,
            field: "frames",
            expected: "array of unsigned 32-bit integers",
        });
    };
    if values.len() > MAX_FLIPBOOK_FRAMES {
        return Err(AssetError::TooManyFlipbookFrames {
            index,
            count: values.len(),
            max: MAX_FLIPBOOK_FRAMES,
        });
    }

    let mut frames = Vec::with_capacity(values.len());
    for (element, value) in values.into_iter().enumerate() {
        let frame = match value {
            Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
            _ => None,
        }
        .ok_or(AssetError::InvalidFlipbookInteger {
            index,
            field: "frames",
            element: Some(element),
        })?;
        frames.push(frame);
    }
    Ok(frames.into_boxed_slice())
}

fn flipbook_bool(
    value: PresentValue,
    default: bool,
    index: usize,
    field: &'static str,
) -> Result<bool, AssetError> {
    match value {
        PresentValue::Missing => Ok(default),
        PresentValue::Present(Value::Bool(value)) => Ok(value),
        PresentValue::Present(_) => Err(AssetError::InvalidFlipbookFieldType {
            index,
            field,
            expected: "boolean",
        }),
    }
}

fn validate_block_keys(
    blocks: &BlockTextureMap,
    terrain: &TerrainTextureMap,
) -> Result<(), AssetError> {
    for (block, texture) in &blocks.entries {
        texture.try_for_each_key(|key| {
            if terrain.entries.contains_key(key) {
                Ok(())
            } else {
                Err(AssetError::MissingTerrainKey {
                    block: block.clone(),
                    key: key.into(),
                })
            }
        })?;
    }
    Ok(())
}

fn validate_texture_path(path: &str) -> Result<(), AssetError> {
    if path.len() > MAX_TEXTURE_PATH_BYTES {
        return Err(AssetError::TexturePathTooLong {
            path: path.into(),
            length: path.len(),
            max: MAX_TEXTURE_PATH_BYTES,
        });
    }

    let source_path = Path::new(path);
    let bytes = path.as_bytes();
    let has_windows_drive_prefix =
        bytes.get(1) == Some(&b':') && bytes.first().is_some_and(u8::is_ascii_alphabetic);
    let has_portable_root = path.starts_with(['/', '\\']);
    let has_portable_parent = path.split(['/', '\\']).any(|component| component == "..");
    let unsafe_component = source_path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if path.is_empty()
        || source_path.is_absolute()
        || has_windows_drive_prefix
        || has_portable_root
        || has_portable_parent
        || unsafe_component
    {
        return Err(AssetError::UnsafeTexturePath { path: path.into() });
    }
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path, strip_comments: bool) -> Result<T, AssetError> {
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

    let text = str::from_utf8(&bytes).map_err(|source| AssetError::InvalidJsonUtf8 {
        path: path.to_path_buf(),
        source,
    })?;
    let text = if strip_comments {
        strip_leading_comment_lines(text)
    } else {
        text
    };
    serde_json::from_str(text).map_err(|source| AssetError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn strip_leading_comment_lines(input: &str) -> &str {
    let mut offset = 0;
    for line in input.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let trimmed = body.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            offset += line.len();
        } else {
            break;
        }
    }
    &input[offset..]
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
    Z,
}

fn state_axis(canonical_state: &str) -> Axis {
    let Ok(properties) = serde_json::from_str::<Map<String, Value>>(canonical_state) else {
        return Axis::Y;
    };
    match properties
        .get("pillar_axis")
        .or_else(|| properties.get("axis"))
        .and_then(Value::as_str)
    {
        Some("x") => Axis::X,
        Some("z") => Axis::Z,
        _ => Axis::Y,
    }
}

fn orient_face(face: BlockFace, axis: Axis) -> (BlockFace, bool) {
    match (axis, face) {
        (Axis::X, BlockFace::West) => (BlockFace::Down, false),
        (Axis::X, BlockFace::East) => (BlockFace::Up, false),
        (Axis::X, BlockFace::Down) => (BlockFace::East, true),
        (Axis::X, BlockFace::Up) => (BlockFace::West, true),
        (Axis::X, other) => (other, true),
        (Axis::Z, BlockFace::North) => (BlockFace::Down, false),
        (Axis::Z, BlockFace::South) => (BlockFace::Up, false),
        (Axis::Z, BlockFace::Down) => (BlockFace::South, true),
        (Axis::Z, BlockFace::Up) => (BlockFace::North, true),
        (Axis::Z, other) => (other, true),
        (Axis::Y, other) => (other, false),
    }
}

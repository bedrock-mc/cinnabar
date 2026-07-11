use std::{
    collections::BTreeMap,
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

/// Deterministically selected terrain paths indexed by texture key.
#[derive(Debug)]
pub struct TerrainTextureMap {
    entries: BTreeMap<Box<str>, Box<str>>,
}

impl TerrainTextureMap {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(AsRef::as_ref)
    }
}

/// Flipbook metadata needed to keep animated strips out of static layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlipbookSource {
    pub texture_path: Box<str>,
    pub atlas_tile: Box<str>,
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
    fn into_path(self) -> String {
        match self {
            Self::Path(path) => path,
            Self::Entry {
                path,
                overlay_color,
            } => {
                drop(overlay_color);
                path
            }
        }
    }
}

#[derive(Deserialize)]
struct RawFlipbook {
    flipbook_texture: String,
    atlas_tile: String,
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
    let Some(texture) = blocks.entries.get(block_name) else {
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
        let selected = select_terrain_path(&key, entry.textures)?;
        entries.insert(key.into_boxed_str(), selected.into_boxed_str());
    }
    Ok(TerrainTextureMap { entries })
}

fn select_terrain_path(key: &str, value: TerrainValue) -> Result<String, AssetError> {
    match value {
        TerrainValue::Path(path) => {
            validate_texture_path(&path)?;
            Ok(path)
        }
        TerrainValue::Entry {
            path,
            overlay_color,
        } => {
            drop(overlay_color);
            validate_texture_path(&path)?;
            Ok(path)
        }
        TerrainValue::Variants(variants) => {
            if variants.len() > MAX_TEXTURE_VARIANTS {
                return Err(AssetError::TooManyTextureVariants {
                    key: key.into(),
                    count: variants.len(),
                    max: MAX_TEXTURE_VARIANTS,
                });
            }
            let mut selected = None;
            for variant in variants {
                let path = variant.into_path();
                validate_texture_path(&path)?;
                if selected.is_none() {
                    selected = Some(path);
                }
            }
            selected.ok_or_else(|| AssetError::EmptyTextureVariants(key.into()))
        }
    }
}

fn read_flipbooks(
    path: &Path,
    terrain: &TerrainTextureMap,
) -> Result<Box<[FlipbookSource]>, AssetError> {
    let raw: Vec<RawFlipbook> = read_json(path, true)?;
    if raw.len() > MAX_TEXTURE_KEYS {
        return Err(AssetError::TooManyFlipbooks {
            count: raw.len(),
            max: MAX_TEXTURE_KEYS,
        });
    }

    let mut flipbooks = Vec::with_capacity(raw.len());
    for entry in raw {
        validate_texture_path(&entry.flipbook_texture)?;
        if !terrain.entries.contains_key(entry.atlas_tile.as_str()) {
            return Err(AssetError::MissingTerrainKey {
                block: "flipbook atlas_tile".into(),
                key: entry.atlas_tile.into_boxed_str(),
            });
        }
        flipbooks.push(FlipbookSource {
            texture_path: entry.flipbook_texture.into_boxed_str(),
            atlas_tile: entry.atlas_tile.into_boxed_str(),
        });
    }
    Ok(flipbooks.into_boxed_slice())
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

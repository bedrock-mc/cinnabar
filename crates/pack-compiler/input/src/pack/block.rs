use assets::facts::pack::{PackBlock as B, PackTexture as T};
use std::{collections::BTreeMap, path::Path};

use assets::{AssetError, BlockFace, RegistryRecord, legacy_resource_pack_block_alias};
use serde::Deserialize;
use serde_json::{Map, Value};

use super::parse::{BoundedMapIssue, BoundedUniqueMap, MAX_TEXTURE_KEYS, read_json};

#[path = "block/facing_cube.rs"]
mod facing_cube;
#[path = "block/furnace.rs"]
mod furnace;
#[path = "block/leaf_metadata.rs"]
mod leaf_metadata;

pub use facing_cube::facing_cube_face;

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
    pub(super) entries: BTreeMap<Box<str>, BlockSourceEntry>,
    pub(super) component_crosses: BTreeMap<Box<str>, super::CrossMaterial>,
    pub(super) component_models: BTreeMap<u32, super::ComponentModel>,
}

/// Bounded terrain path variants indexed by texture key.
impl BlockTextureMap {
    /// State-resolved authored model from the registry used to read this pack.
    pub fn component_model(&self, record: &RegistryRecord) -> Option<&super::ComponentModel> {
        self.component_models.get(&record.sequential_id)
    }
    /// Authored material policy for a supported behavior-pack cross.
    pub fn component_cross(&self, identifier: &str) -> Option<super::CrossMaterial> {
        self.component_crosses.get(identifier).copied()
    }

    /// Returns a block texture key only when the source uses the scalar form.
    #[must_use]
    pub fn get_exact_scalar(&self, block_name: &str) -> Option<&str> {
        match &self.entries.get(block_name)?.textures {
            TextureValue::Key(key) => Some(key),
            TextureValue::Faces(_) => None,
        }
    }

    /// Returns the exact scalar source used by a reviewed vanilla block only
    /// when its entry contains the expected sound and no additional fields.
    pub fn get_exact_scalar_plain(&self, block_name: &str, expected_sound: &str) -> Option<&str> {
        let entry = self.entries.get(block_name)?;
        if entry.sound.as_ref()?.as_str()? != expected_sound || !entry.extra.is_empty() {
            return None;
        }
        match &entry.textures {
            TextureValue::Key(key) => Some(key),
            TextureValue::Faces(_) => None,
        }
    }

    /// Returns six explicit face keys. Fallback `side` routing is deliberately
    /// excluded so exact model families cannot accept an underspecified map.
    pub fn get_exact_faces(&self, block_name: &str) -> Option<[&str; 6]> {
        let TextureValue::Faces(faces) = &self.entries.get(block_name)?.textures else {
            return None;
        };
        if faces.side.is_some() || !faces.extra.is_empty() {
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

    /// Returns the exact six-face vanilla cake routing and rejects all
    /// fallback, alias, missing, or additional face keys.
    #[must_use]
    pub fn get_exact_cake_faces(&self) -> Option<[&str; 6]> {
        let faces = self.get_exact_faces(B::Cake.bare())?;
        (faces
            == [
                T::CakeWest.key(),
                T::CakeSide.key(),
                T::CakeBottom.key(),
                T::CakeTop.key(),
                T::CakeSide.key(),
                T::CakeSide.key(),
            ])
        .then_some(faces)
    }

    /// Returns the exact vanilla pillar form: down/up caps plus one horizontal
    /// side fallback and no explicit horizontal overrides.
    pub fn get_exact_pillar(&self, block_name: &str) -> Option<[&str; 3]> {
        let TextureValue::Faces(faces) = &self.entries.get(block_name)?.textures else {
            return None;
        };
        if faces.west.is_some()
            || faces.east.is_some()
            || faces.north.is_some()
            || faces.south.is_some()
            || !faces.extra.is_empty()
        {
            return None;
        }
        Some([
            faces.down.as_deref()?,
            faces.up.as_deref()?,
            faces.side.as_deref()?,
        ])
    }

    /// Returns the exact vanilla side/caps form: one horizontal side fallback
    /// plus down/up caps and no explicit horizontal overrides.
    #[must_use]
    pub fn get_exact_side_caps(&self, block_name: &str) -> Option<[&str; 3]> {
        let TextureValue::Faces(faces) = &self.entries.get(block_name)?.textures else {
            return None;
        };
        if faces.west.is_some()
            || faces.east.is_some()
            || faces.north.is_some()
            || faces.south.is_some()
            || !faces.extra.is_empty()
        {
            return None;
        }
        let side = faces.side.as_deref()?;
        let down = faces.down.as_deref()?;
        let up = faces.up.as_deref()?;
        if side.is_empty() || down.is_empty() || up.is_empty() {
            return None;
        }
        Some([side, down, up])
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum TextureValue {
    Key(String),
    Faces(FaceKeys),
}

impl TextureValue {
    pub(super) fn try_for_each_key<E>(
        &self,
        mut visitor: impl FnMut(&str) -> Result<(), E>,
    ) -> Result<(), E> {
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
pub(super) struct FaceKeys {
    west: Option<String>,
    east: Option<String>,
    down: Option<String>,
    up: Option<String>,
    north: Option<String>,
    south: Option<String>,
    side: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
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
    #[serde(default)]
    sound: Option<Value>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug)]
pub(super) struct BlockSourceEntry {
    pub(super) textures: TextureValue,
    pub(super) sound: Option<Value>,
    pub(super) extra: BTreeMap<String, Value>,
}

pub fn resolve_texture_key(
    blocks: &BlockTextureMap,
    record: &RegistryRecord,
    face: BlockFace,
) -> TextureKey {
    if let Some(model) = blocks.component_model(record) {
        return TextureKey::resolved(&model.texture, false);
    }
    let block_name = record
        .name
        .strip_prefix("minecraft:")
        .unwrap_or(&record.name);
    let resource_pack_name = if blocks.entries.contains_key(block_name) {
        block_name
    } else {
        legacy_resource_pack_block_alias(block_name).unwrap_or(block_name)
    };
    let Some(entry) = blocks.entries.get(resource_pack_name) else {
        return TextureKey::diagnostic();
    };

    match &entry.textures {
        TextureValue::Key(key) => TextureKey::resolved(key, false),
        TextureValue::Faces(faces) => {
            if facing_cube::is_record(record) {
                return facing_cube_face(record, face)
                    .and_then(|(source_face, _)| faces.resolve(source_face))
                    .map_or_else(TextureKey::diagnostic, |key| {
                        TextureKey::resolved(key, false)
                    });
            }
            if furnace::is_record(record) {
                return furnace::source_face(record, face)
                    .and_then(|source_face| faces.resolve(source_face))
                    .map_or_else(TextureKey::diagnostic, |key| {
                        TextureKey::resolved(key, false)
                    });
            }
            let axis = match state_axis(&record.canonical_state) {
                AxisState::Absent => Axis::Y,
                AxisState::Exact(axis) => axis,
                AxisState::Invalid if is_reviewed_selector_alias_cube_name(&record.name) => {
                    return TextureKey::diagnostic();
                }
                AxisState::Invalid => Axis::Y,
            };
            let (source_face, rotate_uv) = orient_face(face, axis);
            faces
                .resolve(source_face)
                .map_or_else(TextureKey::diagnostic, |key| {
                    TextureKey::resolved(key, rotate_uv)
                })
        }
    }
}

/// Retains the same block alias and axis mapping used by world texture compilation.
pub fn runtime_binding_identity(
    blocks: &BlockTextureMap,
    record: &RegistryRecord,
) -> (Box<str>, [u8; 6], [bool; 6]) {
    let name = record
        .name
        .strip_prefix("minecraft:")
        .unwrap_or(&record.name);
    let binding_name = if blocks.entries.contains_key(name) {
        name
    } else {
        legacy_resource_pack_block_alias(name).unwrap_or(name)
    };
    let axis = match state_axis(&record.canonical_state) {
        AxisState::Exact(axis) => axis,
        AxisState::Absent | AxisState::Invalid => Axis::Y,
    };
    (
        binding_name.into(),
        BlockFace::ALL.map(|face| orient_face(face, axis).0 as u8),
        BlockFace::ALL.map(|face| orient_face(face, axis).1),
    )
}

/// Admits the axis mapping shared by ordinary cube compilation, rejecting other rotations.
pub fn has_runtime_cube_face_mapping(record: &RegistryRecord) -> bool {
    let Ok(properties) = serde_json::from_str::<Map<String, Value>>(&record.canonical_state) else {
        return false;
    };
    if properties
        .keys()
        .any(|key| key.contains("direction") || key.contains("rotation"))
    {
        return false;
    }
    match state_axis(&record.canonical_state) {
        AxisState::Exact(_) => true,
        AxisState::Absent => record
            .model_state
            .get(assets::ModelStateField::Orientation)
            .is_none(),
        AxisState::Invalid => false,
    }
}

/// The texture key a vanilla block item's icon reads: `carried_textures`, else `textures`,
/// down face.
pub fn resolve_carried_down_key<'a>(
    blocks: &'a BlockTextureMap,
    record: &RegistryRecord,
) -> Option<std::borrow::Cow<'a, str>> {
    carried_or_world_key(blocks, record, BlockFace::Down, true)
}

/// A face of a block's `carried_textures`; `None` when the block declares none.
pub fn resolve_carried_face_key(
    blocks: &BlockTextureMap,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<String> {
    carried_or_world_key(blocks, record, face, false).map(std::borrow::Cow::into_owned)
}

/// Retains the unrotated carried fallback used when a block has no carried override.
pub fn resolve_carried_fallback_key(
    blocks: &BlockTextureMap,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<Box<str>> {
    carried_or_world_key(blocks, record, face, true).map(|key| key.into_owned().into_boxed_str())
}

fn carried_or_world_key<'a>(
    blocks: &'a BlockTextureMap,
    record: &RegistryRecord,
    face: BlockFace,
    world_fallback: bool,
) -> Option<std::borrow::Cow<'a, str>> {
    let block_name = record
        .name
        .strip_prefix("minecraft:")
        .unwrap_or(&record.name);
    let entry = blocks.entries.get(block_name).or_else(|| {
        blocks
            .entries
            .get(legacy_resource_pack_block_alias(block_name)?)
    })?;
    let carried = entry
        .extra
        .get("carried_textures")
        .and_then(|value| serde_json::from_value::<TextureValue>(value.clone()).ok());
    let key: std::borrow::Cow<'a, str> = match carried {
        Some(TextureValue::Key(key)) => key.into(),
        Some(TextureValue::Faces(faces)) => faces.resolve(face)?.to_owned().into(),
        None if world_fallback => match &entry.textures {
            TextureValue::Key(key) => key.as_str().into(),
            TextureValue::Faces(faces) => faces.resolve(face)?.into(),
        },
        None => return None,
    };
    (!key.is_empty()).then_some(key)
}

pub(super) fn read_blocks(path: &Path) -> Result<BlockTextureMap, AssetError> {
    let document: BoundedUniqueMap<Value, { MAX_TEXTURE_KEYS + 1 }> = read_json(path)?;
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
        entries.insert(
            name.into_boxed_str(),
            BlockSourceEntry {
                textures,
                sound: entry.sound,
                extra: entry.extra,
            },
        );
    }
    Ok(BlockTextureMap {
        entries,
        component_crosses: BTreeMap::new(),
        component_models: BTreeMap::new(),
    })
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
    Z,
}

fn is_reviewed_selector_alias_cube_name(name: &str) -> bool {
    matches!(
        assets::facts::block_render_fact(name).kind,
        assets::facts::BlockRenderKind::SelectorAliasCube
            | assets::facts::BlockRenderKind::Tnt
            | assets::facts::BlockRenderKind::DeprecatedPillar
    )
}

enum AxisState {
    Absent,
    Exact(Axis),
    Invalid,
}

fn axis_value(value: &Value) -> Option<Axis> {
    let value = if let Some(value) = value.as_str() {
        value
    } else {
        let tagged = value.as_object()?;
        if tagged.len() != 2 || tagged.get("type")?.as_str()? != "string" {
            return None;
        }
        tagged.get("value")?.as_str()?
    };
    match value {
        "x" => Some(Axis::X),
        "y" => Some(Axis::Y),
        "z" => Some(Axis::Z),
        _ => None,
    }
}

fn state_axis(canonical_state: &str) -> AxisState {
    let Ok(properties) = serde_json::from_str::<Map<String, Value>>(canonical_state) else {
        return AxisState::Invalid;
    };
    let pillar_axis = properties.get(assets::facts::BlockProperty::PillarAxis.name());
    let legacy_axis = properties.get("axis");
    match (pillar_axis, legacy_axis) {
        (None, None) => AxisState::Absent,
        (Some(_), Some(_)) => AxisState::Invalid,
        (Some(value), None) | (None, Some(value)) => {
            axis_value(value).map_or(AxisState::Invalid, AxisState::Exact)
        }
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

use std::{collections::BTreeMap, path::Path};

use assets::{AssetError, BlockFace, RegistryRecord};
use serde::Deserialize;
use serde_json::{Map, Value};

use super::parse::{BoundedMapIssue, BoundedUniqueMap, MAX_TEXTURE_KEYS, read_json};

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
}

/// Bounded terrain path variants indexed by texture key.
impl BlockTextureMap {
    /// Returns a block texture key only when the source uses the scalar form.
    #[must_use]
    pub(crate) fn get_exact_scalar(&self, block_name: &str) -> Option<&str> {
        match &self.entries.get(block_name)?.textures {
            TextureValue::Key(key) => Some(key),
            TextureValue::Faces(_) => None,
        }
    }

    /// Returns the exact scalar source used by a reviewed vanilla block only
    /// when its entry contains the expected sound and no additional fields.
    pub(crate) fn get_exact_scalar_plain(
        &self,
        block_name: &str,
        expected_sound: &str,
    ) -> Option<&str> {
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
    pub(crate) fn get_exact_faces(&self, block_name: &str) -> Option<[&str; 6]> {
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
        let faces = self.get_exact_faces("cake")?;
        (faces
            == [
                "cake_west",
                "cake_side",
                "cake_bottom",
                "cake_top",
                "cake_side",
                "cake_side",
            ])
        .then_some(faces)
    }

    /// Returns the exact vanilla pillar form: down/up caps plus one horizontal
    /// side fallback and no explicit horizontal overrides.
    pub(crate) fn get_exact_pillar(&self, block_name: &str) -> Option<[&str; 3]> {
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
    sound: Option<Value>,
    extra: BTreeMap<String, Value>,
}

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
    let Some(entry) = blocks.entries.get(resource_pack_name) else {
        return TextureKey::diagnostic();
    };

    match &entry.textures {
        TextureValue::Key(key) => TextureKey::resolved(key, false),
        TextureValue::Faces(faces) => {
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

pub(super) fn model_variant_index(
    key: &str,
    record: &RegistryRecord,
    count: usize,
) -> Option<usize> {
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

pub(super) fn read_blocks(path: &Path) -> Result<BlockTextureMap, AssetError> {
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
        entries.insert(
            name.into_boxed_str(),
            BlockSourceEntry {
                textures,
                sound: entry.sound,
                extra: entry.extra,
            },
        );
    }
    Ok(BlockTextureMap { entries })
}

enum Axis {
    X,
    Y,
    Z,
}

fn is_reviewed_selector_alias_cube_name(name: &str) -> bool {
    matches!(
        name,
        "minecraft:bone_block"
            | "minecraft:chiseled_quartz_block"
            | "minecraft:hay_block"
            | "minecraft:purpur_block"
            | "minecraft:quartz_block"
            | "minecraft:smooth_quartz"
            | "minecraft:tnt"
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
    let pillar_axis = properties.get("pillar_axis");
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

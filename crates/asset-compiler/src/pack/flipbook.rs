use std::{collections::BTreeSet, path::Path};

use assets::AssetError;
use serde::Deserialize;
use serde_json::Value;

use super::{
    parse::{MAX_TEXTURE_KEYS, read_json, validate_texture_path},
    terrain::TerrainTextureMap,
};

/// Maximum number of flipbook selectors accepted from one resource pack.
pub const MAX_FLIPBOOKS: usize = MAX_TEXTURE_KEYS;
/// Maximum number of explicit frame indices retained for one flipbook.
pub const MAX_FLIPBOOK_FRAMES: usize = 4_096;

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

pub(super) fn read_flipbooks(
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

use std::{io, path::PathBuf, str::Utf8Error};

use thiserror::Error;

/// Errors produced while reading bounded registry and resource-pack sources.
#[derive(Debug, Error)]
pub enum AssetError {
    #[error("invalid atmosphere source manifest: {source}")]
    InvalidAtmosphereManifest {
        #[source]
        source: serde_json::Error,
    },

    #[error("atmosphere source manifest is {size} bytes, exceeding the {max}-byte limit")]
    AtmosphereManifestTooLarge { size: usize, max: usize },

    #[error("invalid atmosphere source provenance: {detail}")]
    InvalidAtmosphereProvenance { detail: Box<str> },

    #[error("failed to read atmosphere texture {role} from {path}: {source}")]
    AtmosphereTextureIo {
        role: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("atmosphere texture {role} at {path} is {size} bytes, exceeding the {max}-byte limit")]
    AtmosphereTextureTooLarge {
        role: &'static str,
        path: PathBuf,
        size: usize,
        max: usize,
    },

    #[error("atmosphere texture {role} at {path} does not match the pinned source SHA-256")]
    AtmosphereTextureHashMismatch { role: &'static str, path: PathBuf },

    #[error("failed to decode atmosphere texture {role} from {path}: {source}")]
    AtmosphereTextureDecode {
        role: &'static str,
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error(
        "atmosphere texture {role} at {path} is {width}x{height}; expected exactly {expected_width}x{expected_height}"
    )]
    WrongAtmosphereTextureDimensions {
        role: &'static str,
        path: PathBuf,
        width: u32,
        height: u32,
        expected_width: u32,
        expected_height: u32,
    },

    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("JSON source {path} is {size} bytes, exceeding the {max}-byte limit")]
    JsonTooLarge {
        path: PathBuf,
        size: usize,
        max: usize,
    },

    #[error("JSON source {path} is not valid UTF-8: {source}")]
    InvalidJsonUtf8 {
        path: PathBuf,
        #[source]
        source: Utf8Error,
    },

    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("invalid JSON for block {block} in {path}: {source}")]
    InvalidBlockEntry {
        path: PathBuf,
        block: Box<str>,
        #[source]
        source: serde_json::Error,
    },

    #[error("invalid block registry magic")]
    InvalidRegistryMagic,

    #[error(
        "unexpected end of registry while reading {context}: need {needed} bytes, have {remaining}"
    )]
    UnexpectedEof {
        context: &'static str,
        needed: usize,
        remaining: usize,
    },

    #[error("registry has {count} records, exceeding the limit of {max}")]
    TooManyRegistryRecords { count: usize, max: usize },

    #[error("registry state is {size} bytes, exceeding the limit of {max}")]
    RegistryStateTooLarge { size: usize, max: usize },

    #[error("registry flags contain unsupported bits: {0:#04x}")]
    InvalidRegistryFlags(u8),

    #[error("registry {field} is not valid UTF-8: {source}")]
    InvalidRegistryUtf8 {
        field: &'static str,
        #[source]
        source: Utf8Error,
    },

    #[error("duplicate registry sequential ID {0}")]
    DuplicateSequentialId(u32),

    #[error("duplicate registry network hash {0:#010x}")]
    DuplicateNetworkHash(u32),

    #[error("registry has {remaining} trailing bytes")]
    TrailingRegistryBytes { remaining: usize },

    #[error("invalid PREG1001 physics registry: {detail}")]
    InvalidPhysicsRegistry { detail: Box<str> },

    #[error("source contains {count} texture keys, exceeding the limit of {max}")]
    TooManyTextureKeys { count: usize, max: usize },

    #[error("duplicate block key {key} in {path}")]
    DuplicateBlockKey { path: PathBuf, key: Box<str> },

    #[error("duplicate terrain texture key {key} in {path}")]
    DuplicateTerrainTextureKey { path: PathBuf, key: Box<str> },

    #[error("texture key {key} has {count} variants, exceeding the limit of {max}")]
    TooManyTextureVariants {
        key: Box<str>,
        count: usize,
        max: usize,
    },

    #[error("texture key {0} has no variants")]
    EmptyTextureVariants(Box<str>),

    #[error("texture path is {length} bytes, exceeding the limit of {max}: {path}")]
    TexturePathTooLong {
        path: Box<str>,
        length: usize,
        max: usize,
    },

    #[error("texture path must remain relative and may not contain '..': {path}")]
    UnsafeTexturePath { path: Box<str> },

    #[error("block {block} references missing terrain texture key {key}")]
    MissingTerrainKey { block: Box<str>, key: Box<str> },

    #[error("block {0} has no usable texture keys")]
    MissingBlockTextureKeys(Box<str>),

    #[error("source has {count} flipbooks, exceeding the limit of {max}")]
    TooManyFlipbooks { count: usize, max: usize },

    #[error("flipbook {index} field {field} has the wrong type; expected {expected}")]
    InvalidFlipbookFieldType {
        index: usize,
        field: &'static str,
        expected: &'static str,
    },

    #[error(
        "flipbook {index} field {field} contains an invalid unsigned 32-bit integer at element {element:?}"
    )]
    InvalidFlipbookInteger {
        index: usize,
        field: &'static str,
        element: Option<usize>,
    },

    #[error("flipbook {index} field {field} must be non-zero")]
    ZeroFlipbookValue { index: usize, field: &'static str },

    #[error("flipbook {index} has {count} explicit frames, exceeding the limit of {max}")]
    TooManyFlipbookFrames {
        index: usize,
        count: usize,
        max: usize,
    },

    #[error(
        "duplicate flipbook selector ({atlas_tile}, atlas index {atlas_index}, tile variant {atlas_tile_variant})"
    )]
    DuplicateFlipbookSelector {
        atlas_tile: Box<str>,
        atlas_index: u32,
        atlas_tile_variant: u32,
    },

    #[error("flipbook {index} timeline duration overflows its bounded u32 representation")]
    FlipbookTimelineOverflow { index: usize },

    #[error(
        "invalid animation texture limits: max_layers_per_page={max_layers_per_page}, max_pages={max_pages}; limits are 1..=2048 layers and 1..=2 pages"
    )]
    InvalidAnimationLimits {
        max_layers_per_page: u32,
        max_pages: u32,
    },

    #[error("decoded texture source {source_path} was supplied more than once")]
    DuplicateDecodedTexture { source_path: Box<str> },

    #[error("animation texture source {source_path} was not decoded")]
    MissingAnimationTexture { source_path: Box<str> },

    #[error("animation texture {source_path} is {width}x{height}: {detail}")]
    AnimationTextureDimensions {
        source_path: Box<str>,
        width: u32,
        height: u32,
        detail: Box<str>,
    },

    #[error(
        "animation texture {source_path} decoded to {actual} bytes, expected exactly {expected}"
    )]
    AnimationTextureByteLength {
        source_path: Box<str>,
        actual: usize,
        expected: usize,
    },

    #[error(
        "animation {animation} references physical frame {frame}, but its strip has {physical_frames} frames"
    )]
    FlipbookFrameOutOfRange {
        animation: usize,
        frame: u32,
        physical_frames: u32,
    },

    #[error(
        "animation layers require {required_layers} layers, exceeding {max_pages} pages of {max_layers_per_page} layers"
    )]
    TooManyAnimationTexturePages {
        required_layers: usize,
        max_layers_per_page: u32,
        max_pages: u32,
    },

    #[error("failed to read texture key {key} from {path}: {source}")]
    TextureIo {
        key: Box<str>,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("texture key {key} at {path} is {size} bytes, exceeding the {max}-byte limit")]
    TextureTooLarge {
        key: Box<str>,
        path: PathBuf,
        size: usize,
        max: usize,
    },

    #[error("failed to decode texture key {key} from {path}: {source}")]
    TextureDecode {
        key: Box<str>,
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("unsupported static texture format for key {key} at {path}; expected .png or .tga")]
    UnsupportedTextureFormat { key: Box<str>, path: PathBuf },

    #[error(
        "texture key {key} at {path} is {width}x{height}; this compiler requires exactly 16x16"
    )]
    WrongTextureDimensions {
        key: Box<str>,
        path: PathBuf,
        width: u32,
        height: u32,
    },

    #[error(
        "texture array has {count} layers, exceeding the limit of {max} (source key {key:?}, path {path:?})"
    )]
    TooManyTextureLayers {
        count: usize,
        max: usize,
        key: Option<Box<str>>,
        path: Option<PathBuf>,
    },

    #[error("compiled assets have {count} materials, exceeding the limit of {max}")]
    TooManyMaterials { count: usize, max: usize },

    #[error("registry sequential ID {id} exceeds the direct lookup limit of {max}")]
    SequentialIdOutOfRange { id: u32, max: usize },

    #[error("invalid compiled assets: {detail}")]
    InvalidCompiledAssets { detail: Box<str> },

    #[error("compiled {section} section size overflowed")]
    BlobSizeOverflow { section: &'static str },
}

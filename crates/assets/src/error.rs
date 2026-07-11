use std::{io, path::PathBuf, str::Utf8Error};

use thiserror::Error;

/// Errors produced while reading bounded registry and resource-pack sources.
#[derive(Debug, Error)]
pub enum AssetError {
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
}

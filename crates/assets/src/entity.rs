use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::AssetError;

pub const ENTITY_BLOB_MAGIC: [u8; 8] = *b"MCBEENT2";
pub const ENTITY_BLOB_VERSION: u32 = 2;
pub const MAX_ENTITY_ASSET_SOURCES: usize = 8_192;
pub const MAX_ENTITY_ASSET_SYMBOLS: usize = 16_384;
pub const MAX_ENTITY_DEPENDENCIES: usize = 512;
pub const MAX_ENTITY_ASSET_PATH_BYTES: usize = 512;
pub const MAX_ENTITY_IDENTIFIER_BYTES: usize = 512;
pub const MAX_ENTITY_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ENTITY_TOTAL_SOURCE_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_ENTITY_CATALOG_BYTES: usize = 8 * 1024 * 1024;

const HEADER_BYTES: usize = 80;
const HASH_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(u8)]
pub enum EntityAssetKind {
    Entity = 1,
    Geometry = 2,
    Animation = 3,
    AnimationController = 4,
    RenderController = 5,
    Texture = 6,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(u8)]
pub enum EntityDependencyKind {
    Geometry = 1,
    Animation = 2,
    AnimationController = 3,
    RenderController = 4,
    Texture = 5,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(u8)]
pub enum EntityDependencyResolution {
    Catalog = 1,
    External = 2,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityAssetSource {
    pub path: Box<str>,
    pub source_bytes: u32,
    pub source_sha256: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityDependency {
    pub kind: EntityDependencyKind,
    pub identifier: Box<str>,
    pub resolution: EntityDependencyResolution,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityAssetSymbol {
    pub kind: EntityAssetKind,
    pub identifier: Box<str>,
    pub source_index: u32,
    pub dependencies: Box<[EntityDependency]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledEntityAssets {
    pub source_manifest_sha256: [u8; 32],
    pub sources: Box<[EntityAssetSource]>,
    pub symbols: Box<[EntityAssetSymbol]>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EntityCatalogPayload {
    sources: Box<[EntityAssetSource]>,
    symbols: Box<[EntityAssetSymbol]>,
}

#[derive(Clone, Debug)]
pub struct RuntimeEntityAssets {
    source_manifest_sha256: [u8; 32],
    sources: Box<[EntityAssetSource]>,
    symbols: Box<[EntityAssetSymbol]>,
}

impl RuntimeEntityAssets {
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() < HEADER_BYTES + HASH_BYTES {
            return Err(invalid("truncated MCBEENT2 blob"));
        }
        if bytes[..8] != ENTITY_BLOB_MAGIC
            || u32_at(bytes, 8)? != ENTITY_BLOB_VERSION
            || bytes[20..24] != [0; 4]
            || bytes[64..HEADER_BYTES] != [0; 16]
        {
            return Err(invalid("unsupported MCBEENT2 header"));
        }
        let source_count = u32_at(bytes, 12)? as usize;
        let symbol_count = u32_at(bytes, 16)? as usize;
        if source_count == 0
            || source_count > MAX_ENTITY_ASSET_SOURCES
            || symbol_count == 0
            || symbol_count > MAX_ENTITY_ASSET_SYMBOLS
        {
            return Err(invalid("MCBEENT2 header counts exceed bounds"));
        }
        let source_manifest_sha256 = array_at::<32>(bytes, 24)?;
        let payload_bytes = usize::try_from(u64::from_le_bytes(array_at(bytes, 56)?))
            .map_err(|_| invalid("MCBEENT2 payload size exceeds platform"))?;
        if payload_bytes > MAX_ENTITY_CATALOG_BYTES
            || bytes.len()
                != HEADER_BYTES
                    .checked_add(payload_bytes)
                    .and_then(|length| length.checked_add(HASH_BYTES))
                    .ok_or_else(|| invalid("MCBEENT2 length overflow"))?
        {
            return Err(invalid("noncanonical MCBEENT2 section layout"));
        }
        let payload_end = HEADER_BYTES + payload_bytes;
        if Sha256::digest(&bytes[..payload_end]).as_slice() != &bytes[payload_end..] {
            return Err(invalid("MCBEENT2 envelope hash mismatch"));
        }
        let payload: EntityCatalogPayload =
            serde_json::from_slice(&bytes[HEADER_BYTES..payload_end])
                .map_err(|_| invalid("invalid MCBEENT2 catalog payload"))?;
        if payload.sources.len() != source_count || payload.symbols.len() != symbol_count {
            return Err(invalid("MCBEENT2 catalog counts do not match header"));
        }
        let canonical = serde_json::to_vec(&payload)
            .map_err(|_| invalid("failed to canonicalize MCBEENT2 catalog payload"))?;
        if canonical.as_slice() != &bytes[HEADER_BYTES..payload_end] {
            return Err(invalid("noncanonical MCBEENT2 catalog encoding"));
        }
        let compiled = CompiledEntityAssets {
            source_manifest_sha256,
            sources: payload.sources,
            symbols: payload.symbols,
        };
        validate_compiled(&compiled)?;
        Ok(Self {
            source_manifest_sha256,
            sources: compiled.sources,
            symbols: compiled.symbols,
        })
    }

    #[must_use]
    pub const fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }

    #[must_use]
    pub fn sources(&self) -> &[EntityAssetSource] {
        &self.sources
    }

    #[must_use]
    pub fn symbols(&self) -> &[EntityAssetSymbol] {
        &self.symbols
    }

    #[must_use]
    pub fn symbol_candidates(
        &self,
        kind: EntityAssetKind,
        identifier: &str,
    ) -> &[EntityAssetSymbol] {
        symbol_candidates(&self.symbols, kind, identifier)
    }
}

pub fn encode_entity_blob(compiled: &CompiledEntityAssets) -> Result<Box<[u8]>, AssetError> {
    validate_compiled(compiled)?;
    let payload = serde_json::to_vec(&EntityCatalogPayload {
        sources: compiled.sources.clone(),
        symbols: compiled.symbols.clone(),
    })
    .map_err(|_| invalid("failed to encode MCBEENT2 catalog payload"))?;
    if payload.len() > MAX_ENTITY_CATALOG_BYTES {
        return Err(invalid("MCBEENT2 catalog payload exceeds bound"));
    }
    let total = HEADER_BYTES
        .checked_add(payload.len())
        .and_then(|length| length.checked_add(HASH_BYTES))
        .ok_or_else(|| invalid("MCBEENT2 length overflow"))?;
    let mut bytes = Vec::with_capacity(total);
    bytes.extend_from_slice(&ENTITY_BLOB_MAGIC);
    bytes.extend_from_slice(&ENTITY_BLOB_VERSION.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(compiled.sources.len())
            .map_err(|_| invalid("MCBEENT2 source count overflow"))?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(
        &u32::try_from(compiled.symbols.len())
            .map_err(|_| invalid("MCBEENT2 symbol count overflow"))?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&compiled.source_manifest_sha256);
    bytes.extend_from_slice(
        &u64::try_from(payload.len())
            .map_err(|_| invalid("MCBEENT2 payload length overflow"))?
            .to_le_bytes(),
    );
    bytes.resize(HEADER_BYTES, 0);
    bytes.extend_from_slice(&payload);
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    Ok(bytes.into_boxed_slice())
}

fn validate_compiled(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    if compiled.source_manifest_sha256 == [0; 32]
        || compiled.sources.is_empty()
        || compiled.sources.len() > MAX_ENTITY_ASSET_SOURCES
        || compiled.symbols.is_empty()
        || compiled.symbols.len() > MAX_ENTITY_ASSET_SYMBOLS
    {
        return Err(invalid("invalid entity catalog provenance or counts"));
    }
    let mut total_source_bytes = 0usize;
    let mut previous_path: Option<&str> = None;
    for source in &compiled.sources {
        validate_relative_path(&source.path)?;
        if previous_path.is_some_and(|previous| previous >= source.path.as_ref())
            || source.source_bytes == 0
            || source.source_bytes as usize > MAX_ENTITY_SOURCE_BYTES
            || source.source_sha256 == [0; 32]
        {
            return Err(invalid("invalid or unordered entity catalog source"));
        }
        total_source_bytes = total_source_bytes
            .checked_add(source.source_bytes as usize)
            .ok_or_else(|| invalid("entity catalog source-byte total overflow"))?;
        previous_path = Some(&source.path);
    }
    if total_source_bytes > MAX_ENTITY_TOTAL_SOURCE_BYTES {
        return Err(invalid("entity catalog source-byte total exceeds bound"));
    }

    let mut previous_symbol: Option<(EntityAssetKind, &str, u32)> = None;
    for symbol in &compiled.symbols {
        validate_identifier(&symbol.identifier)?;
        let key = (symbol.kind, symbol.identifier.as_ref(), symbol.source_index);
        if previous_symbol.is_some_and(|previous| previous >= key)
            || symbol.source_index as usize >= compiled.sources.len()
            || symbol.dependencies.len() > MAX_ENTITY_DEPENDENCIES
        {
            return Err(invalid("invalid or unordered entity catalog symbol"));
        }
        validate_symbol_source(
            symbol.kind,
            &compiled.sources[symbol.source_index as usize].path,
        )?;
        let mut previous_dependency: Option<(EntityDependencyKind, &str)> = None;
        for dependency in &symbol.dependencies {
            validate_identifier(&dependency.identifier)?;
            let dependency_key = (dependency.kind, dependency.identifier.as_ref());
            if previous_dependency.is_some_and(|previous| previous >= dependency_key) {
                return Err(invalid("entity dependencies are not strictly ordered"));
            }
            let target_kind = dependency_asset_kind(dependency.kind);
            let has_catalog_target =
                !symbol_candidates(&compiled.symbols, target_kind, &dependency.identifier)
                    .is_empty();
            let resolution_agrees = match dependency.resolution {
                EntityDependencyResolution::Catalog => has_catalog_target,
                EntityDependencyResolution::External => !has_catalog_target,
            };
            if !resolution_agrees {
                return Err(invalid(
                    "entity dependency resolution disagrees with catalog contents",
                ));
            }
            previous_dependency = Some(dependency_key);
        }
        previous_symbol = Some(key);
    }
    Ok(())
}

fn symbol_candidates<'a>(
    symbols: &'a [EntityAssetSymbol],
    kind: EntityAssetKind,
    identifier: &str,
) -> &'a [EntityAssetSymbol] {
    let start = symbols
        .partition_point(|symbol| (symbol.kind, symbol.identifier.as_ref()) < (kind, identifier));
    let matching = &symbols[start..];
    let length = matching
        .partition_point(|symbol| symbol.kind == kind && symbol.identifier.as_ref() == identifier);
    &matching[..length]
}

const fn dependency_asset_kind(kind: EntityDependencyKind) -> EntityAssetKind {
    match kind {
        EntityDependencyKind::Geometry => EntityAssetKind::Geometry,
        EntityDependencyKind::Animation => EntityAssetKind::Animation,
        EntityDependencyKind::AnimationController => EntityAssetKind::AnimationController,
        EntityDependencyKind::RenderController => EntityAssetKind::RenderController,
        EntityDependencyKind::Texture => EntityAssetKind::Texture,
    }
}

fn validate_symbol_source(kind: EntityAssetKind, path: &str) -> Result<(), AssetError> {
    let matches = match kind {
        EntityAssetKind::Entity => path.starts_with("entity/") && path.ends_with(".json"),
        EntityAssetKind::Geometry => path.starts_with("models/entity/") && path.ends_with(".json"),
        EntityAssetKind::Animation => path.starts_with("animations/") && path.ends_with(".json"),
        EntityAssetKind::AnimationController => {
            path.starts_with("animation_controllers/") && path.ends_with(".json")
        }
        EntityAssetKind::RenderController => {
            path.starts_with("render_controllers/") && path.ends_with(".json")
        }
        EntityAssetKind::Texture => {
            path.starts_with("textures/entity/")
                && (path.ends_with(".png") || path.ends_with(".tga"))
        }
    };
    if matches {
        Ok(())
    } else {
        Err(invalid("entity symbol kind does not match its source path"))
    }
}

fn validate_relative_path(path: &str) -> Result<(), AssetError> {
    if path.is_empty()
        || path.len() > MAX_ENTITY_ASSET_PATH_BYTES
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(invalid("entity source path is unsafe or exceeds its bound"));
    }
    Ok(())
}

fn validate_identifier(identifier: &str) -> Result<(), AssetError> {
    if identifier.is_empty()
        || identifier.len() > MAX_ENTITY_IDENTIFIER_BYTES
        || identifier.chars().any(char::is_control)
    {
        return Err(invalid("entity identifier is empty or exceeds its bound"));
    }
    Ok(())
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, AssetError> {
    Ok(u32::from_le_bytes(array_at(bytes, offset)?))
}

fn array_at<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], AssetError> {
    bytes
        .get(offset..offset + N)
        .ok_or_else(|| invalid("truncated MCBEENT2 field"))?
        .try_into()
        .map_err(|_| invalid("invalid MCBEENT2 field"))
}

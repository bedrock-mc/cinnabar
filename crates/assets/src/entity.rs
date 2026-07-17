use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::AssetError;
use crate::item::{ItemVisualAlias, ItemVisualDefinition};

#[path = "entity/v4.rs"]
mod v4;

use v4::validate_extended_payload;
#[allow(unused_imports)]
pub use v4::{
    CompiledMolangExpression, EntityAnimationChannel, EntityAnimationClip,
    EntityAnimationController, EntityAnimationInterpolation, EntityAnimationKeyframe,
    EntityAnimationLoop, EntityAnimationProperty, EntityAssetSummary, EntityControllerAnimation,
    EntityControllerState, EntityControllerTransition, EntityRigAnimationBinding, EntityRigBinding,
    EntityRigControllerBinding, EntityRigFallback, MAX_ENTITY_ANIMATION_CHANNELS,
    MAX_ENTITY_ANIMATION_CLIPS, MAX_ENTITY_ANIMATION_KEYFRAMES, MAX_ENTITY_CONTROLLER_ANIMATIONS,
    MAX_ENTITY_CONTROLLER_STATES, MAX_ENTITY_CONTROLLER_TRANSITIONS, MAX_ENTITY_CONTROLLERS,
    MAX_ENTITY_RIG_ANIMATIONS, MAX_ENTITY_RIG_BINDINGS, MAX_ENTITY_RIG_CONTROLLERS,
    MAX_MOLANG_COLLECTION_ITEMS, MAX_MOLANG_COLLECTION_ITEMS_TOTAL, MAX_MOLANG_COLLECTIONS,
    MAX_MOLANG_EXPRESSIONS, MAX_MOLANG_OPS, MAX_MOLANG_OPS_PER_EXPRESSION, MAX_MOLANG_STACK_DEPTH,
    MolangCollection, MolangCollectionItem, MolangOp, MolangSymbol, MolangSymbolKind,
};

pub const ENTITY_BLOB_MAGIC: [u8; 8] = *b"MCBEENT3";
pub const ENTITY_BLOB_VERSION: u32 = 4;
pub const MAX_ENTITY_ASSET_SOURCES: usize = 8_192;
pub const MAX_ENTITY_ASSET_SYMBOLS: usize = 16_384;
pub const MAX_ENTITY_DEPENDENCIES: usize = 512;
pub const MAX_ENTITY_ASSET_PATH_BYTES: usize = 512;
pub const MAX_ENTITY_IDENTIFIER_BYTES: usize = 512;
pub const MAX_ENTITY_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ENTITY_TOTAL_SOURCE_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_ENTITY_CATALOG_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_ENTITY_GEOMETRIES: usize = 4_096;
pub const MAX_ENTITY_GEOMETRY_BONES: usize = 512;
pub const MAX_ENTITY_GEOMETRY_CUBES: usize = 8_192;
pub const MAX_ENTITY_GEOMETRY_NAME_BYTES: usize = 256;
pub const MAX_ENTITY_TEXTURE_DIMENSION: u16 = 16_384;
pub const MAX_ENTITY_GEOMETRY_SCALAR: f32 = 1_048_576.0;

const MAX_ENTITY_GEOMETRY_INHERITANCE_DEPTH: usize = 64;

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

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EntityGeometryScalar(u32);

impl EntityGeometryScalar {
    #[must_use]
    pub fn new(value: f32) -> Option<Self> {
        if !value.is_finite() || value.abs() > MAX_ENTITY_GEOMETRY_SCALAR {
            return None;
        }
        Some(Self(if value == 0.0 { 0 } else { value.to_bits() }))
    }

    #[must_use]
    pub const fn get(self) -> f32 {
        f32::from_bits(self.0)
    }

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometryCube {
    pub origin: [EntityGeometryScalar; 3],
    pub size: [EntityGeometryScalar; 3],
    pub pivot: [EntityGeometryScalar; 3],
    pub rotation: [EntityGeometryScalar; 3],
    pub uv: EntityGeometryUv,
    pub inflate: EntityGeometryScalar,
    pub mirror: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "mapping", content = "value")]
pub enum EntityGeometryUv {
    Box([EntityGeometryScalar; 2]),
    Faces(EntityGeometryFaceUvs),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometryFaceUv {
    pub uv: [EntityGeometryScalar; 2],
    pub uv_size: Option<[EntityGeometryScalar; 2]>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometryFaceUvs {
    pub north: Option<EntityGeometryFaceUv>,
    pub south: Option<EntityGeometryFaceUv>,
    pub east: Option<EntityGeometryFaceUv>,
    pub west: Option<EntityGeometryFaceUv>,
    pub up: Option<EntityGeometryFaceUv>,
    pub down: Option<EntityGeometryFaceUv>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometryBone {
    pub name: Box<str>,
    pub parent: Option<Box<str>>,
    pub pivot: Option<[EntityGeometryScalar; 3]>,
    pub rotation: Option<[EntityGeometryScalar; 3]>,
    pub mirror: Option<bool>,
    pub inflate: Option<EntityGeometryScalar>,
    pub never_render: Option<bool>,
    pub reset: Option<bool>,
    pub cubes: Box<[EntityGeometryCube]>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometry {
    pub identifier: Box<str>,
    pub inherits: Option<EntityGeometryInheritance>,
    pub source_index: u32,
    pub texture_width: u16,
    pub texture_height: u16,
    pub bones: Box<[EntityGeometryBone]>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometryInheritance {
    pub identifier: Box<str>,
    pub resolution: EntityDependencyResolution,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledEntityAssets {
    pub source_manifest_sha256: [u8; 32],
    pub block_visual_count: u32,
    pub sources: Box<[EntityAssetSource]>,
    pub symbols: Box<[EntityAssetSymbol]>,
    pub geometries: Box<[EntityGeometry]>,
    pub animation_clips: Box<[EntityAnimationClip]>,
    pub animation_channels: Box<[EntityAnimationChannel]>,
    pub animation_keyframes: Box<[EntityAnimationKeyframe]>,
    pub molang_symbols: Box<[MolangSymbol]>,
    pub molang_expressions: Box<[CompiledMolangExpression]>,
    pub molang_ops: Box<[MolangOp]>,
    pub molang_collections: Box<[MolangCollection]>,
    pub molang_collection_items: Box<[MolangCollectionItem]>,
    pub controllers: Box<[EntityAnimationController]>,
    pub controller_states: Box<[EntityControllerState]>,
    pub controller_animations: Box<[EntityControllerAnimation]>,
    pub controller_transitions: Box<[EntityControllerTransition]>,
    pub rig_bindings: Box<[EntityRigBinding]>,
    pub rig_animations: Box<[EntityRigAnimationBinding]>,
    pub rig_controllers: Box<[EntityRigControllerBinding]>,
    pub item_visuals: Box<[ItemVisualDefinition]>,
    pub item_visual_aliases: Box<[ItemVisualAlias]>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EntityCatalogPayload {
    block_visual_count: u32,
    sources: Box<[EntityAssetSource]>,
    symbols: Box<[EntityAssetSymbol]>,
    geometries: Box<[EntityGeometry]>,
    animation_clips: Box<[EntityAnimationClip]>,
    animation_channels: Box<[EntityAnimationChannel]>,
    animation_keyframes: Box<[EntityAnimationKeyframe]>,
    molang_symbols: Box<[MolangSymbol]>,
    molang_expressions: Box<[CompiledMolangExpression]>,
    molang_ops: Box<[MolangOp]>,
    molang_collections: Box<[MolangCollection]>,
    molang_collection_items: Box<[MolangCollectionItem]>,
    controllers: Box<[EntityAnimationController]>,
    controller_states: Box<[EntityControllerState]>,
    controller_animations: Box<[EntityControllerAnimation]>,
    controller_transitions: Box<[EntityControllerTransition]>,
    rig_bindings: Box<[EntityRigBinding]>,
    rig_animations: Box<[EntityRigAnimationBinding]>,
    rig_controllers: Box<[EntityRigControllerBinding]>,
    item_visuals: Box<[ItemVisualDefinition]>,
    item_visual_aliases: Box<[ItemVisualAlias]>,
}

#[derive(Clone, Debug)]
pub struct RuntimeEntityAssets {
    source_manifest_sha256: [u8; 32],
    block_visual_count: u32,
    sources: Arc<[EntityAssetSource]>,
    symbols: Arc<[EntityAssetSymbol]>,
    geometries: Arc<[EntityGeometry]>,
    animation_clips: Arc<[EntityAnimationClip]>,
    animation_channels: Arc<[EntityAnimationChannel]>,
    animation_keyframes: Arc<[EntityAnimationKeyframe]>,
    molang_symbols: Arc<[MolangSymbol]>,
    molang_expressions: Arc<[CompiledMolangExpression]>,
    molang_ops: Arc<[MolangOp]>,
    molang_collections: Arc<[MolangCollection]>,
    molang_collection_items: Arc<[MolangCollectionItem]>,
    controllers: Arc<[EntityAnimationController]>,
    controller_states: Arc<[EntityControllerState]>,
    controller_animations: Arc<[EntityControllerAnimation]>,
    controller_transitions: Arc<[EntityControllerTransition]>,
    rig_bindings: Arc<[EntityRigBinding]>,
    rig_animations: Arc<[EntityRigAnimationBinding]>,
    rig_controllers: Arc<[EntityRigControllerBinding]>,
    item_visuals: Arc<[ItemVisualDefinition]>,
    item_visual_aliases: Arc<[ItemVisualAlias]>,
}

impl RuntimeEntityAssets {
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() < HEADER_BYTES + HASH_BYTES {
            return Err(invalid("truncated MCBEENT4 blob"));
        }
        if bytes[..8] != ENTITY_BLOB_MAGIC || u32_at(bytes, 8)? != ENTITY_BLOB_VERSION {
            return Err(invalid("unsupported MCBEENT4 header"));
        }
        let source_count = u32_at(bytes, 12)? as usize;
        let symbol_count = u32_at(bytes, 16)? as usize;
        let geometry_count = u32_at(bytes, 20)? as usize;
        let animation_clip_count = u32_at(bytes, 64)? as usize;
        let controller_count = u32_at(bytes, 68)? as usize;
        let rig_binding_count = u32_at(bytes, 72)? as usize;
        let item_visual_count = u32_at(bytes, 76)? as usize;
        if source_count == 0
            || source_count > MAX_ENTITY_ASSET_SOURCES
            || symbol_count == 0
            || symbol_count > MAX_ENTITY_ASSET_SYMBOLS
            || geometry_count > MAX_ENTITY_GEOMETRIES
            || animation_clip_count > MAX_ENTITY_ANIMATION_CLIPS
            || controller_count > MAX_ENTITY_CONTROLLERS
            || rig_binding_count > MAX_ENTITY_RIG_BINDINGS
            || item_visual_count > crate::item::MAX_ITEM_VISUALS
        {
            return Err(invalid("MCBEENT4 header counts exceed bounds"));
        }
        let source_manifest_sha256 = array_at::<32>(bytes, 24)?;
        let payload_bytes = usize::try_from(u64::from_le_bytes(array_at(bytes, 56)?))
            .map_err(|_| invalid("MCBEENT4 payload size exceeds platform"))?;
        if payload_bytes > MAX_ENTITY_CATALOG_BYTES
            || bytes.len()
                != HEADER_BYTES
                    .checked_add(payload_bytes)
                    .and_then(|length| length.checked_add(HASH_BYTES))
                    .ok_or_else(|| invalid("MCBEENT4 length overflow"))?
        {
            return Err(invalid("noncanonical MCBEENT4 section layout"));
        }
        let payload_end = HEADER_BYTES + payload_bytes;
        if Sha256::digest(&bytes[..payload_end]).as_slice() != &bytes[payload_end..] {
            return Err(invalid("MCBEENT4 envelope hash mismatch"));
        }
        let payload_counts = v4::payload_counts(&bytes[HEADER_BYTES..payload_end])
            .map_err(|_| invalid("invalid MCBEENT4 catalog count preflight"))?;
        if payload_counts
            != [
                source_count,
                symbol_count,
                geometry_count,
                animation_clip_count,
                controller_count,
                rig_binding_count,
                item_visual_count,
            ]
        {
            return Err(invalid("MCBEENT4 catalog counts do not match header"));
        }
        let payload: EntityCatalogPayload =
            serde_json::from_slice(&bytes[HEADER_BYTES..payload_end])
                .map_err(|_| invalid("invalid MCBEENT4 catalog payload"))?;
        let canonical = serde_json::to_vec(&payload)
            .map_err(|_| invalid("failed to canonicalize MCBEENT4 catalog payload"))?;
        if canonical.as_slice() != &bytes[HEADER_BYTES..payload_end] {
            return Err(invalid("noncanonical MCBEENT4 catalog encoding"));
        }
        let compiled = CompiledEntityAssets {
            source_manifest_sha256,
            block_visual_count: payload.block_visual_count,
            sources: payload.sources,
            symbols: payload.symbols,
            geometries: payload.geometries,
            animation_clips: payload.animation_clips,
            animation_channels: payload.animation_channels,
            animation_keyframes: payload.animation_keyframes,
            molang_symbols: payload.molang_symbols,
            molang_expressions: payload.molang_expressions,
            molang_ops: payload.molang_ops,
            molang_collections: payload.molang_collections,
            molang_collection_items: payload.molang_collection_items,
            controllers: payload.controllers,
            controller_states: payload.controller_states,
            controller_animations: payload.controller_animations,
            controller_transitions: payload.controller_transitions,
            rig_bindings: payload.rig_bindings,
            rig_animations: payload.rig_animations,
            rig_controllers: payload.rig_controllers,
            item_visuals: payload.item_visuals,
            item_visual_aliases: payload.item_visual_aliases,
        };
        validate_compiled(&compiled)?;
        Ok(Self {
            source_manifest_sha256,
            block_visual_count: compiled.block_visual_count,
            sources: Arc::from(compiled.sources),
            symbols: Arc::from(compiled.symbols),
            geometries: Arc::from(compiled.geometries),
            animation_clips: Arc::from(compiled.animation_clips),
            animation_channels: Arc::from(compiled.animation_channels),
            animation_keyframes: Arc::from(compiled.animation_keyframes),
            molang_symbols: Arc::from(compiled.molang_symbols),
            molang_expressions: Arc::from(compiled.molang_expressions),
            molang_ops: Arc::from(compiled.molang_ops),
            molang_collections: Arc::from(compiled.molang_collections),
            molang_collection_items: Arc::from(compiled.molang_collection_items),
            controllers: Arc::from(compiled.controllers),
            controller_states: Arc::from(compiled.controller_states),
            controller_animations: Arc::from(compiled.controller_animations),
            controller_transitions: Arc::from(compiled.controller_transitions),
            rig_bindings: Arc::from(compiled.rig_bindings),
            rig_animations: Arc::from(compiled.rig_animations),
            rig_controllers: Arc::from(compiled.rig_controllers),
            item_visuals: Arc::from(compiled.item_visuals),
            item_visual_aliases: Arc::from(compiled.item_visual_aliases),
        })
    }

    #[must_use]
    pub const fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }

    #[must_use]
    pub const fn block_visual_count(&self) -> u32 {
        self.block_visual_count
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
    pub fn geometries(&self) -> &[EntityGeometry] {
        &self.geometries
    }

    #[must_use]
    pub fn geometry_candidates(&self, identifier: &str) -> &[EntityGeometry] {
        let start = self
            .geometries
            .partition_point(|geometry| geometry.identifier.as_ref() < identifier);
        let matching = &self.geometries[start..];
        let length =
            matching.partition_point(|geometry| geometry.identifier.as_ref() == identifier);
        &matching[..length]
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
    v4::encode_compiled(compiled)
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
    validate_geometries(compiled)?;
    validate_extended_payload(compiled)?;
    Ok(())
}

fn validate_geometries(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    if compiled.geometries.len() > MAX_ENTITY_GEOMETRIES {
        return Err(invalid("entity geometry count exceeds bound"));
    }
    let geometry_symbols = compiled
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == EntityAssetKind::Geometry)
        .collect::<Vec<_>>();
    if geometry_symbols.len() != compiled.geometries.len() {
        return Err(invalid(
            "entity geometry payloads do not exactly match geometry symbols",
        ));
    }

    let mut previous: Option<(&str, u32)> = None;
    for (geometry, symbol) in compiled.geometries.iter().zip(geometry_symbols) {
        validate_identifier(&geometry.identifier)?;
        if let Some(inherits) = &geometry.inherits {
            validate_identifier(&inherits.identifier)?;
            let has_catalog_target = !symbol_candidates(
                &compiled.symbols,
                EntityAssetKind::Geometry,
                &inherits.identifier,
            )
            .is_empty();
            let resolution_agrees = match inherits.resolution {
                EntityDependencyResolution::Catalog => has_catalog_target,
                EntityDependencyResolution::External => !has_catalog_target,
            };
            if inherits.identifier == geometry.identifier || !resolution_agrees {
                return Err(invalid("invalid entity geometry inheritance target"));
            }
        }
        let key = (geometry.identifier.as_ref(), geometry.source_index);
        if previous.is_some_and(|previous| previous >= key)
            || key != (symbol.identifier.as_ref(), symbol.source_index)
            || geometry.source_index as usize >= compiled.sources.len()
            || geometry.texture_width == 0
            || geometry.texture_width > MAX_ENTITY_TEXTURE_DIMENSION
            || geometry.texture_height == 0
            || geometry.texture_height > MAX_ENTITY_TEXTURE_DIMENSION
            || geometry.bones.len() > MAX_ENTITY_GEOMETRY_BONES
        {
            return Err(invalid("invalid or unordered entity geometry payload"));
        }
        validate_geometry_bones(&geometry.bones, geometry.inherits.is_some())?;
        previous = Some(key);
    }
    validate_entity_geometry_inheritance(&compiled.geometries).map(|_| ())
}

/// Validates deterministic catalog inheritance selection and inherited bone parents.
///
/// External inheritance overlays are retained, but an explicit bone parent must
/// resolve within the retained local/catalog chain; unseen external bones are
/// never assumed to exist. Returns the selected catalog parent index for each
/// geometry; roots and external inheritance targets return `None`.
pub fn validate_entity_geometry_inheritance(
    geometries: &[EntityGeometry],
) -> Result<Box<[Option<usize>]>, AssetError> {
    let parents = geometries
        .iter()
        .enumerate()
        .map(|(index, geometry)| select_geometry_parent(geometries, index, geometry))
        .collect::<Result<Vec<_>, AssetError>>()?;

    for start in 0..geometries.len() {
        let chain = selected_geometry_chain(&parents, start)?;
        let mut effective_parents = std::collections::BTreeMap::<String, Option<String>>::new();
        for index in chain.into_iter().rev() {
            for bone in &geometries[index].bones {
                let name = bone.name.to_ascii_lowercase();
                if let Some(parent) = &bone.parent {
                    effective_parents.insert(name, Some(parent.to_ascii_lowercase()));
                } else {
                    effective_parents.entry(name).or_insert(None);
                }
            }
        }

        for parent in effective_parents.values().flatten() {
            if !effective_parents.contains_key(parent) {
                return Err(invalid(
                    "unresolved entity geometry bone parent in selected inheritance chain",
                ));
            }
        }
        for bone in effective_parents.keys() {
            let mut current = Some(bone.as_str());
            for step in 0..=effective_parents.len() {
                let Some(name) = current else {
                    break;
                };
                if step == effective_parents.len() {
                    return Err(invalid(
                        "entity geometry bone hierarchy contains an inherited cycle",
                    ));
                }
                current = effective_parents.get(name).and_then(Option::as_deref);
            }
        }
    }
    Ok(parents.into_boxed_slice())
}

fn select_geometry_parent(
    geometries: &[EntityGeometry],
    geometry_index: usize,
    geometry: &EntityGeometry,
) -> Result<Option<usize>, AssetError> {
    let Some(inherits) = &geometry.inherits else {
        return Ok(None);
    };
    if inherits.identifier == geometry.identifier {
        return Err(invalid("invalid entity geometry inheritance target"));
    }
    if inherits.resolution == EntityDependencyResolution::External {
        return Ok(None);
    }

    let mut candidates = geometries
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.identifier == inherits.identifier);
    let first = candidates
        .next()
        .ok_or_else(|| invalid("missing catalog entity geometry inheritance target"))?;
    let second = candidates.next();
    if second.is_none() {
        return Ok(Some(first.0));
    }

    let same_source = geometries
        .iter()
        .enumerate()
        .filter(|(index, candidate)| {
            *index != geometry_index
                && candidate.identifier == inherits.identifier
                && candidate.source_index == geometry.source_index
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match same_source.as_slice() {
        [index] => Ok(Some(*index)),
        _ => Err(invalid(
            "ambiguous catalog entity geometry inheritance target",
        )),
    }
}

fn selected_geometry_chain(
    parents: &[Option<usize>],
    start: usize,
) -> Result<Vec<usize>, AssetError> {
    let mut chain = Vec::new();
    let mut current = start;
    for _ in 0..=MAX_ENTITY_GEOMETRY_INHERITANCE_DEPTH {
        if chain.contains(&current) {
            return Err(invalid("entity geometry inheritance contains a cycle"));
        }
        chain.push(current);
        match parents[current] {
            None => return Ok(chain),
            Some(parent) => current = parent,
        }
    }
    Err(invalid("entity geometry inheritance depth exceeds bound"))
}

pub(super) fn effective_geometry_bone_counts(
    geometries: &[EntityGeometry],
) -> Result<Box<[usize]>, AssetError> {
    let parents = validate_entity_geometry_inheritance(geometries)?;
    let mut counts = Vec::with_capacity(geometries.len());
    for geometry_index in 0..geometries.len() {
        let chain = selected_geometry_chain(&parents, geometry_index)?;
        let mut names = std::collections::BTreeSet::new();
        for index in chain {
            for bone in &geometries[index].bones {
                names.insert(bone.name.to_ascii_lowercase());
            }
        }
        counts.push(names.len());
    }
    Ok(counts.into_boxed_slice())
}

fn validate_geometry_bones(
    bones: &[EntityGeometryBone],
    allow_inherited_parent: bool,
) -> Result<(), AssetError> {
    let mut total_cubes = 0usize;
    for bone in bones {
        validate_geometry_name(&bone.name)?;
        if let Some(parent) = &bone.parent {
            validate_geometry_name(parent)?;
            let invalid_parent = parent.eq_ignore_ascii_case(&bone.name)
                || (!allow_inherited_parent
                    && !bones
                        .iter()
                        .any(|candidate| candidate.name.eq_ignore_ascii_case(parent)));
            if invalid_parent {
                return Err(invalid("invalid entity geometry bone parent"));
            }
        }
        if let Some(pivot) = &bone.pivot {
            validate_scalars(pivot)?;
        }
        if let Some(rotation) = &bone.rotation {
            validate_scalars(rotation)?;
        }
        if let Some(inflate) = bone.inflate {
            validate_geometry_scalar(inflate)?;
        }
        total_cubes = total_cubes
            .checked_add(bone.cubes.len())
            .ok_or_else(|| invalid("entity geometry cube count overflow"))?;
        if total_cubes > MAX_ENTITY_GEOMETRY_CUBES {
            return Err(invalid("entity geometry cube count exceeds bound"));
        }
        for cube in &bone.cubes {
            validate_scalars(&cube.origin)?;
            validate_scalars(&cube.size)?;
            validate_scalars(&cube.pivot)?;
            validate_scalars(&cube.rotation)?;
            validate_geometry_uv(&cube.uv)?;
            validate_geometry_scalar(cube.inflate)?;
            if cube.size.iter().any(|value| value.get() < 0.0) {
                return Err(invalid("entity geometry cube size is negative"));
            }
        }
    }

    for start in 0..bones.len() {
        let mut current = Some(start);
        for step in 0..=bones.len() {
            let Some(index) = current else {
                break;
            };
            if step == bones.len() {
                return Err(invalid("entity geometry bone hierarchy contains a cycle"));
            }
            current = bones[index].parent.as_ref().and_then(|parent| {
                bones
                    .iter()
                    .position(|candidate| candidate.name.eq_ignore_ascii_case(parent))
            });
        }
    }
    Ok(())
}

fn validate_geometry_uv(uv: &EntityGeometryUv) -> Result<(), AssetError> {
    match uv {
        EntityGeometryUv::Box(uv) => validate_scalars(uv),
        EntityGeometryUv::Faces(faces) => {
            let faces = [
                &faces.north,
                &faces.south,
                &faces.east,
                &faces.west,
                &faces.up,
                &faces.down,
            ];
            if faces.iter().all(|face| face.is_none()) {
                return Err(invalid("entity geometry per-face UV map is empty"));
            }
            for face in faces.into_iter().flatten() {
                validate_scalars(&face.uv)?;
                if let Some(uv_size) = &face.uv_size {
                    validate_scalars(uv_size)?;
                }
            }
            Ok(())
        }
    }
}

fn validate_geometry_name(name: &str) -> Result<(), AssetError> {
    if name.is_empty()
        || name.len() > MAX_ENTITY_GEOMETRY_NAME_BYTES
        || name.chars().any(char::is_control)
    {
        return Err(invalid("invalid entity geometry bone name"));
    }
    Ok(())
}

fn validate_scalars<const N: usize>(values: &[EntityGeometryScalar; N]) -> Result<(), AssetError> {
    for value in values {
        validate_geometry_scalar(*value)?;
    }
    Ok(())
}

fn validate_geometry_scalar(value: EntityGeometryScalar) -> Result<(), AssetError> {
    if EntityGeometryScalar::new(value.get()) != Some(value) {
        return Err(invalid("noncanonical entity geometry scalar"));
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
        .ok_or_else(|| invalid("truncated MCBEENT4 field"))?
        .try_into()
        .map_err(|_| invalid("invalid MCBEENT4 field"))
}

use serde::{Deserialize, Serialize};

use crate::AssetError;
use crate::item::{ItemVisualAlias, ItemVisualDefinition};

mod engine_variables;
mod geometry;
pub use engine_variables::EngineVariable;
pub mod interactions;
pub mod item_use;
mod molang_program;
pub mod placement;
mod query_manifest;
pub mod seats;
pub use query_manifest::{
    MOLANG_QUERIES, MOLANG_QUERY_DESCRIPTORS, MOLANG_QUERY_MANIFEST_VERSION, MolangQuery,
    MolangQueryArguments, MolangQueryDescriptor, MolangQueryHandler, MolangQueryOutput,
    MolangQuerySupport, bind_molang_queries,
};
pub mod mount;
mod server_animation;
pub use molang_program::MolangProgram;
#[path = "entity/inherited_cubes.rs"]
mod inherited_cubes;
mod locator;
pub use geometry::{
    EntityGeometry, EntityGeometryBounds, EntityGeometryInheritance, GEOMETRY_NO_HEAD_BOB_FIELD,
};
pub use locator::{EntityGeometryLocator, EntityGeometryLocatorBinding, MAX_GEOMETRY_LOCATORS};
mod source_paths;
#[path = "entity/texture_mesh.rs"]
mod texture_mesh;
#[path = "entity/v4.rs"]
mod v4;
pub use source_paths::{
    ACTOR_GLINT_TEXTURE_IDENTIFIER, BED_GEOMETRY_IDENTIFIER, CAPE_GEOMETRY_IDENTIFIER,
    ELYTRA_GEOMETRY_IDENTIFIER, GLINT_TEXTURE_IDENTIFIERS, ITEM_GLINT_TEXTURE_IDENTIFIER,
    LEGACY_ENTITY_GEOMETRY_PATH,
};
use source_paths::{validate_relative_path, validate_symbol_source};
pub use texture_mesh::{EntityGeometryTextureMesh, MAX_ENTITY_GEOMETRY_TEXTURE_MESHES};

use v4::validate_extended_payload;
#[allow(unused_imports)]
pub use v4::{
    CompiledMolangExpression, ENTITY_ALPHA_TEST_THRESHOLD, EntityAnimationChannel,
    EntityAnimationClip, EntityAnimationController, EntityAnimationInterpolation,
    EntityAnimationKeyframe, EntityAnimationLoop, EntityAnimationProperty, EntityAssetSummary,
    EntityControllerAnimation, EntityControllerAnimationTarget, EntityControllerState,
    EntityControllerTransition, EntityRenderCandidate, EntityRenderData, EntityRenderGeometry,
    EntityRenderLayer, EntityRenderMaterial, EntityRenderMaterialState, EntityRenderSlot,
    EntityRenderVisibility, EntityRigAnimationBinding, EntityRigBinding,
    EntityRigControllerBinding, EntityRigFallback, EntityRigGeometryBinding,
    MAX_ENTITY_ANIMATION_CHANNELS, MAX_ENTITY_ANIMATION_CLIPS, MAX_ENTITY_ANIMATION_KEYFRAMES,
    MAX_ENTITY_CONTROLLER_ANIMATIONS, MAX_ENTITY_CONTROLLER_NESTING, MAX_ENTITY_CONTROLLER_STATES,
    MAX_ENTITY_CONTROLLER_TRANSITIONS, MAX_ENTITY_CONTROLLERS, MAX_ENTITY_RENDER_CANDIDATES,
    MAX_ENTITY_RENDER_LAYERS, MAX_ENTITY_RENDER_PATTERN_BYTES, MAX_ENTITY_RENDER_SLOTS,
    MAX_ENTITY_RENDER_VISIBILITY, MAX_ENTITY_RIG_ANIMATIONS, MAX_ENTITY_RIG_BINDINGS,
    MAX_ENTITY_RIG_CONTROLLERS, MAX_ENTITY_RIG_GEOMETRIES, MAX_MOLANG_COLLECTION_ITEMS,
    MAX_MOLANG_COLLECTION_ITEMS_TOTAL, MAX_MOLANG_COLLECTIONS, MAX_MOLANG_EXPRESSIONS,
    MAX_MOLANG_LOOP_DEPTH, MAX_MOLANG_LOOP_ITERATIONS, MAX_MOLANG_OPS,
    MAX_MOLANG_OPS_PER_EXPRESSION, MAX_MOLANG_QUERY_ARGUMENTS, MAX_MOLANG_STACK_DEPTH,
    MAX_MOLANG_STRING_BYTES, MolangBranch, MolangCall, MolangCollection, MolangCollectionItem,
    MolangEaseCurve, MolangEaseMode, MolangFunction, MolangOp, MolangSymbol, MolangSymbolKind,
    entity_render_pattern_matches, molang_call, molang_program_stack,
};

pub const ENTITY_BLOB_MAGIC: [u8; 8] = *b"MCBEENT3";
/// Includes item-use, seat, interaction, rig-frame, intrinsic shine and geometry-locator facts;
/// stale carriers need `make assets`.
pub const ENTITY_BLOB_VERSION: u32 = 23;
/// Actor rig id ranges (a rig id is a `u32`):
/// - `0..PACK_RIG_ID_BASE`: vanilla catalog rig-geometry bindings, and from `0x2000_0000`
///   the catalog geometries render controllers draw beside the rig (`0x2000_0000 + index`).
/// - `PACK_RIG_ID_BASE..0x8000_0000`: the same two ranges for the session's server-pack catalog.
/// - `0x8000_0000..0xC000_0000`: equipment geometry, `0x8000_0000 + index`, where an index
///   below `PACK_EQUIPMENT_INDEX_BASE` is a vanilla entity-catalog geometry (skulls sit at
///   `0x00ff_0000`) and one at or above it is `PACK_EQUIPMENT_INDEX_BASE + ` a geometry index
///   of the session's server-pack catalog.
/// - `0xC000_0000..0xE000_0000`: generated item meshes.
/// - `0xE000_0000..`: player skins' own models; `u32::MAX` is the diagnostic rig.
pub const PACK_RIG_ID_BASE: u32 = 0x4000_0000;
/// First equipment geometry index that names a server-pack catalog geometry.
pub const PACK_EQUIPMENT_INDEX_BASE: u32 = 0x1000_0000;
pub const MAX_ENTITY_ASSET_SOURCES: usize = 8_192;
pub const MAX_ENTITY_ASSET_SYMBOLS: usize = 16_384;
pub const MAX_ENTITY_DEPENDENCIES: usize = 512;
pub const MAX_ENTITY_ASSET_PATH_BYTES: usize = 512;
pub const MAX_ENTITY_IDENTIFIER_BYTES: usize = 512;
pub const MAX_ENTITY_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ENTITY_TOTAL_SOURCE_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_ENTITY_CATALOG_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_ENTITY_GEOMETRY_BONES: usize = 2_048;
pub const MAX_ENTITY_GEOMETRY_CUBES: usize = 32_768;
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
    Attachable = 7,
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
    /// Equipment capabilities; non-entity symbols remain unresolved.
    pub mount_equipment: mount::MountEquipment,
    pub kind: EntityAssetKind,
    pub identifier: Box<str>,
    pub source_index: u32,
    pub dependencies: Box<[EntityDependency]>,
}

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct EntityGeometryScalar(u32);

impl EntityGeometryScalar {
    pub const ZERO: Self = Self(0);

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

/// One cube after its producer has resolved authored bone defaults and cube overrides.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometryCube {
    pub origin: [EntityGeometryScalar; 3],
    pub size: [EntityGeometryScalar; 3],
    pub pivot: [EntityGeometryScalar; 3],
    pub rotation: [EntityGeometryScalar; 3],
    pub uv: EntityGeometryUv,
    /// Resolved cube inflation; retained bone metadata must not be added again.
    pub inflate: EntityGeometryScalar,
    /// Final cube mirroring, including an explicit false override of a bone default.
    pub mirror: bool,
}

impl EntityGeometryCube {
    /// Native Geometry cube rotations use the uninflated box center when no pivot is authored.
    /// Vanilla bone parsing and the geometry 1.21 schema agree.
    #[must_use]
    pub fn default_rotation_pivot(
        origin: [EntityGeometryScalar; 3],
        size: [EntityGeometryScalar; 3],
    ) -> Option<[EntityGeometryScalar; 3]> {
        let center: [f32; 3] =
            std::array::from_fn(|axis| origin[axis].get() + size[axis].get() * 0.5);
        let [x, y, z] = center.map(EntityGeometryScalar::new);
        Some([x?, y?, z?])
    }

    /// Default UV sizes for north, south, east, west, up and down faces.
    #[must_use]
    pub fn face_uv_dimensions(&self) -> [[f32; 2]; 6] {
        let [x, y, z] = self.size.map(EntityGeometryScalar::get);
        [[x, y], [x, y], [z, y], [z, y], [x, z], [x, z]]
    }
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
    #[serde(default, skip_serializing_if = "<[EntityGeometryLocator]>::is_empty")]
    pub locators: Box<[EntityGeometryLocator]>,
    pub name: Box<str>,
    pub parent: Option<Box<str>>,
    pub pivot: Option<[EntityGeometryScalar; 3]>,
    pub rotation: Option<[EntityGeometryScalar; 3]>,
    /// Rotation baked into this part's cubes, independently of the animated bone frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_pose_rotation: Option<[EntityGeometryScalar; 3]>,
    /// Authored default metadata retained for sparse bone overlays; cubes are already resolved.
    pub mirror: Option<bool>,
    /// Authored default metadata retained for sparse bone overlays; cubes are already resolved.
    pub inflate: Option<EntityGeometryScalar>,
    pub never_render: Option<bool>,
    pub reset: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<Box<str>>,
    #[serde(
        default,
        skip_serializing_if = "<[EntityGeometryTextureMesh]>::is_empty"
    )]
    pub texture_meshes: Box<[EntityGeometryTextureMesh]>,
    pub cubes: Box<[EntityGeometryCube]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledEntityAssets {
    pub item_use_profiles: Box<[item_use::ItemUseDescriptor]>,
    pub seat_layouts: Box<[seats::EntitySeatDescriptor]>,
    pub source_manifest_sha256: [u8; 32],
    pub interaction_traits: Box<[interactions::EntityInteractionDescriptor]>,
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
    pub rig_geometries: Box<[EntityRigGeometryBinding]>,
    pub rig_animations: Box<[EntityRigAnimationBinding]>,
    pub rig_controllers: Box<[EntityRigControllerBinding]>,
    pub item_visuals: Box<[ItemVisualDefinition]>,
    pub item_visual_aliases: Box<[ItemVisualAlias]>,
    pub render: EntityRenderData,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EntityCatalogPayload {
    item_use_profiles: Box<[item_use::ItemUseDescriptor]>,
    seat_layouts: Box<[seats::EntitySeatDescriptor]>,
    interaction_traits: Box<[interactions::EntityInteractionDescriptor]>,
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
    rig_geometries: Box<[EntityRigGeometryBinding]>,
    rig_animations: Box<[EntityRigAnimationBinding]>,
    rig_controllers: Box<[EntityRigControllerBinding]>,
    item_visuals: Box<[ItemVisualDefinition]>,
    item_visual_aliases: Box<[ItemVisualAlias]>,
    render: EntityRenderData,
}

/// An encoded entity carrier payload.
pub type EntityCarrierBlob = Box<[u8]>;

impl CompiledEntityAssets {
    /// Rebuilds the identity-keyed seat and interaction entries as unresolved from `symbols`.
    pub fn with_unresolved_admissions(self) -> Self {
        self.with_unresolved_seats().with_unresolved_interactions()
    }
}

mod attachable;
mod retained;
mod runtime;
pub use runtime::RuntimeEntityAssets;

pub fn encode_entity_blob(compiled: &CompiledEntityAssets) -> Result<Box<[u8]>, AssetError> {
    v4::encode_compiled(compiled)
}

fn validate_compiled(compiled: &CompiledEntityAssets) -> Result<Box<[Option<usize>]>, AssetError> {
    if compiled.source_manifest_sha256 == [0; 32]
        || compiled.sources.is_empty()
        || compiled.sources.len() > MAX_ENTITY_ASSET_SOURCES
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
    let parents = validate_geometries(compiled)?;
    mount::validate_mount_equipment(&compiled.sources, &compiled.symbols)?;
    validate_extended_payload(compiled)?;
    Ok(parents)
}

fn validate_geometries(
    compiled: &CompiledEntityAssets,
) -> Result<Box<[Option<usize>]>, AssetError> {
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
        if let Some(bounds) = geometry.visible_bounds {
            bounds.validate()?;
        }
        validate_geometry_bones(&geometry.bones, geometry.inherits.is_some())?;
        previous = Some(key);
    }
    validate_entity_geometry_inheritance(&compiled.geometries)
}

/// Validates catalog inheritance selection, inherited bone parents and locator limits.
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
        let mut locator_names = std::collections::BTreeSet::new();
        for index in chain.into_iter().rev() {
            for bone in &geometries[index].bones {
                for locator in &bone.locators {
                    if locator_names.len() >= MAX_GEOMETRY_LOCATORS
                        && !locator_names.contains(locator.name.as_ref())
                    {
                        return Err(invalid("inherited geometry locator count exceeds bound"));
                    }
                    locator_names.insert(locator.name.as_ref());
                }
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
    locator::validate_total(bones)?;
    let mut total_cubes = 0usize;
    let mut total_texture_meshes = 0usize;
    for bone in bones {
        total_texture_meshes = total_texture_meshes
            .checked_add(bone.texture_meshes.len())
            .filter(|count| *count <= MAX_ENTITY_GEOMETRY_TEXTURE_MESHES)
            .ok_or_else(|| invalid("entity geometry texture mesh count exceeds bound"))?;
        texture_mesh::validate(bone)?;
        locator::validate(bone)?;
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
        if let Some(rotation) = &bone.bind_pose_rotation {
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
            geometry::validate_uv(&cube.uv)?;
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

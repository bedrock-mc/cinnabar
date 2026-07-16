//! Bounded Bedrock resource-pack source readers.

mod atmosphere;
mod biome;
mod blob;
mod compiled;
mod entity;
mod error;
mod light_registry;
mod model;
mod registry;
mod runtime;
mod texture;

pub use atmosphere::{
    ATMOSPHERE_BLOB_MAGIC, ATMOSPHERE_BLOB_VERSION, AtmosphereRole, AtmosphereTexture,
    BiomeVisualProfile, CelestialBorderTexel, CelestialTile, CompiledAtmosphereAssets, FogDistance,
    FogDistanceMode, FogMedium, FogProfile, MAX_ENVIRONMENT_IDENTIFIER_BYTES,
    MAX_ENVIRONMENT_PROFILES, MAX_FOG_DISTANCES, ResolvedFog, RuntimeAtmosphereAssets,
    composite_celestial, encode_atmosphere_blob,
};
pub use biome::{
    BIOME_REGISTRY_MAGIC, BIOME_RULE_FLAG_GRASS_SHADED, BiomeRegistryRecord, BiomeRule,
    CompiledBiomeAssets, LinearBiomeTints, LiveBiomeDefinition, MAX_BIOME_NAME_BYTES,
    MAX_BIOME_NAMES_BYTES, MAX_BIOME_RULES, MISSING_BIOME_DENSE_INDEX, RAW_BIOME_ID_COUNT,
    ResolvedBiomeTints, TINT_MAP_BYTES, TINT_MAP_COUNT, TINT_MAP_SIZE, TintMapId, TintSource,
    colormap_coordinate, read_biome_registry,
};
pub use blob::{BLOB_MAGIC, BLOB_VERSION, encode_blob, write_blob_atomic};
pub use compiled::{
    BlockFace, BlockVisual, CompiledAssets, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND,
    MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_DRY_FOLIAGE,
    MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_CLASS_MASK, MATERIAL_FLAG_FOLIAGE_TINT,
    MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_OVERLAY_MASK,
    MATERIAL_FLAG_ROTATE_UV, MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_UV_MASK,
    MATERIAL_FLAG_WATER_TINT, MATERIAL_FLAGS_MASK, MAX_MATERIALS, MAX_TEXTURE_LAYERS, Material,
};
pub use entity::{
    CompiledEntityAssets, ENTITY_BLOB_MAGIC, ENTITY_BLOB_VERSION, EntityAssetKind,
    EntityAssetSource, EntityAssetSymbol, EntityDependency, EntityDependencyKind,
    EntityDependencyResolution, EntityGeometry, EntityGeometryBone, EntityGeometryCube,
    EntityGeometryFaceUv, EntityGeometryFaceUvs, EntityGeometryInheritance, EntityGeometryScalar,
    EntityGeometryUv, MAX_ENTITY_ASSET_PATH_BYTES, MAX_ENTITY_ASSET_SOURCES,
    MAX_ENTITY_ASSET_SYMBOLS, MAX_ENTITY_CATALOG_BYTES, MAX_ENTITY_DEPENDENCIES,
    MAX_ENTITY_GEOMETRIES, MAX_ENTITY_GEOMETRY_BONES, MAX_ENTITY_GEOMETRY_CUBES,
    MAX_ENTITY_GEOMETRY_NAME_BYTES, MAX_ENTITY_GEOMETRY_SCALAR, MAX_ENTITY_IDENTIFIER_BYTES,
    MAX_ENTITY_SOURCE_BYTES, MAX_ENTITY_TEXTURE_DIMENSION, MAX_ENTITY_TOTAL_SOURCE_BYTES,
    RuntimeEntityAssets, encode_entity_blob,
};
pub use error::AssetError;
pub use light_registry::{LightProperties, read_light_registry};
pub use model::{
    ANIMATION_FLAG_BLEND, Animation, MAX_ANIMATION_FRAMES, MAX_ANIMATIONS, MAX_MODEL_QUADS,
    MAX_MODEL_TEMPLATES, MAX_TEXTURE_PAGES, MODEL_QUAD_FLAG_CULL_FACE_MASK,
    MODEL_QUAD_FLAG_FACE_MASK, MODEL_QUAD_FLAG_TWO_SIDED, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
    MODEL_TEMPLATE_FLAG_FENCE_NETHER, MODEL_TEMPLATE_FLAG_FENCE_WOOD,
    MODEL_TEMPLATE_FLAG_GATE_AXIS_X, MODEL_TEMPLATE_FLAG_GATE_AXIS_Z, MODEL_TEMPLATE_FLAG_KELP,
    MODEL_TEMPLATE_FLAG_PANE, MODEL_TEMPLATE_FLAG_STAIR, MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE,
    MODEL_TEMPLATE_FLAG_WALL, ModelQuad, ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE,
    TexturePage, TextureRef, VisualKind,
};
pub use registry::{
    BlockFlags, CollisionBox, CollisionConfidence, CollisionSeed, ContributorRole, ModelFamily,
    ModelState, ModelStateField, RegistryProvenance, RegistryRecord, read_registry,
};
pub use runtime::{NetworkIdMode, ResolvedBlock, ResolvedFace, RuntimeAssets};
pub use texture::{MIP_COUNT, TILE_SIZE, TextureArray, TextureMip};

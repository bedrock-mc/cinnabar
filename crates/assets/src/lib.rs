//! Bounded Bedrock resource-pack source readers.

mod animation;
mod atmosphere;
mod biome;
mod blob;
mod compiler;
mod error;
mod image;
mod light_registry;
mod model;
mod pack;
mod registry;
mod runtime;

pub use animation::AnimationInventory;
pub use atmosphere::{
    ATMOSPHERE_BLOB_MAGIC, ATMOSPHERE_BLOB_VERSION, AtmosphereRole, AtmosphereTexture,
    CompiledAtmosphereAssets, RuntimeAtmosphereAssets, compile_atmosphere_assets,
    encode_atmosphere_blob,
};
pub use biome::{
    BIOME_REGISTRY_MAGIC, BIOME_RULE_FLAG_GRASS_SHADED, BiomeRegistryRecord, BiomeRule,
    CompiledBiomeAssets, LinearBiomeTints, LiveBiomeDefinition, MAX_BIOME_NAME_BYTES,
    MAX_BIOME_NAMES_BYTES, MAX_BIOME_RULES, MISSING_BIOME_DENSE_INDEX, RAW_BIOME_ID_COUNT,
    ResolvedBiomeTints, TINT_MAP_BYTES, TINT_MAP_COUNT, TINT_MAP_SIZE, TintMapId, TintSource,
    colormap_coordinate, compile_biome_assets, read_biome_registry,
};
pub use blob::{BLOB_MAGIC, BLOB_VERSION, encode_blob, write_blob_atomic};
pub use compiler::{
    BlockVisual, CompiledAssets, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND,
    MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_DRY_FOLIAGE,
    MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_CLASS_MASK, MATERIAL_FLAG_FOLIAGE_TINT,
    MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_OVERLAY_MASK,
    MATERIAL_FLAG_ROTATE_UV, MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_UV_MASK,
    MATERIAL_FLAG_WATER_TINT, MATERIAL_FLAGS_MASK, MAX_MATERIALS, MAX_TEXTURE_LAYERS, Material,
    compile_pack, compile_pack_with_biomes, inspect_animation_inventory,
};
pub use error::AssetError;
pub use image::{MIP_COUNT, TILE_SIZE, TextureArray, TextureMip};
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
pub use pack::{
    BlockFace, BlockTextureMap, FlipbookSource, MAX_FLIPBOOK_FRAMES, MAX_FLIPBOOKS, PackSources,
    TerrainTextureMap, TextureKey, read_pack, resolve_texture_key,
};
pub use registry::{
    BlockFlags, CollisionBox, CollisionConfidence, CollisionSeed, ContributorRole, ModelFamily,
    ModelState, ModelStateField, RegistryProvenance, RegistryRecord, read_registry,
};
pub use runtime::{NetworkIdMode, ResolvedBlock, ResolvedFace, RuntimeAssets};

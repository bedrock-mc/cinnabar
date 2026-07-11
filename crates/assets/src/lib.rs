//! Bounded Bedrock resource-pack source readers.

mod biome;
mod blob;
mod compiler;
mod error;
mod image;
mod pack;
mod registry;
mod runtime;

pub use biome::{
    BIOME_REGISTRY_MAGIC, BIOME_RULE_FLAG_GRASS_SHADED, BiomeRegistryRecord, BiomeRule,
    CompiledBiomeAssets, LinearBiomeTints, LiveBiomeDefinition, MAX_BIOME_NAME_BYTES,
    MAX_BIOME_NAMES_BYTES, MAX_BIOME_RULES, MISSING_BIOME_DENSE_INDEX, RAW_BIOME_ID_COUNT,
    ResolvedBiomeTints, TINT_MAP_BYTES, TINT_MAP_COUNT, TINT_MAP_SIZE, TintMapId, TintSource,
    colormap_coordinate, compile_biome_assets, read_biome_registry,
};
pub use blob::{BLOB_MAGIC, BLOB_VERSION, encode_blob, write_blob_atomic};
pub use compiler::{
    BlockVisual, CompiledAssets, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_CUTOUT,
    MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_DRY_FOLIAGE, MATERIAL_FLAG_EVERGREEN_FOLIAGE,
    MATERIAL_FLAG_FOLIAGE_CLASS_MASK, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT,
    MATERIAL_FLAG_OVERLAY_MASK, MATERIAL_FLAG_ROTATE_UV, MATERIAL_FLAG_TINT_MASK,
    MATERIAL_FLAG_UV_MASK, MATERIAL_FLAG_WATER_TINT, MATERIAL_FLAGS_MASK, MAX_MATERIALS,
    MAX_TEXTURE_LAYERS, Material, compile_pack, compile_pack_with_biomes,
};
pub use error::AssetError;
pub use image::{MIP_COUNT, TILE_SIZE, TextureArray, TextureMip};
pub use pack::{
    BlockFace, BlockTextureMap, FlipbookSource, PackSources, TerrainTextureMap, TextureKey,
    read_pack, resolve_texture_key,
};
pub use registry::{BlockFlags, RegistryRecord, read_registry};
pub use runtime::{NetworkIdMode, ResolvedBlock, ResolvedFace, RuntimeAssets};

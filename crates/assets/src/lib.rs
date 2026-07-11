//! Bounded Bedrock resource-pack source readers.

mod blob;
mod compiler;
mod error;
mod image;
mod pack;
mod registry;
mod runtime;

pub use blob::{BLOB_MAGIC, BLOB_VERSION, encode_blob, write_blob_atomic};
pub use compiler::{
    BlockVisual, CompiledAssets, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_CUTOUT,
    MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_OVERLAY_MASK, MATERIAL_FLAG_ROTATE_UV,
    MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_UV_MASK, MATERIAL_FLAGS_MASK, MAX_MATERIALS,
    MAX_TEXTURE_LAYERS, Material, compile_pack,
};
pub use error::AssetError;
pub use image::{MIP_COUNT, TILE_SIZE, TextureArray, TextureMip};
pub use pack::{
    BlockFace, BlockTextureMap, FlipbookSource, PackSources, TerrainTextureMap, TextureKey,
    read_pack, resolve_texture_key,
};
pub use registry::{BlockFlags, RegistryRecord, read_registry};
pub use runtime::{NetworkIdMode, ResolvedBlock, ResolvedFace, RuntimeAssets};

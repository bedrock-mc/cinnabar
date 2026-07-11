//! Bounded Bedrock resource-pack source readers.

mod error;
mod pack;
mod registry;

pub use error::AssetError;
pub use pack::{
    BlockFace, BlockTextureMap, FlipbookSource, PackSources, TerrainTextureMap, TextureKey,
    read_pack, resolve_texture_key,
};
pub use registry::{BlockFlags, RegistryRecord, read_registry};

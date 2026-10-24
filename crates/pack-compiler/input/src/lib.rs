//! Bounded pack inputs, source validation and shared texture decoding.

pub mod barrel;
pub mod geometry_key;
pub mod image;
pub mod json;
pub mod lightning_rod;
pub mod pack;
pub mod respawn_anchor;
pub mod source;
pub mod texture_path;
pub use texture_path::static_texture_path;

pub use pack::{
    BlockTextureMap, MAX_FLIPBOOKS, PackSources, TerrainTextureMap, TextureKey, read_pack,
    resolve_texture_key,
};
pub use pack::{apply_atlas_tint, parse_atlas_tint};

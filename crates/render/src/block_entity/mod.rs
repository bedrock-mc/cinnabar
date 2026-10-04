//! Block-entity rendering: CPU-built model and overlay vertices plus a small wgpu pass.
//!
//! Models are authored in code against the carrier's atlas; break-crack overlays and
//! sign text share the same vertex path. Geometry the pinned pack does not define is
//! marked provisional in its module and needs native measurement.

mod atlas;
mod banner;
mod beam;
mod bed;
mod bell;
mod book;
mod chest;
mod conduit;
mod crack;
mod crystal_beam;
mod frame;
mod gpu;
mod heads;
mod items;
mod mesh;
mod mob;
mod portal;
mod pot;
mod scene;
mod selection;
mod shulker;
mod sign;
mod skull;
mod spawner;
mod statue;

pub use atlas::{AtlasRect, BlockEntityAtlas, TEXT_CELL, TEXT_SLOT_COUNT, TextureRef};
pub use banner::{
    BannerLayer, BannerModel, BannerMount, MAX_BANNER_LAYERS, banner_color, pattern_texture,
};
pub use beam::BeaconModel;
pub use bed::{BedModel, bed_color};
pub use bell::{BellAttachment, BellModel, swing_degrees};
pub use chest::{ChestModel, ChestPair, ChestVariant, CopperAge, lid_angle_radians};
pub use conduit::ConduitModel;
pub use crack::{CrackQuad, CrackShape, crack_shape_from_template, crack_texture_name};
pub use crystal_beam::CrystalBeamModel;
pub use frame::{ItemFrameModel, item_frame_item_transform};
pub use gpu::BlockEntityRenderPlugin;
pub use items::{StaticItemPlacement, StaticItemPlacements, matrix_rows};
pub use mesh::{
    BLOCK_ENTITY_VERTEX_WORDS, BlockEntityVertex, Facing, MAX_BLOCK_ENTITY_VERTICES,
    model_matrix as block_matrix,
};
pub use mob::SPAWNER_MOBS;
pub use pot::{DecoratedPotModel, sherd_pattern};
pub use scene::{
    BlockEntityAtlasImage, BlockEntityFrame, BlockEntityKind, BlockEntityLight, BlockEntityScene,
    BlockEntitySubmission, CrackInstance, SceneClock,
};
pub use selection::{BlockSelectionFrame, BlockSelectionTarget};
pub use shulker::{ShulkerModel, shulker_color_from_block_name};
pub use sign::{SignFace, SignModel, SignMount};
pub use skull::{SkullKind, SkullModel, SkullMount, floor_yaw_degrees};
pub use spawner::SpawnerModel;
pub use statue::{Oxidation, StatueModel, StatuePose};

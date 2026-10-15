//! Far-terrain level of detail from terrain the server sent: per-column surface summaries,
//! their caches and the bounds that govern them. Bevy-free; rendering lives in `render`.

pub mod config;

mod budget;
mod cache;
mod colours;
mod disk;
mod extract;
mod identity;
mod publication;
mod region;
mod store;
mod summary;
mod surface_slice;
mod tile_coverage;
mod tile_mesh;
mod tile_set;
mod tiles;

pub use budget::SummaryBudget;
pub use cache::{FarTerrainCache, RetiringSession};
pub use disk::{DiskBounds, DiskCache, DiskEvent};
pub use identity::{NamespaceKey, ServerKey, WorldKey, surface_similarity};
pub use publication::{TerrainPublication, TerrainTarget};
pub use region::{
    REGION_SIDE, RegionDecodeError, RegionPos, decode_region, encode_region, region_index,
};

pub use colours::SurfaceColours;
pub use config::FarTerrainConfig;
pub use extract::{ColumnSnapshot, SurfaceContext, SurfaceScan, summarise_column};
pub use store::{Admission, ColumnPos, SummaryStore, Versioned};
pub use summary::{
    COLUMN_CELLS, COLUMN_SIDE, ColumnSummary, EncodedSummary, SummaryDecodeError, SurfaceCell,
};
pub use surface_slice::SurfaceSlice;
pub use tile_coverage::TileCoverage;
pub use tile_mesh::{
    FACE_NEGATIVE_X, FACE_NEGATIVE_Z, FACE_POSITIVE_X, FACE_POSITIVE_Z, FACE_UP, TileGrid,
    TileMesh, mesh_tile,
};
pub use tile_set::FarTiles;
pub use tiles::{NearMask, TileKey, TileView, select_tiles};

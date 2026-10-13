//! Far-terrain level of detail from terrain the server sent: per-column surface summaries,
//! their caches and the bounds that govern them. Bevy-free; rendering lives in `render`.

pub mod config;

mod colours;
mod extract;
mod store;
mod summary;

pub use colours::SurfaceColours;
pub use config::FarTerrainConfig;
pub use extract::{ColumnSnapshot, SurfaceContext, summarise_column};
pub use store::{Admission, ColumnPos, SummaryStore};
pub use summary::{
    COLUMN_CELLS, COLUMN_SIDE, ColumnSummary, EncodedSummary, SummaryDecodeError, SurfaceCell,
};

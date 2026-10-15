//! Summary evidence retained with a tile, independently of its generated quads.

use crate::{COLUMN_SIDE, ColumnPos, TileKey};

/// Columns decoded successfully for one immutable tile build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TileCoverage {
    /// Every interior column has a valid summary, including any known empty columns.
    Complete,
    /// Missing and invalid summaries provide no coverage evidence.
    Partial(Box<[ColumnPos]>),
}

impl TileCoverage {
    /// Records only valid interior summaries; halo columns cannot prove interior coverage.
    #[must_use]
    pub fn from_columns(key: TileKey, columns: impl IntoIterator<Item = ColumnPos>) -> Self {
        let mut columns: Vec<_> = columns
            .into_iter()
            .filter(|pos| contains(key, *pos))
            .collect();
        columns.sort_unstable();
        columns.dedup();
        let side = key.width_blocks() as usize / COLUMN_SIDE;
        if columns.len() == side * side {
            Self::Complete
        } else {
            Self::Partial(columns.into_boxed_slice())
        }
    }

    /// Whether this tile build has decoded evidence for the specified interior column.
    #[must_use]
    pub fn covers(&self, key: TileKey, column: ColumnPos) -> bool {
        contains(key, column)
            && match self {
                Self::Complete => true,
                Self::Partial(columns) => columns.binary_search(&column).is_ok(),
            }
    }
}

/// Uses the tile's own mapping to reject columns outside its interior.
fn contains(key: TileKey, column: ColumnPos) -> bool {
    key == TileKey::containing(
        key.level,
        column.x * COLUMN_SIDE as i32,
        column.z * COLUMN_SIDE as i32,
    )
}

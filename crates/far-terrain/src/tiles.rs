//! Which tiles cover the far ring and at which level, and which columns near terrain owns.

use crate::{
    ColumnPos,
    config::{EDGE_MARGIN_BLOCKS, LEVEL_COUNT, LEVEL_SPLIT_DISTANCE, TILE_CELLS},
};

const COLUMN_BLOCKS: i32 = 16;

/// A square of `TILE_CELLS` cells, each `2^level` blocks wide, indexed in tile widths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    pub level: u8,
    pub x: i32,
    pub z: i32,
}

impl TileKey {
    #[must_use]
    pub const fn cell_blocks(self) -> i32 {
        1 << self.level
    }

    #[must_use]
    pub const fn width_blocks(self) -> i32 {
        (TILE_CELLS as i32) << self.level
    }

    /// The block coordinates of the tile's minimum corner.
    #[must_use]
    pub const fn origin(self) -> [i32; 2] {
        [self.x * self.width_blocks(), self.z * self.width_blocks()]
    }

    /// Whether two tile squares share any horizontal area.
    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        let ([ax, az], aw) = (self.origin(), self.width_blocks());
        let ([bx, bz], bw) = (other.origin(), other.width_blocks());
        ax < bx + bw && bx < ax + aw && az < bz + bw && bz < az + aw
    }

    /// The tile at `level` holding block `x`, `z`.
    #[must_use]
    pub const fn containing(level: u8, x: i32, z: i32) -> Self {
        let width = (TILE_CELLS as i32) << level;
        Self {
            level,
            x: x.div_euclid(width),
            z: z.div_euclid(width),
        }
    }

    /// Every column the tile covers, row by row.
    pub fn columns(self) -> impl Iterator<Item = ColumnPos> {
        let [x, z] = self.origin().map(|block| block.div_euclid(COLUMN_BLOCKS));
        let side = self.width_blocks() / COLUMN_BLOCKS;
        (0..side).flat_map(move |dz| (0..side).map(move |dx| ColumnPos::new(x + dx, z + dz)))
    }

    fn children(self) -> [Self; 4] {
        let level = self.level - 1;
        let (x, z) = (self.x * 2, self.z * 2);
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dz)| Self {
            level,
            x: x + dx,
            z: z + dz,
        })
    }

    /// Horizontal distances from `point` to the nearest and farthest points of the tile.
    pub(crate) fn distances(self, point: [f32; 2]) -> (f32, f32) {
        let [x0, z0] = self.origin().map(|block| block as f32);
        let width = self.width_blocks() as f32;
        let near = |p: f32, low: f32| (low - p).max(p - (low + width)).max(0.0);
        let far = |p: f32, low: f32| (p - low).abs().max((low + width - p).abs());
        (
            near(point[0], x0).hypot(near(point[1], z0)),
            far(point[0], x0).hypot(far(point[1], z0)),
        )
    }
}

/// Where the far ring lies for one camera, in blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileView {
    /// Horizontal camera position.
    pub centre: [f32; 2],
    /// Distance at which near terrain may end; only the finest level reaches inside it.
    pub near_blocks: f32,
    /// Tiles wholly inside this distance are left out; near terrain draws everything there.
    pub skip_blocks: f32,
    /// Reach of far terrain beyond the near edge.
    pub far_blocks: f32,
}

impl TileView {
    /// The view around the `centre` column for a mask built by [`NearMask::around_with_empty`] with
    /// `near_radius`: finest tiles only out to [`EDGE_MARGIN_BLOCKS`] past near geometry,
    /// so nothing coarse is drawn close to the player; tiles wholly inside the masked
    /// disc short of that margin, and never past the near radius, left out; far terrain out to
    /// `horizon_chunks`.
    #[must_use]
    pub fn around(
        centre: [i32; 2],
        near: &NearMask,
        near_radius: i32,
        horizon_chunks: i32,
    ) -> Self {
        let near_blocks = near.outer_blocks() + EDGE_MARGIN_BLOCKS;
        let skip = near.inner_blocks().min((near_radius.max(0) * 16) as f32);
        Self {
            centre: [centre[0] as f32 * 16.0 + 8.0, centre[1] as f32 * 16.0 + 8.0],
            near_blocks,
            skip_blocks: (skip - EDGE_MARGIN_BLOCKS).max(0.0),
            far_blocks: ((horizon_chunks * 16) as f32 - near_blocks).max(0.0),
        }
    }
}

/// Tiles covering the ring between the near edge and the far reach, finer toward the camera:
/// a tile splits while it is within [`LEVEL_SPLIT_DISTANCE`] widths of the near edge, so only
/// the finest level ever borders near terrain.
#[must_use]
pub fn select_tiles(view: TileView) -> Vec<TileKey> {
    let top = LEVEL_COUNT - 1;
    let reach = view.near_blocks + view.far_blocks;
    let low = TileKey::containing(
        top,
        (view.centre[0] - reach) as i32,
        (view.centre[1] - reach) as i32,
    );
    let high = TileKey::containing(
        top,
        (view.centre[0] + reach) as i32,
        (view.centre[1] + reach) as i32,
    );
    let mut selected = Vec::new();
    let mut pending: Vec<TileKey> = (low.z..=high.z)
        .flat_map(|z| (low.x..=high.x).map(move |x| TileKey { level: top, x, z }))
        .collect();
    while let Some(tile) = pending.pop() {
        let (nearest, farthest) = tile.distances(view.centre);
        if nearest > reach || farthest < view.skip_blocks {
            continue;
        }
        let split = view.near_blocks + LEVEL_SPLIT_DISTANCE * tile.width_blocks() as f32;
        if tile.level > 0 && nearest < split {
            pending.extend(tile.children());
        } else {
            selected.push(tile);
        }
    }
    selected.sort_unstable();
    selected
}

/// Near geometry and delivered air that hide far terrain, with a separate geometry bound.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NearMask {
    origin: [i32; 2],
    side: u32,
    bits: Box<[u32]>,
    /// Squared column distance from the centre to the nearest column that is not near.
    hole_squared: i64,
    /// Squared column distance to the farthest geometry column, excluding delivered air.
    outer_squared: Option<i64>,
}

/// Half a column's diagonal, rounded up: how far a column's blocks reach from its middle.
const HALF_COLUMN_DIAGONAL_BLOCKS: f32 = 12.0;

impl NearMask {
    /// Every column within `near_radius` of the `centre` column, plus the `drawn` columns past
    /// it, all within `limit`. Near terrain owns that disc however far its meshes have loaded,
    /// so far terrain never stands in there; past it far terrain hides only under columns
    /// near terrain draws, such as those handing over to it.
    #[must_use]
    pub fn around(
        centre: [i32; 2],
        limit: i32,
        near_radius: i32,
        drawn: impl IntoIterator<Item = ColumnPos>,
    ) -> Self {
        Self::around_with_empty(centre, limit, near_radius, drawn, [])
    }

    /// Masks the near disc, published geometry and delivered air within `limit`. Only the
    /// disc and geometry extend the outer bound used to choose tile detail.
    #[must_use]
    pub fn around_with_empty(
        centre: [i32; 2],
        limit: i32,
        near_radius: i32,
        drawn: impl IntoIterator<Item = ColumnPos>,
        empty: impl IntoIterator<Item = ColumnPos>,
    ) -> Self {
        let radius = near_radius.clamp(0, limit.max(0));
        let reach = i64::from(radius).pow(2);
        let disc = (-radius..=radius).flat_map(move |dz| {
            (-radius..=radius)
                .filter(move |dx| i64::from(*dx).pow(2) + i64::from(dz).pow(2) <= reach)
                .map(move |dx| ColumnPos::new(centre[0] + dx, centre[1] + dz))
        });
        Self::from_coverage(
            centre,
            limit,
            disc.chain(drawn)
                .map(|column| (column, true))
                .chain(empty.into_iter().map(|column| (column, false))),
        )
    }

    /// Exactly the `columns` within `limit` columns of the `centre` column; farther ones, such
    /// as those left behind by a teleport and not yet evicted, are left out.
    #[must_use]
    pub fn from_columns(
        centre: [i32; 2],
        limit: i32,
        columns: impl IntoIterator<Item = ColumnPos>,
    ) -> Self {
        Self::from_coverage(
            centre,
            limit,
            columns.into_iter().map(|column| (column, true)),
        )
    }

    /// Builds one coverage bitset while bounding only entries marked as geometry.
    fn from_coverage(
        centre: [i32; 2],
        limit: i32,
        columns: impl IntoIterator<Item = (ColumnPos, bool)>,
    ) -> Self {
        let limit = i64::from(limit.max(0));
        let columns: Vec<([i32; 2], bool)> = columns
            .into_iter()
            .map(|(column, geometry)| {
                (
                    [
                        i64::from(column.x) - i64::from(centre[0]),
                        i64::from(column.z) - i64::from(centre[1]),
                    ],
                    geometry,
                )
            })
            .filter(|([dx, dz], _)| dx.abs() <= limit && dz.abs() <= limit)
            .map(|(offset, geometry)| (offset.map(|delta| delta as i32), geometry))
            .collect();
        let reach = columns
            .iter()
            .map(|([dx, dz], _)| dx.abs().max(dz.abs()))
            .max()
            .unwrap_or(0);
        let side = (2 * reach + 1) as u32;
        let origin = [centre[0] - reach, centre[1] - reach];
        let mut bits = vec![0_u32; (side * side).div_ceil(32) as usize].into_boxed_slice();
        let mut outer_squared = None;
        for ([dx, dz], geometry) in &columns {
            let index = ((dz + reach) as u32 * side + (dx + reach) as u32) as usize;
            bits[index / 32] |= 1 << (index % 32);
            if *geometry {
                outer_squared = outer_squared.max(Some(squared(*dx, *dz)));
            }
        }
        // The nearest column that is not near; past the square every column is not.
        let mut hole_squared = squared(reach + 1, 0);
        for index in 0..(side * side) as usize {
            if bits[index / 32] & (1 << (index % 32)) == 0 {
                let (dx, dz) = (
                    index as i32 % side as i32 - reach,
                    index as i32 / side as i32 - reach,
                );
                hole_squared = hole_squared.min(squared(dx, dz));
            }
        }
        Self {
            origin,
            side,
            bits,
            hole_squared,
            outer_squared,
        }
    }

    /// Distance from the centre column's middle within which every block is near terrain.
    #[must_use]
    pub fn inner_blocks(&self) -> f32 {
        (16.0 * (self.hole_squared as f32).sqrt() - HALF_COLUMN_DIAGONAL_BLOCKS).max(0.0)
    }

    /// Distance beyond which no block belongs to near geometry; air coverage may reach farther.
    #[must_use]
    pub fn outer_blocks(&self) -> f32 {
        self.outer_squared.map_or(0.0, |outer| {
            16.0 * (outer as f32).sqrt() + HALF_COLUMN_DIAGONAL_BLOCKS
        })
    }

    #[must_use]
    pub fn contains(&self, column: ColumnPos) -> bool {
        let (dx, dz) = (
            i64::from(column.x) - i64::from(self.origin[0]),
            i64::from(column.z) - i64::from(self.origin[1]),
        );
        let side = i64::from(self.side);
        if dx < 0 || dz < 0 || dx >= side || dz >= side {
            return false;
        }
        let index = (dz * side + dx) as usize;
        self.bits[index / 32] & (1 << (index % 32)) != 0
    }

    /// Column coordinates of bit zero.
    #[must_use]
    pub const fn origin(&self) -> [i32; 2] {
        self.origin
    }

    /// Columns per row; bit `z * side + x` is set for a near column.
    #[must_use]
    pub const fn side(&self) -> u32 {
        self.side
    }

    #[must_use]
    pub fn words(&self) -> &[u32] {
        &self.bits
    }
}

fn squared(dx: i32, dz: i32) -> i64 {
    i64::from(dx).pow(2) + i64::from(dz).pow(2)
}

//! Which tiles cover the far ring and at which level, and which columns near terrain owns.

use crate::{
    ColumnPos,
    config::{LEVEL_COUNT, LEVEL_SPLIT_DISTANCE, TILE_CELLS},
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
    /// Tiles wholly inside this distance are left out; near terrain always draws there.
    pub skip_blocks: f32,
    /// Reach of far terrain beyond the near edge.
    pub far_blocks: f32,
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

/// Columns near terrain owns around the player; far terrain hides itself there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NearMask {
    origin: [i32; 2],
    side: u32,
    bits: Box<[u32]>,
}

impl NearMask {
    /// The columns the client's chunk grid retains for `radius` around `centre`.
    #[must_use]
    pub fn retained(centre: [i32; 2], radius: i32) -> Self {
        let reach = world::chunk_view_distance(radius) as i32 + 1;
        let side = (2 * reach + 1) as u32;
        let origin = [centre[0] - reach, centre[1] - reach];
        let mut bits = vec![0_u32; (side * side).div_ceil(32) as usize].into_boxed_slice();
        for dz in 0..side as i32 {
            for dx in 0..side as i32 {
                let column = [origin[0] + dx, origin[1] + dz];
                if world::chunk_in_view(radius, column, centre) {
                    let index = (dz as u32 * side + dx as u32) as usize;
                    bits[index / 32] |= 1 << (index % 32);
                }
            }
        }
        Self { origin, side, bits }
    }

    /// Exactly `columns`, which must lie within `reach` columns of `centre`; others are dropped.
    #[must_use]
    pub fn from_columns(
        centre: [i32; 2],
        reach: i32,
        columns: impl IntoIterator<Item = ColumnPos>,
    ) -> Self {
        let side = (2 * reach + 1) as u32;
        let origin = [centre[0] - reach, centre[1] - reach];
        let mut bits = vec![0_u32; (side * side).div_ceil(32) as usize].into_boxed_slice();
        for column in columns {
            let (dx, dz) = (column.x - origin[0], column.z - origin[1]);
            if (0..side as i32).contains(&dx) && (0..side as i32).contains(&dz) {
                let index = (dz as u32 * side + dx as u32) as usize;
                bits[index / 32] |= 1 << (index % 32);
            }
        }
        Self { origin, side, bits }
    }

    #[must_use]
    pub fn contains(&self, column: ColumnPos) -> bool {
        let (dx, dz) = (column.x - self.origin[0], column.z - self.origin[1]);
        let side = self.side as i32;
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

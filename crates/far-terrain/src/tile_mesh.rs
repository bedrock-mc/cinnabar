//! Downsamples column summaries into a tile's cells and meshes them as blocky terrain.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    COLUMN_SIDE, ColumnPos, ColumnSummary, SurfaceCell, TileKey,
    config::{SKIRT_DEPTH_BLOCKS, TILE_CELLS},
};

/// Quad faces as packed into bits 28..31 of a quad's first word.
pub const FACE_UP: u32 = 0;
pub const FACE_POSITIVE_X: u32 = 1;
pub const FACE_NEGATIVE_X: u32 = 2;
pub const FACE_POSITIVE_Z: u32 = 3;
pub const FACE_NEGATIVE_Z: u32 = 4;

const CELLS: usize = TILE_CELLS * TILE_CELLS;
const EMPTY: i16 = SurfaceCell::EMPTY_HEIGHT;

/// One tile's cells: the summarised surface downsampled to the tile's cell width.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileGrid {
    key: TileKey,
    cells: Box<[SurfaceCell]>,
    /// Lowest surface among every column given, those just outside the tile included, so
    /// edge skirts reach down to whatever a neighbouring tile shows.
    lowest: i16,
}

impl TileGrid {
    /// Aggregates every summarised block of `columns` inside the tile. A cell's height leans
    /// toward its highest block, so ridges survive coarse levels; a cell is water, at the
    /// water's mean surface, when most of its blocks are; colour is the mean. Columns outside
    /// the tile only lower its skirts.
    #[must_use]
    pub fn build(key: TileKey, columns: &[(ColumnPos, ColumnSummary)]) -> Self {
        #[derive(Clone, Copy, Default)]
        struct Sum {
            count: u32,
            height: i64,
            max: i16,
            rgb: [u32; 3],
            water: u32,
            depth: u32,
            /// Sum of water surface heights, so a mostly-water cell sits at sea level.
            water_height: i64,
        }
        let shift = key.level;
        let [origin_x, origin_z] = key.origin();
        let mut sums = vec![Sum::default(); CELLS];
        let mut lowest = i16::MAX;
        for (pos, summary) in columns {
            let column_x = pos.x * COLUMN_SIDE as i32 - origin_x;
            let column_z = pos.z * COLUMN_SIDE as i32 - origin_z;
            for (index, cell) in summary.cells().iter().enumerate() {
                if cell.is_empty() {
                    continue;
                }
                lowest = lowest.min(cell.height);
                let x = column_x + (index % COLUMN_SIDE) as i32;
                let z = column_z + (index / COLUMN_SIDE) as i32;
                let (x, z) = (x >> shift, z >> shift);
                if !(0..TILE_CELLS as i32).contains(&x) || !(0..TILE_CELLS as i32).contains(&z) {
                    continue;
                }
                let sum = &mut sums[z as usize * TILE_CELLS + x as usize];
                sum.max = if sum.count == 0 {
                    cell.height
                } else {
                    sum.max.max(cell.height)
                };
                sum.count += 1;
                sum.height += i64::from(cell.height);
                for channel in 0..3 {
                    sum.rgb[channel] += u32::from(cell.rgb[channel]);
                }
                if cell.water_depth > 0 {
                    sum.water += 1;
                    sum.depth += u32::from(cell.water_depth);
                    sum.water_height += i64::from(cell.height);
                }
            }
        }
        let cells = sums
            .into_iter()
            .map(|sum| {
                if sum.count == 0 {
                    return SurfaceCell::EMPTY;
                }
                let water = sum.water * 2 > sum.count;
                let height = if water {
                    // Shores would otherwise lift the sea toward the land's highest block.
                    (sum.water_height as f32 / sum.water as f32).round() as i16
                } else {
                    let mean = sum.height as f32 / sum.count as f32;
                    ((mean + f32::from(sum.max)) / 2.0).round() as i16
                };
                SurfaceCell {
                    height,
                    rgb: sum.rgb.map(|channel| (channel / sum.count) as u8),
                    water_depth: if water {
                        (sum.depth / sum.water).max(1) as u8
                    } else {
                        0
                    },
                }
            })
            .collect();
        Self { key, cells, lowest }
    }

    #[must_use]
    pub const fn key(&self) -> TileKey {
        self.key
    }

    #[must_use]
    pub fn cell(&self, x: usize, z: usize) -> SurfaceCell {
        self.cells[z * TILE_CELLS + x]
    }

    fn height(&self, x: usize, z: usize) -> i16 {
        self.cells[z * TILE_CELLS + x].height
    }
}

/// GPU-ready geometry for one tile: packed quads plus a per-cell colour layer.
#[derive(Debug, PartialEq, Eq)]
pub struct TileMesh {
    pub key: TileKey,
    /// Unique per build, so a consumer re-uploads only what changed.
    pub revision: u64,
    /// `[x | z << 7 | w << 14 | l << 21 | face << 28, top | bottom << 16]`; positions and spans
    /// are in cells, heights in blocks. Walls on an X plane span `l` cells, on a Z plane `w`.
    pub quads: Box<[[u32; 2]]>,
    /// `TILE_CELLS²` RGBA texels: sRGB colour and water depth in alpha.
    pub colours: Box<[u8]>,
    pub min_y: i16,
    pub max_y: i16,
}

impl TileMesh {
    /// An invisible tile without allocating or aggregating surface cells.
    pub(crate) fn empty(key: TileKey) -> Self {
        Self {
            key,
            revision: NEXT_REVISION.fetch_add(1, Ordering::Relaxed),
            quads: Box::new([]),
            colours: Box::new([]),
            min_y: 0,
            max_y: 0,
        }
    }
}

static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

/// Meshes a grid as greedy top faces and the walls between differing cells, with skirts on
/// the tile edge. At level 0 no quad crosses a column, so near columns can hide their quads.
#[must_use]
pub fn mesh_tile(grid: &TileGrid) -> TileMesh {
    let block = if grid.key.level == 0 {
        COLUMN_SIDE
    } else {
        TILE_CELLS
    };
    let heights = grid
        .cells
        .iter()
        .map(|cell| cell.height)
        .filter(|&h| h != EMPTY);
    let (min, max) = heights.fold((i16::MAX, i16::MIN), |(lo, hi), h| (lo.min(h), hi.max(h)));
    let mut quads = Vec::new();
    let floor = min.min(grid.lowest).saturating_sub(SKIRT_DEPTH_BLOCKS);
    if min <= max {
        greedy_tops(grid, block, &mut quads);
        walls(grid, block, floor, &mut quads);
    }
    let colours = grid
        .cells
        .iter()
        .flat_map(|cell| {
            let [r, g, b] = cell.rgb;
            [r, g, b, cell.water_depth.saturating_mul(8)]
        })
        .collect();
    TileMesh {
        key: grid.key,
        revision: NEXT_REVISION.fetch_add(1, Ordering::Relaxed),
        quads: quads.into_boxed_slice(),
        colours,
        min_y: if min <= max { floor } else { 0 },
        max_y: if min <= max { max } else { 0 },
    }
}

fn pack(x: usize, z: usize, w: usize, l: usize, face: u32, top: i16, bottom: i16) -> [u32; 2] {
    [
        x as u32 | (z as u32) << 7 | (w as u32) << 14 | (l as u32) << 21 | face << 28,
        u32::from(top as u16) | u32::from(bottom as u16) << 16,
    ]
}

/// Merges equal-height cells into rectangles that stay inside `block`-aligned squares.
fn greedy_tops(grid: &TileGrid, block: usize, quads: &mut Vec<[u32; 2]>) {
    let mut done = vec![false; CELLS];
    for z in 0..TILE_CELLS {
        for x in 0..TILE_CELLS {
            let height = grid.height(x, z);
            if done[z * TILE_CELLS + x] || height == EMPTY {
                continue;
            }
            let x_end = (x / block + 1) * block;
            let z_end = (z / block + 1) * block;
            let fits =
                |cx: usize, cz: usize| !done[cz * TILE_CELLS + cx] && grid.height(cx, cz) == height;
            let mut width = 1;
            while x + width < x_end && fits(x + width, z) {
                width += 1;
            }
            let mut length = 1;
            while z + length < z_end && (x..x + width).all(|cx| fits(cx, z + length)) {
                length += 1;
            }
            for cz in z..z + length {
                done[cz * TILE_CELLS + x..cz * TILE_CELLS + x + width].fill(true);
            }
            quads.push(pack(x, z, width, length, FACE_UP, height, height));
        }
    }
}

/// A vertical face on the plane between two cells, owned by the higher one.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Wall {
    face: u32,
    top: i16,
    bottom: i16,
}

/// The wall between `inner` (on the low side of the plane) and `outer`, if any.
fn wall_between(low: i16, high: i16, floor: i16, faces: (u32, u32)) -> Option<Wall> {
    let base = |h: i16| if h == EMPTY { floor } else { h };
    match (low != EMPTY, high != EMPTY) {
        (true, _) if base(high) < low => Some(Wall {
            face: faces.0,
            top: low,
            bottom: base(high),
        }),
        (_, true) if base(low) < high => Some(Wall {
            face: faces.1,
            top: high,
            bottom: base(low),
        }),
        _ => None,
    }
}

/// Emits walls on every interior plane plus skirts on the tile's four edges, merging runs
/// along each plane without crossing a `block` boundary.
fn walls(grid: &TileGrid, block: usize, floor: i16, quads: &mut Vec<[u32; 2]>) {
    let height = |x: isize, z: isize| {
        if (0..TILE_CELLS as isize).contains(&x) && (0..TILE_CELLS as isize).contains(&z) {
            grid.height(x as usize, z as usize)
        } else {
            EMPTY
        }
    };
    for plane in 0..=TILE_CELLS {
        let p = plane as isize;
        let mut along_x = Vec::with_capacity(TILE_CELLS);
        let mut along_z = Vec::with_capacity(TILE_CELLS);
        for i in 0..TILE_CELLS as isize {
            along_z.push(wall_between(
                height(p - 1, i),
                height(p, i),
                floor,
                (FACE_POSITIVE_X, FACE_NEGATIVE_X),
            ));
            along_x.push(wall_between(
                height(i, p - 1),
                height(i, p),
                floor,
                (FACE_POSITIVE_Z, FACE_NEGATIVE_Z),
            ));
        }
        for (start, length, wall) in runs(&along_z, block) {
            quads.push(pack(
                plane,
                start,
                0,
                length,
                wall.face,
                wall.top,
                wall.bottom,
            ));
        }
        for (start, length, wall) in runs(&along_x, block) {
            quads.push(pack(
                start,
                plane,
                length,
                0,
                wall.face,
                wall.top,
                wall.bottom,
            ));
        }
    }
}

/// Maximal runs of equal walls that stay inside one `block`.
fn runs(walls: &[Option<Wall>], block: usize) -> Vec<(usize, usize, Wall)> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < walls.len() {
        let Some(wall) = walls[index] else {
            index += 1;
            continue;
        };
        let end = ((index / block + 1) * block).min(walls.len());
        let mut length = 1;
        while index + length < end && walls[index + length] == Some(wall) {
            length += 1;
        }
        out.push((index, length, wall));
        index += length;
    }
    out
}

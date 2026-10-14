//! A section's 32×32 surface-biome grid packed for a fragment shader: one 16-bit raw biome
//! id per voxel column, two columns per word, the even column of a pair in the low half.
//! Shaders map the raw id to a tint record through
//! [`crate::colour::BiomeTints::by_raw_id`]; ids at or past its length, including
//! [`GPU_UNKNOWN_BIOME`], read the fallback record, as on the CPU. Raw ids rather than
//! record indices keep packed grids valid when the session's tint table changes.

use crate::{
    biome_grid::{BiomeGrid, MAX_BIOME_PALETTE, UNKNOWN_BIOME},
    geometry::{SECTION_COLUMNS, column_index},
};

/// Bits of one packed column.
pub const BIOME_CELL_BITS: u32 = u16::BITS;
/// Columns per packed word.
pub const BIOME_CELLS_PER_WORD: usize = (u32::BITS / BIOME_CELL_BITS) as usize;
/// Words of one packed grid.
pub const BIOME_GRID_WORDS: usize = SECTION_COLUMNS / BIOME_CELLS_PER_WORD;
/// The packed id of unknown biomes and of raw ids too large for a cell; past the end of
/// every tint table, so it reads the fallback record.
pub const GPU_UNKNOWN_BIOME: u16 = u16::MAX;

const _: () = assert!(SECTION_COLUMNS.is_multiple_of(BIOME_CELLS_PER_WORD));

/// One packed grid; see the module documentation.
pub type PackedBiomeGrid = [u32; BIOME_GRID_WORDS];

/// The packed cell of raw biome `raw`.
#[must_use]
pub const fn gpu_biome(raw: u32) -> u16 {
    if raw == UNKNOWN_BIOME || raw >= GPU_UNKNOWN_BIOME as u32 {
        GPU_UNKNOWN_BIOME
    } else {
        raw as u16
    }
}

/// Packs `grid`; column `(x, z)` sits in word `column_index(x, z) / 2`. Allocates only the
/// returned grid.
#[must_use]
pub fn pack_biome_grid(grid: &BiomeGrid) -> Box<PackedBiomeGrid> {
    let mut cells = [GPU_UNKNOWN_BIOME; MAX_BIOME_PALETTE];
    for (cell, &raw) in cells.iter_mut().zip(grid.palette()) {
        *cell = gpu_biome(raw);
    }
    let mut out = Box::new([0; BIOME_GRID_WORDS]);
    for (index, &cell) in grid.cells().iter().enumerate() {
        let shift = (index % BIOME_CELLS_PER_WORD) as u32 * BIOME_CELL_BITS;
        out[index / BIOME_CELLS_PER_WORD] |= u32::from(cells[usize::from(cell)]) << shift;
    }
    out
}

/// The packed cell of column `(x, z)`; panics outside `0..32`.
#[must_use]
pub fn packed_biome(grid: &PackedBiomeGrid, x: usize, z: usize) -> u16 {
    let index = column_index(x, z);
    let shift = (index % BIOME_CELLS_PER_WORD) as u32 * BIOME_CELL_BITS;
    (grid[index / BIOME_CELLS_PER_WORD] >> shift) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::SECTION_SIDE;

    #[test]
    fn packed_grids_keep_every_column_and_map_unknown_ids_to_the_fallback() {
        let mut grid = BiomeGrid::uniform(1);
        assert!(grid.set(0, 0, 7));
        assert!(grid.set(1, 0, 300));
        assert!(grid.set(31, 31, UNKNOWN_BIOME));
        assert!(grid.set(5, 9, 1 << 20));
        let packed = pack_biome_grid(&grid);
        for z in 0..SECTION_SIDE {
            for x in 0..SECTION_SIDE {
                assert_eq!(packed_biome(&packed, x, z), gpu_biome(grid.get(x, z)));
            }
        }
        assert_eq!(packed_biome(&packed, 1, 0), 300);
        assert_eq!(packed_biome(&packed, 31, 31), GPU_UNKNOWN_BIOME);
        assert_eq!(packed_biome(&packed, 5, 9), GPU_UNKNOWN_BIOME);
        assert_eq!(packed[0], 7 | 300 << 16);
    }
}

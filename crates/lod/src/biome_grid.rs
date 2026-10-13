//! Per-section 32×32 grid of surface biomes, palette-indexed with one byte per cell.

use crate::{
    geometry::{SECTION_COLUMNS, SECTION_SIDE, column_index},
    storage::StorageError,
};

/// Biome of a column no sample has covered yet. Mips never let it replace a known biome.
pub const UNKNOWN_BIOME: u32 = u32::MAX;

/// Most distinct biomes one grid holds.
pub const MAX_BIOME_PALETTE: usize = u8::MAX as usize + 1;

/// Raw Bedrock biome ids for each voxel column of a section.
///
/// Equality compares resolved cells, not palette order.
#[derive(Debug, Clone)]
pub struct BiomeGrid {
    palette: Vec<u32>,
    cells: Box<[u8; SECTION_COLUMNS]>,
}

impl BiomeGrid {
    /// Every column set to `biome`.
    #[must_use]
    pub fn uniform(biome: u32) -> Self {
        Self {
            palette: vec![biome],
            cells: Box::new([0; SECTION_COLUMNS]),
        }
    }

    /// Validates stored parts: 1..=256 palette entries and in-range cells.
    pub fn from_parts(
        palette: Vec<u32>,
        cells: [u8; SECTION_COLUMNS],
    ) -> Result<Self, StorageError> {
        if palette.is_empty() || palette.len() > MAX_BIOME_PALETTE {
            return Err(StorageError::BiomePaletteLength(palette.len()));
        }
        if let Some((cell, &index)) = cells
            .iter()
            .enumerate()
            .find(|&(_, &index)| usize::from(index) >= palette.len())
        {
            return Err(StorageError::BiomeIndex {
                cell,
                index,
                palette_len: palette.len(),
            });
        }
        Ok(Self {
            palette,
            cells: Box::new(cells),
        })
    }

    /// Raw biome id of column `(x, z)`; panics outside `0..32`.
    #[must_use]
    pub fn get(&self, x: usize, z: usize) -> u32 {
        self.palette[usize::from(self.cells[column_index(x, z)])]
    }

    /// Sets column `(x, z)`. When 256 distinct biomes are already in use the palette is
    /// compacted; if it is still full the cell keeps its biome and this returns false.
    pub fn set(&mut self, x: usize, z: usize, biome: u32) -> bool {
        let cell = column_index(x, z);
        let index = match self.palette.iter().position(|&entry| entry == biome) {
            Some(index) => index,
            None => {
                if self.palette.len() == MAX_BIOME_PALETTE {
                    self.compact();
                }
                if self.palette.len() == MAX_BIOME_PALETTE {
                    return false;
                }
                self.palette.push(biome);
                self.palette.len() - 1
            }
        };
        self.cells[cell] = index as u8;
        true
    }

    /// Rewrites the palette in first-use order with no unused entries.
    pub fn compact(&mut self) {
        let mut remap = [None::<u8>; MAX_BIOME_PALETTE];
        let mut palette = Vec::with_capacity(self.palette.len());
        for cell in self.cells.iter_mut() {
            let old = usize::from(*cell);
            *cell = *remap[old].get_or_insert_with(|| {
                palette.push(self.palette[old]);
                (palette.len() - 1) as u8
            });
        }
        self.palette = palette;
    }

    /// The canonical form: compacted, first-use palette order.
    #[must_use]
    pub fn canonical(&self) -> Self {
        let mut grid = self.clone();
        grid.compact();
        grid
    }

    /// Palette entries; canonical grids list them in first-use order.
    #[must_use]
    pub fn palette(&self) -> &[u32] {
        &self.palette
    }

    /// Palette index per column, X fastest.
    #[must_use]
    pub fn cells(&self) -> &[u8; SECTION_COLUMNS] {
        &self.cells
    }
}

impl PartialEq for BiomeGrid {
    fn eq(&self, other: &Self) -> bool {
        (0..SECTION_SIDE)
            .flat_map(|z| (0..SECTION_SIDE).map(move |x| (x, z)))
            .all(|(x, z)| self.get(x, z) == other.get(x, z))
    }
}

impl Eq for BiomeGrid {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_and_canonical_equality() {
        let mut grid = BiomeGrid::uniform(1);
        assert!(grid.set(3, 4, 7));
        assert!(grid.set(0, 0, 9));
        assert_eq!(grid.get(3, 4), 7);
        assert_eq!(grid.get(0, 0), 9);
        assert_eq!(grid.get(31, 31), 1);
        let canonical = grid.canonical();
        assert_eq!(canonical.palette(), &[9, 1, 7]);
        assert_eq!(canonical, grid);
    }

    #[test]
    fn full_palette_compacts_before_refusing() {
        let mut grid = BiomeGrid::uniform(0);
        for biome in 1..MAX_BIOME_PALETTE as u32 {
            assert!(grid.set(biome as usize % 32, biome as usize / 32, biome));
        }
        // Overwriting a cell frees its old id only after compaction.
        assert!(grid.set(1, 0, 0));
        assert!(grid.set(2, 0, 5000));
        assert_eq!(grid.get(2, 0), 5000);
        assert!(grid.palette().len() <= MAX_BIOME_PALETTE);
    }

    #[test]
    fn invalid_parts_are_rejected() {
        assert_eq!(
            BiomeGrid::from_parts(Vec::new(), [0; SECTION_COLUMNS]),
            Err(StorageError::BiomePaletteLength(0))
        );
        let mut cells = [0; SECTION_COLUMNS];
        cells[5] = 2;
        assert_eq!(
            BiomeGrid::from_parts(vec![1, 2], cells),
            Err(StorageError::BiomeIndex {
                cell: 5,
                index: 2,
                palette_len: 2
            })
        );
    }
}

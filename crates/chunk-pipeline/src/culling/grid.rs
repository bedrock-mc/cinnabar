use hashbrown::HashMap;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
};

use meshing::FaceConnectivity;
use world::SubChunkKey;

const INITIAL_XZ_BITS: u32 = 5;
const INITIAL_Y_BITS: u32 = 5;
/// Growth stops at 2^20 cells; anything beyond stays in the overflow map.
const MAX_CELL_BITS: u32 = 20;
const PRESENT: u64 = 1 << 63;
const MAX_ADDITIONS: usize = 4096;
static NEXT_GRID_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
struct Cell {
    key: SubChunkKey,
    bits: u64, // `PRESENT` plus the connectivity matrix.
}

const EMPTY: Cell = Cell {
    key: SubChunkKey::new(0, 0, 0, 0),
    bits: 0,
};

/// Where a key lives: its home cell, or the overflow map after a toroidal collision.
#[derive(Clone, Copy)]
pub(super) enum Slot {
    Cell(u32, u64),
    Overflow(FaceConnectivity),
    Missing,
}

/// Face connectivity on a toroidal grid indexed by sub-chunk coordinates.
///
/// Invariant: an overflow key's home cell is occupied, so an empty cell is a definite miss.
pub(crate) struct ConnectivityGrid {
    xz_bits: u32,
    y_bits: u32,
    cells: Vec<Cell>,
    overflow: HashMap<SubChunkKey, FaceConnectivity>,
    len: usize,
    identity: u64,
    epoch: u64,
    additions: VecDeque<SubChunkKey>,
    additions_start: usize,
}

impl Default for ConnectivityGrid {
    /// Independent grids cannot reuse another graph's retained traversal state.
    fn default() -> Self {
        Self {
            xz_bits: 0,
            y_bits: 0,
            cells: Vec::new(),
            overflow: HashMap::new(),
            len: 0,
            identity: NEXT_GRID_ID.fetch_add(1, Ordering::Relaxed),
            epoch: 0,
            additions: VecDeque::new(),
            additions_start: 0,
        }
    }
}

impl ConnectivityGrid {
    /// Identifies this graph, its slot layout and the retained addition history.
    pub(super) fn checkpoint(&self) -> (u64, u64, usize) {
        (
            self.identity,
            self.epoch,
            self.additions_start + self.additions.len(),
        )
    }

    /// Additions since a checkpoint remain valid only within the same graph epoch.
    pub(super) fn additions_since(
        &self,
        offset: usize,
    ) -> impl ExactSizeIterator<Item = &SubChunkKey> {
        self.additions.iter().skip(offset - self.additions_start)
    }

    /// Slow readers rebuild only when their additions have actually left the bounded journal.
    pub(super) fn retains_additions(&self, offset: usize) -> bool {
        (self.additions_start..=self.additions_start + self.additions.len()).contains(&offset)
    }

    /// Retires incremental readers after a destructive change or slot-layout change.
    fn invalidate_traversals(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.additions.clear();
        self.additions_start = 0;
    }
    #[must_use]
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    pub(super) const fn dims(&self) -> (u32, u32) {
        (self.xz_bits, self.y_bits)
    }

    pub(super) const fn cell_count(&self) -> usize {
        self.cells.len()
    }

    pub(super) fn index(&self, key: SubChunkKey) -> usize {
        cell_index(self.xz_bits, self.y_bits, key)
    }

    /// Key and matrix bits of an occupied cell.
    pub(super) fn cell(&self, index: u32) -> (SubChunkKey, u64) {
        let cell = self.cells[index as usize];
        (cell.key, cell.bits & !PRESENT)
    }

    pub(super) fn slot(&self, key: SubChunkKey) -> Slot {
        if self.cells.is_empty() {
            return Slot::Missing;
        }
        let index = self.index(key);
        let cell = self.cells[index];
        if cell.bits & PRESENT == 0 {
            Slot::Missing
        } else if cell.key == key {
            Slot::Cell(index as u32, cell.bits & !PRESENT)
        } else {
            self.overflow
                .get(&key)
                .map_or(Slot::Missing, |value| Slot::Overflow(*value))
        }
    }

    #[must_use]
    pub(crate) fn get(&self, key: &SubChunkKey) -> Option<FaceConnectivity> {
        match self.slot(*key) {
            Slot::Cell(_, bits) => Some(FaceConnectivity::from_bits(bits)),
            Slot::Overflow(value) => Some(value),
            Slot::Missing => None,
        }
    }

    #[must_use]
    pub(crate) fn contains_key(&self, key: &SubChunkKey) -> bool {
        !matches!(self.slot(*key), Slot::Missing)
    }

    pub(crate) fn insert(
        &mut self,
        key: SubChunkKey,
        value: FaceConnectivity,
    ) -> Option<FaceConnectivity> {
        if self.cells.is_empty() {
            self.reset(INITIAL_XZ_BITS, INITIAL_Y_BITS);
        }
        let previous = self.place(key, value);
        if previous.is_none() {
            self.len += 1;
            if self.overflow.len() > 64.max(self.len / 16) {
                self.grow();
            }
            if self.additions.len() == MAX_ADDITIONS {
                self.additions.pop_front();
                self.additions_start += 1;
            }
            self.additions.push_back(key);
        } else if previous != Some(value) {
            self.invalidate_traversals();
        }
        previous
    }

    pub(crate) fn remove(&mut self, key: &SubChunkKey) -> Option<FaceConnectivity> {
        if self.cells.is_empty() {
            return None;
        }
        let index = self.index(*key);
        let cell = self.cells[index];
        let removed = if cell.bits & PRESENT == 0 {
            None
        } else if cell.key == *key {
            self.cells[index] = EMPTY;
            self.promote_into(index);
            Some(FaceConnectivity::from_bits(cell.bits))
        } else {
            self.overflow.remove(key)
        };
        self.len -= usize::from(removed.is_some());
        if removed.is_some() {
            self.invalidate_traversals();
        }
        removed
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&SubChunkKey) -> bool) {
        let previous_len = self.len;
        self.overflow.retain(|key, _| keep(key));
        let mut len = self.overflow.len();
        for cell in &mut self.cells {
            if cell.bits & PRESENT != 0 {
                if keep(&cell.key) {
                    len += 1;
                } else {
                    *cell = EMPTY;
                }
            }
        }
        self.len = len;
        let homeless = self
            .overflow
            .keys()
            .copied()
            .filter(|key| self.cells[self.index(*key)].bits & PRESENT == 0)
            .collect::<Vec<_>>();
        for key in homeless {
            let index = self.index(key);
            // Several overflow keys can share one emptied home; the first one moves in.
            if self.cells[index].bits & PRESENT != 0 {
                continue;
            }
            let value = self.overflow.remove(&key).expect("listed overflow key");
            self.cells[index] = Cell {
                key,
                bits: value.bits() | PRESENT,
            };
        }
        if self.len != previous_len {
            self.invalidate_traversals();
        }
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = SubChunkKey> + '_ {
        self.cells
            .iter()
            .filter(|cell| cell.bits & PRESENT != 0)
            .map(|cell| cell.key)
            .chain(self.overflow.keys().copied())
    }

    fn place(&mut self, key: SubChunkKey, value: FaceConnectivity) -> Option<FaceConnectivity> {
        let index = self.index(key);
        let cell = &mut self.cells[index];
        if cell.bits & PRESENT == 0 {
            *cell = Cell {
                key,
                bits: value.bits() | PRESENT,
            };
            None
        } else if cell.key == key {
            let previous = FaceConnectivity::from_bits(cell.bits);
            cell.bits = value.bits() | PRESENT;
            Some(previous)
        } else {
            self.overflow.insert(key, value)
        }
    }

    /// Restores the invariant after `index` empties by moving one overflow key home.
    fn promote_into(&mut self, index: usize) {
        if self.overflow.is_empty() {
            return;
        }
        let Some(key) = self
            .overflow
            .keys()
            .copied()
            .find(|key| self.index(*key) == index)
        else {
            return;
        };
        let value = self.overflow.remove(&key).expect("found overflow key");
        self.cells[index] = Cell {
            key,
            bits: value.bits() | PRESENT,
        };
    }

    /// Doubles the axis most overflow keys collide on, while the cell budget allows.
    fn grow(&mut self) {
        if 2 * self.xz_bits + self.y_bits >= MAX_CELL_BITS {
            return;
        }
        let (mut horizontal, mut vertical) = (0, 0);
        for key in self.overflow.keys() {
            let occupant = self.cells[self.index(*key)].key;
            if occupant.x != key.x || occupant.z != key.z {
                horizontal += 1;
            } else if occupant.y != key.y {
                vertical += 1;
            }
        }
        let (xz_bits, y_bits) = if horizontal >= vertical && horizontal > 0 {
            (self.xz_bits + 1, self.y_bits)
        } else if vertical > 0 {
            (self.xz_bits, self.y_bits + 1)
        } else {
            return;
        };
        if 2 * xz_bits + y_bits > MAX_CELL_BITS {
            return;
        }
        let entries = self
            .cells
            .iter()
            .filter(|cell| cell.bits & PRESENT != 0)
            .map(|cell| (cell.key, FaceConnectivity::from_bits(cell.bits)))
            .chain(self.overflow.drain())
            .collect::<Vec<_>>();
        self.reset(xz_bits, y_bits);
        for (key, value) in entries {
            self.place(key, value);
        }
    }

    fn reset(&mut self, xz_bits: u32, y_bits: u32) {
        self.invalidate_traversals();
        self.xz_bits = xz_bits;
        self.y_bits = y_bits;
        self.cells = vec![EMPTY; 1 << (2 * xz_bits + y_bits)];
        self.overflow.clear();
    }
}

impl FromIterator<(SubChunkKey, FaceConnectivity)> for ConnectivityGrid {
    fn from_iter<I: IntoIterator<Item = (SubChunkKey, FaceConnectivity)>>(entries: I) -> Self {
        let mut grid = Self::default();
        for (key, value) in entries {
            grid.insert(key, value);
        }
        grid
    }
}

/// Wrapping coordinates modulo the power-of-two axis sizes, so negatives wrap too.
pub(super) fn cell_index(xz_bits: u32, y_bits: u32, key: SubChunkKey) -> usize {
    let xz_mask = (1_u32 << xz_bits) - 1;
    let y_mask = (1_u32 << y_bits) - 1;
    let x = key.x as u32 & xz_mask;
    let z = key.z as u32 & xz_mask;
    let y = key.y as u32 & y_mask;
    (x | (z << xz_bits) | (y << (2 * xz_bits))) as usize
}

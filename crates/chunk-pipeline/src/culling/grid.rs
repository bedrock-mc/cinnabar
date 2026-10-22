use hashbrown::HashMap;
use std::{
    collections::{BTreeSet, VecDeque},
    sync::atomic::{AtomicU64, Ordering},
};

use meshing::FaceConnectivity;
use world::SubChunkKey;

const INITIAL_XZ_BITS: u32 = 5;
const INITIAL_Y_BITS: u32 = 5;
/// Growth stops at 2^20 cells; anything beyond stays in the overflow map.
const MAX_CELL_BITS: u32 = 20;
const PRESENT: u64 = 1 << 63;
const MAX_CHANGES: usize = 4096;
static NEXT_GRID_ID: AtomicU64 = AtomicU64::new(1);

/// A graph change retained for incremental traversals within one slot layout.
#[derive(Clone, Copy, Debug)]
pub(super) enum GraphChange {
    /// The key joined the graph, or its matrix gained connections.
    Opened(SubChunkKey),
    /// A matrix lost connections, so reachability can only have shrunk.
    Narrowed,
    /// The key left the graph, from this cell when it occupied one.
    Removed(SubChunkKey, Option<u32>),
    /// An overflow key moved into its emptied home cell.
    Promoted(SubChunkKey, u32),
}

/// The latest graph changes, dropping the oldest past [`MAX_CHANGES`].
#[derive(Default)]
struct ChangeJournal {
    changes: VecDeque<GraphChange>,
    /// Changes dropped from the front since the layout last changed.
    start: usize,
}

impl ChangeJournal {
    fn record(&mut self, change: GraphChange) {
        if self.changes.len() == MAX_CHANGES {
            self.changes.pop_front();
            self.start += 1;
        }
        self.changes.push_back(change);
    }

    fn end(&self) -> usize {
        self.start + self.changes.len()
    }
}

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
    /// The layout the first insertion allocates, at least the initial one.
    reserved: (u32, u32),
    cells: Vec<Cell>,
    overflow: HashMap<SubChunkKey, FaceConnectivity>,
    /// Derived alongside overflow insertion/removal; orders candidates by home cell.
    overflow_homes: BTreeSet<(usize, SubChunkKey)>,
    len: usize,
    identity: u64,
    epoch: u64,
    journal: ChangeJournal,
    #[cfg(test)]
    promotion_visits: usize,
}

impl Default for ConnectivityGrid {
    /// Independent grids cannot reuse another graph's retained traversal state.
    fn default() -> Self {
        Self {
            xz_bits: 0,
            y_bits: 0,
            reserved: (INITIAL_XZ_BITS, INITIAL_Y_BITS),
            cells: Vec::new(),
            overflow: HashMap::new(),
            overflow_homes: BTreeSet::new(),
            len: 0,
            identity: NEXT_GRID_ID.fetch_add(1, Ordering::Relaxed),
            epoch: 0,
            journal: ChangeJournal::default(),
            #[cfg(test)]
            promotion_visits: 0,
        }
    }
}

impl ConnectivityGrid {
    /// Identifies this graph, its slot layout and the retained change history.
    pub(super) fn checkpoint(&self) -> (u64, u64, usize) {
        (self.identity, self.epoch, self.journal.end())
    }

    /// Changes since a checkpoint, valid only within the same graph epoch.
    pub(super) fn changes_since(
        &self,
        offset: usize,
    ) -> impl ExactSizeIterator<Item = &GraphChange> {
        self.journal
            .changes
            .iter()
            .skip(offset - self.journal.start)
    }

    /// Slow readers rebuild only when their changes have actually left the bounded journal.
    pub(super) fn retains_changes(&self, offset: usize) -> bool {
        (self.journal.start..=self.journal.end()).contains(&offset)
    }

    /// Retires incremental readers after a slot-layout change renumbers every cell.
    fn invalidate_traversals(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.journal = ChangeJournal::default();
    }

    /// Lays the grid out once for a view `radius` chunks around its centre and `height`
    /// sections tall, within the cell budget, so filling that view never re-lays it out (which
    /// rebuilds every traversal). Axes span `2 * radius` columns, the layout growth settles on:
    /// only the outermost columns of a round view then share cells through the overflow map.
    /// A grid never shrinks; an empty one allocates the layout at its first insertion.
    pub(crate) fn reserve_view(&mut self, radius: u32, height: u32) {
        let bits = |extent: u32| extent.max(1).next_power_of_two().trailing_zeros();
        let y_bits = bits(height).max(self.reserved.1);
        let xz_bits = bits(radius.saturating_mul(2))
            .max(self.reserved.0)
            .min(MAX_CELL_BITS.saturating_sub(y_bits) / 2);
        if xz_bits < self.reserved.0 {
            return;
        }
        self.reserved = (xz_bits, y_bits);
        let layout = (xz_bits.max(self.xz_bits), y_bits.max(self.y_bits));
        if !self.cells.is_empty()
            && layout != (self.xz_bits, self.y_bits)
            && 2 * layout.0 + layout.1 <= MAX_CELL_BITS
        {
            self.relayout(layout.0, layout.1);
        }
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
            self.reset(self.reserved.0, self.reserved.1);
        }
        let previous = self.place(key, value);
        match previous {
            None => {
                self.len += 1;
                if self.overflow.len() > 64.max(self.len / 16) {
                    self.grow();
                }
                self.journal.record(GraphChange::Opened(key));
            }
            Some(previous) => {
                let (old, new) = (previous.bits(), value.bits());
                if old & !new != 0 {
                    self.journal.record(GraphChange::Narrowed);
                }
                if new & !old != 0 {
                    self.journal.record(GraphChange::Opened(key));
                }
            }
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
            self.journal
                .record(GraphChange::Removed(*key, Some(index as u32)));
            self.promote_into(index);
            Some(FaceConnectivity::from_bits(cell.bits))
        } else {
            let removed = self.remove_overflow(key);
            if removed.is_some() {
                self.journal.record(GraphChange::Removed(*key, None));
            }
            removed
        };
        self.len -= usize::from(removed.is_some());
        removed
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&SubChunkKey) -> bool) {
        self.overflow.retain(|key, _| {
            let kept = keep(key);
            if !kept {
                self.journal.record(GraphChange::Removed(*key, None));
            }
            kept
        });
        self.overflow_homes
            .retain(|(_, key)| self.overflow.contains_key(key));
        let mut len = self.overflow.len();
        for (index, cell) in self.cells.iter_mut().enumerate() {
            if cell.bits & PRESENT != 0 {
                if keep(&cell.key) {
                    len += 1;
                } else {
                    self.journal
                        .record(GraphChange::Removed(cell.key, Some(index as u32)));
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
            let value = self.remove_overflow(&key).expect("listed overflow key");
            self.cells[index] = Cell {
                key,
                bits: value.bits() | PRESENT,
            };
            self.journal
                .record(GraphChange::Promoted(key, index as u32));
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
            let previous = self.overflow.insert(key, value);
            if previous.is_none() {
                self.overflow_homes.insert((index, key));
            }
            previous
        }
    }

    /// Removes an overflow entry and its home index together.
    fn remove_overflow(&mut self, key: &SubChunkKey) -> Option<FaceConnectivity> {
        let value = self.overflow.remove(key)?;
        self.overflow_homes.remove(&(self.index(*key), *key));
        Some(value)
    }

    /// Restores an empty cell with one matching overflow key in logarithmic time.
    fn promote_into(&mut self, index: usize) {
        let first_key = SubChunkKey::new(i32::MIN, i32::MIN, i32::MIN, i32::MIN);
        let key = self
            .overflow_homes
            .range((index, first_key)..)
            .next()
            .filter(|(home, _)| *home == index)
            .map(|(_, key)| *key);
        #[cfg(test)]
        {
            self.promotion_visits += usize::from(key.is_some());
        }
        let Some(key) = key else {
            return;
        };
        let value = self.remove_overflow(&key).expect("indexed overflow key");
        self.cells[index] = Cell {
            key,
            bits: value.bits() | PRESENT,
        };
        self.journal
            .record(GraphChange::Promoted(key, index as u32));
    }

    /// Counts candidate keys inspected by promotions since the last test observation.
    #[cfg(test)]
    pub(super) fn take_promotion_visits(&mut self) -> usize {
        std::mem::take(&mut self.promotion_visits)
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
        self.relayout(xz_bits, y_bits);
    }

    /// Re-places every key under a new slot layout, which retires incremental readers.
    fn relayout(&mut self, xz_bits: u32, y_bits: u32) {
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
        self.overflow_homes.clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Compares the derived home index with its authoritative overflow entries.
    fn assert_home_index(grid: &ConnectivityGrid) {
        assert_eq!(
            grid.overflow_homes,
            grid.overflow
                .keys()
                .map(|key| (grid.index(*key), *key))
                .collect()
        );
    }

    /// Insertions, replacements, promotion, retain and growth cannot leave a stale home index.
    #[test]
    fn overflow_home_index_tracks_mutations() {
        let mut grid = ConnectivityGrid::default();
        let keys: Vec<_> = (0..96)
            .flat_map(|x| (0..2).map(move |dimension| SubChunkKey::new(dimension, x, 0, x)))
            .collect();
        for key in &keys {
            grid.insert(*key, FaceConnectivity::all());
            assert_home_index(&grid);
        }
        for key in &keys {
            grid.insert(*key, FaceConnectivity::none());
        }
        assert_home_index(&grid);
        for key in keys.iter().step_by(3) {
            grid.remove(key);
            assert_home_index(&grid);
        }
        grid.retain(|key| key.x % 2 == 0);
        assert_home_index(&grid);
        for key in keys {
            grid.remove(&key);
        }
        assert_home_index(&grid);
        assert_eq!(grid.len(), 0);
    }
}

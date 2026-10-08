use std::collections::VecDeque;

use super::{
    solve::{IncreaseEntry, enqueue_counted},
    types::LightSolveError,
};

const PENDING: u8 = 1;
const DIRECT_SKY: u8 = 2;

/// Pending increases merge their latest provenance before the cell visits its neighbours.
#[derive(Default)]
pub(super) struct IncreaseQueue {
    entries: VecDeque<IncreaseEntry>,
    pending: Vec<u8>,
}

impl IncreaseQueue {
    /// Starts a bounded solve while retaining both queue and membership allocations.
    pub(super) fn reset(&mut self, volume: usize) {
        self.clear();
        self.pending.resize(volume, 0);
    }

    /// Counts only new work; pending direct-sky upgrades merge without spending the queue budget.
    #[inline]
    pub(super) fn push_back(
        &mut self,
        entry: IncreaseEntry,
        queued_total: &mut usize,
        max: usize,
    ) -> Result<(), LightSolveError> {
        let index = entry.index;
        if self.pending[index] == 0 {
            enqueue_counted(queued_total, 1, max)?;
            self.entries.push_back(entry);
        }
        self.pending[index] |= PENDING | if entry.direct_sky { DIRECT_SKY } else { 0 };
        Ok(())
    }

    /// Releases membership before propagation so a later increase can revisit the cell.
    #[inline]
    pub(super) fn pop_front(&mut self) -> Option<IncreaseEntry> {
        let mut entry = self.entries.pop_front()?;
        let index = entry.index;
        entry.direct_sky = self.pending[index] & DIRECT_SKY != 0;
        self.pending[index] = 0;
        Some(entry)
    }

    /// Counts pending neighbour visits rather than repeated updates to queued cells.
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Builds a bounded queue for isolated seed tests.
    #[cfg(test)]
    pub(super) fn new(volume: usize) -> Self {
        let mut queue = Self::default();
        queue.reset(volume);
        queue
    }

    /// Exposes pending entries with merged provenance for seed comparisons.
    #[cfg(test)]
    pub(super) fn iter(&self) -> impl Iterator<Item = IncreaseEntry> + '_ {
        self.entries.iter().map(|entry| IncreaseEntry {
            position: entry.position,
            index: entry.index,
            direct_sky: self.pending[entry.index] & DIRECT_SKY != 0,
        })
    }

    /// Records retained storage without inspecting allocator-global state.
    #[cfg(test)]
    pub(super) fn buffers(&self) -> [(usize, usize); 2] {
        [
            (0, self.entries.capacity()),
            (self.pending.as_ptr() as usize, self.pending.capacity()),
        ]
    }

    /// Discards unfinished work while leaving already drained membership slots untouched.
    pub(super) fn clear(&mut self) {
        while self.pop_front().is_some() {}
    }
}

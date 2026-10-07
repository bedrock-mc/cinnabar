//! Stable GPU indices and allocation-free dirty-range draining after warm-up.

/// Slots keep their indices until explicitly released; holes are reused before growing.
#[derive(Debug)]
pub struct PrimitiveSlots<T> {
    pub values: Vec<T>,
    free: Vec<u32>,
    dirty: Vec<u32>,
    marked: Vec<bool>,
}

impl<T> Default for PrimitiveSlots<T> {
    /// Empty buffers allocate only when the first packet arrives.
    fn default() -> Self {
        Self {
            values: Vec::new(),
            free: Vec::new(),
            dirty: Vec::new(),
            marked: Vec::new(),
        }
    }
}

impl<T: Copy + PartialEq> PrimitiveSlots<T> {
    /// Allocates a stable index and publishes exactly that slot.
    pub fn insert(&mut self, value: T) -> u32 {
        let slot = if let Some(slot) = self.free.pop() {
            self.values[slot as usize] = value;
            slot
        } else {
            let slot = self.values.len() as u32;
            self.values.push(value);
            self.marked.push(false);
            slot
        };
        self.mark(slot);
        slot
    }

    /// Marks only actual byte-content changes; repeated patches coalesce before extraction.
    pub fn set(&mut self, slot: u32, value: T) {
        if self.values[slot as usize] != value {
            self.values[slot as usize] = value;
            self.mark(slot);
        }
    }

    /// Returns a hidden slot for reuse; its old GPU value must already be invisible.
    pub fn release(&mut self, slot: u32) {
        self.free.push(slot);
    }

    /// Reports whether every allocated slot is free without scanning instance records.
    pub fn is_empty(&self) -> bool {
        self.values.len() == self.free.len()
    }

    /// Reports whether draining can return immediately without inspecting slots.
    pub fn is_clean(&self) -> bool {
        self.dirty.is_empty()
    }

    /// Sends each changed slot once, merging only adjacent changes and retaining scratch capacity.
    pub fn drain(&mut self, mut upload: impl FnMut(u32, &[T])) {
        if self.dirty.is_empty() {
            return;
        }
        self.dirty.sort_unstable();
        let mut start = self.dirty[0];
        let mut end = start + 1;
        for &slot in &self.dirty[1..] {
            if slot == end {
                end += 1;
            } else {
                upload(start, &self.values[start as usize..end as usize]);
                start = slot;
                end = slot + 1;
            }
        }
        upload(start, &self.values[start as usize..end as usize]);
        for slot in self.dirty.drain(..) {
            self.marked[slot as usize] = false;
        }
    }

    /// Adds one slot to the dirty set without duplicate work in the same publication.
    fn mark(&mut self, slot: u32) {
        if !self.marked[slot as usize] {
            self.marked[slot as usize] = true;
            self.dirty.push(slot);
        }
    }
}

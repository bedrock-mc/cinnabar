//! Stable physical slots let edits change canonical palette order without rewriting every cell.

use std::sync::{Arc, OnceLock};

use super::{
    BLOCKS_PER_SUB_CHUNK, PalettedStorage, bits_for_palette_len, word_count, write_palette_index,
};

/// One physical packed index and its current logical palette entry; zero count means reusable.
#[derive(Debug, Clone)]
pub(super) struct PaletteSlot {
    pub(super) palette_index: u16,
    pub(super) count: u16,
}

/// Snapshot-owned edit bookkeeping; only a requested canonical word view duplicates packed data.
#[derive(Debug, Clone)]
pub(super) struct PaletteEdits {
    pub(super) slots: Box<[PaletteSlot]>,
    /// True when the physical words already use the public palette indices and minimal width.
    pub(super) direct: bool,
    pub(super) canonical_words: OnceLock<Arc<[u32]>>,
}

impl PaletteEdits {
    /// Adopts counts from a verified canonical representation without visiting cells again.
    pub(super) fn new(counts: Vec<u16>) -> Self {
        debug_assert!((1..=BLOCKS_PER_SUB_CHUNK).contains(&counts.len()));
        debug_assert!(counts.iter().all(|&count| count != 0));
        Self {
            slots: counts
                .into_iter()
                .enumerate()
                .map(|(index, count)| PaletteSlot {
                    palette_index: index as u16,
                    count,
                })
                .collect(),
            direct: true,
            canonical_words: OnceLock::new(),
        }
    }

    /// Reuses dead physical slots before growing, bounded by the number of cells in a section.
    fn allocate(&mut self, palette_index: usize) -> usize {
        let slot = self
            .slots
            .iter()
            .position(|slot| slot.count == 0)
            .unwrap_or_else(|| {
                assert!(self.slots.len() < BLOCKS_PER_SUB_CHUNK);
                let mut slots = std::mem::take(&mut self.slots).into_vec();
                slots.push(PaletteSlot {
                    palette_index: 0,
                    count: 0,
                });
                self.slots = slots.into_boxed_slice();
                self.slots.len() - 1
            });
        self.slots[slot] = PaletteSlot {
            palette_index: palette_index as u16,
            count: 0,
        };
        slot
    }
}

impl PalettedStorage {
    /// Changes one physical word and live palette bookkeeping, preserving canonical survivor order.
    pub(super) fn patch_edit(&mut self, linear: usize, runtime_id: u32) {
        let source = self
            .physical_index(linear)
            .expect("validated editable storage");
        let edits = self.edits.as_mut().expect("verified editable palette");
        edits.canonical_words.take();
        let source_index = usize::from(edits.slots[source].palette_index);
        let destination = self
            .palette
            .values
            .iter()
            .position(|&value| value == runtime_id)
            .map(|index| {
                edits
                    .slots
                    .iter()
                    .position(|slot| slot.count != 0 && usize::from(slot.palette_index) == index)
                    .expect("every live palette entry has one physical slot")
            });
        edits.slots[source].count -= 1;
        let removed = edits.slots[source].count == 0;
        if removed || destination.is_none() {
            let mut values = std::mem::take(&mut self.palette.values).into_vec();
            if removed {
                values.remove(source_index);
                if source_index < values.len() {
                    for slot in &mut edits.slots {
                        if slot.count != 0 && usize::from(slot.palette_index) > source_index {
                            slot.palette_index -= 1;
                        }
                    }
                }
            }
            if destination.is_none() {
                values.push(runtime_id);
            }
            self.palette.values = values.into_boxed_slice();
        }
        let slot = destination.unwrap_or_else(|| edits.allocate(self.palette.len() - 1));
        edits.slots[slot].count += 1;
        let bits = bits_for_palette_len(edits.slots.len());
        if bits > self.bits_per_index {
            let mut words = vec![0; word_count(bits)];
            if self.bits_per_index != 0 {
                #[cfg(test)]
                self.work
                    .packed_cells
                    .fetch_add(BLOCKS_PER_SUB_CHUNK, std::sync::atomic::Ordering::Relaxed);
                for cell in 0..BLOCKS_PER_SUB_CHUNK {
                    write_palette_index(&mut words, bits, cell, self.physical_index(cell).unwrap());
                }
            }
            self.words = words.into_boxed_slice();
            self.bits_per_index = bits;
        }
        write_palette_index(&mut self.words, self.bits_per_index, linear, slot);
        if removed || destination.is_none() {
            let edits = self.edits.as_mut().unwrap();
            edits.direct = self.bits_per_index == bits_for_palette_len(self.palette.len())
                && edits.slots.iter().enumerate().all(|(index, slot)| {
                    slot.count == 0 || index == usize::from(slot.palette_index)
                });
        }
    }
}

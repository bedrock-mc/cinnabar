use super::{BufferWrite, MAX_COPIES, SLOT_COUNT};
use bevy::render::renderer::RenderDevice;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

#[cfg(test)]
#[path = "pool_tests.rs"]
mod tests;

const READY: u8 = 0;
const ACTIVE: u8 = 1;
const PENDING: u8 = 2;
const FAILED: u8 = 3;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Stats {
    pub(super) staged_writes: u64,
    pub(super) staged_bytes: u64,
    pub(super) fallback_writes: u64,
    pub(super) fallback_bytes: u64,
    pub(super) overflow_submissions: u64,
}

struct Slot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    offset: u64,
}

struct Copy {
    slot: usize,
    source: u64,
    target: wgpu::Buffer,
    offset: u64,
    bytes: u64,
}

pub(super) struct Pool {
    slots: [Slot; SLOT_COUNT],
    copies: Vec<Copy>,
    pub(super) stats: Stats,
}

impl Pool {
    /// Allocates fixed mapped slots and copy metadata once, before recurring preparation.
    pub(super) fn new(device: &RenderDevice, slot_bytes: u64) -> Self {
        Self {
            slots: std::array::from_fn(|_| Slot {
                buffer: device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
                    label: Some("retained frame upload staging"),
                    size: slot_bytes,
                    usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: true,
                }),
                state: Arc::new(AtomicU8::new(READY)),
                offset: 0,
            }),
            copies: Vec::with_capacity(MAX_COPIES),
            stats: Stats::default(),
        }
    }

    /// Admits a complete batch without allocating GPU storage, polling, or partially staging it.
    pub(super) fn try_stage(&mut self, writes: &[BufferWrite<'_>]) -> bool {
        if !self.can_stage(writes) {
            return false;
        }
        for &(target, offset, bytes) in writes.iter().filter(|write| !write.2.is_empty()) {
            let (index, start) = self
                .reserve(&self.offsets(), bytes.len() as u64)
                .expect("batch reserved storage");
            let slot = &mut self.slots[index];
            slot.state.store(ACTIVE, Ordering::Release);
            slot.offset = start + bytes.len() as u64;
            slot.buffer
                .slice(start..slot.offset)
                .get_mapped_range_mut()
                .copy_from_slice(bytes);
            self.copies.push(Copy {
                slot: index,
                source: start,
                target: target.clone(),
                offset,
                bytes: bytes.len() as u64,
            });
            self.stats.staged_writes += 1;
            self.stats.staged_bytes += bytes.len() as u64;
        }
        true
    }

    /// Reports pending command work without creating an encoder for an empty frame.
    pub(super) fn has_copies(&self) -> bool {
        !self.copies.is_empty()
    }

    /// Samples ready, active, pending and failed slot counts alongside queued copies.
    #[cfg(any(test, feature = "tracy"))]
    pub(super) fn diagnostic_counts(&self) -> ([usize; 4], usize) {
        let mut counts = [0; 4];
        for slot in &self.slots {
            counts[usize::from(slot.state.load(Ordering::Acquire))] += 1;
        }
        (counts, self.copies.len())
    }

    /// Uses only mapped slots; incomplete and failed mappings cannot be reused.
    fn offsets(&self) -> [Option<u64>; SLOT_COUNT] {
        self.slots
            .each_ref()
            .map(|slot| match slot.state.load(Ordering::Acquire) {
                READY => Some(0),
                ACTIVE => Some(slot.offset),
                _ => None,
            })
    }

    /// Simulates the entire batch while leaving storage untouched on admission failure.
    fn can_stage(&self, writes: &[BufferWrite<'_>]) -> bool {
        let count = writes.iter().filter(|write| !write.2.is_empty()).count();
        if count > MAX_COPIES - self.copies.len() {
            return false;
        }
        let mut offsets = self.offsets();
        for &(_, _, bytes) in writes.iter().filter(|write| !write.2.is_empty()) {
            let Some((index, start)) = self.reserve(&offsets, bytes.len() as u64) else {
                return false;
            };
            offsets[index] = Some(start + bytes.len() as u64);
        }
        true
    }

    /// Packs mapped ranges at WebGPU's required alignment, preferring already active slots.
    fn reserve(&self, offsets: &[Option<u64>; SLOT_COUNT], bytes: u64) -> Option<(usize, u64)> {
        for occupied in [true, false] {
            for (index, offset) in offsets.iter().enumerate() {
                let Some(offset) = offset.filter(|offset| (*offset > 0) == occupied) else {
                    continue;
                };
                let start = offset.div_ceil(wgpu::MAP_ALIGNMENT) * wgpu::MAP_ALIGNMENT;
                if start
                    .checked_add(bytes)
                    .is_some_and(|end| end <= self.slots[index].buffer.size())
                {
                    return Some((index, start));
                }
            }
        }
        None
    }

    /// Finds writes that would otherwise execute before an older staged update to the same bytes.
    pub(super) fn overlaps(&self, writes: &[BufferWrite<'_>]) -> bool {
        self.copies.iter().any(|copy| {
            writes
                .iter()
                .filter(|write| !write.2.is_empty())
                .any(|(buffer, offset, bytes)| {
                    **buffer == copy.target
                        && *offset < copy.offset + copy.bytes
                        && copy.offset < offset.saturating_add(bytes.len() as u64)
                })
        })
    }

    /// Encodes copies in issue order and remaps each used slot only after GPU completion.
    pub(super) fn encode(&mut self, encoder: &mut wgpu::CommandEncoder) {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "uploads.encode",
            copies = self.copies.len(),
            bytes = self.copies.iter().map(|copy| copy.bytes).sum::<u64>(),
        )
        .entered();
        for slot in &self.slots {
            if slot.state.load(Ordering::Acquire) == ACTIVE {
                slot.buffer.unmap();
                slot.state.store(PENDING, Ordering::Release);
            }
        }
        for copy in self.copies.drain(..) {
            encoder.copy_buffer_to_buffer(
                &self.slots[copy.slot].buffer,
                copy.source,
                &copy.target,
                copy.offset,
                copy.bytes,
            );
        }
        for slot in &mut self.slots {
            if slot.offset == 0 || slot.state.load(Ordering::Acquire) != PENDING {
                continue;
            }
            let state = Arc::clone(&slot.state);
            encoder.map_buffer_on_submit(&slot.buffer, wgpu::MapMode::Write, .., move |result| {
                state.store(
                    if result.is_ok() { READY } else { FAILED },
                    Ordering::Release,
                );
            });
            slot.offset = 0;
        }
    }
}

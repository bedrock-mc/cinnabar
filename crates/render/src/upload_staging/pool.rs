use super::{BufferWrite, Stats};
use bevy::render::renderer::RenderDevice;
use render_api::PublicationServiceConfig;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

#[cfg(test)]
#[path = "pool_tests.rs"]
mod tests;

/// Retained mapped storage and copy records for one family of participating buffers.
#[derive(Debug, Clone, Copy)]
pub(super) struct Shape {
    pub(super) name: &'static str,
    /// Slots created up front.
    pub(super) slots: usize,
    /// Ceiling that growth after missed frames never passes.
    pub(super) max_slots: usize,
    /// Slots one frame's misses may add; each costs one buffer creation, like one queue write.
    pub(super) growth_slots: usize,
    pub(super) slot_bytes: u64,
    pub(super) max_copies: usize,
}

/// One maximum particle frame, derived from the simulation cap and actual GPU record.
const PARTICLE_FRAME_BYTES: u64 =
    (particles::MAX_LIVE_PARTICLES * std::mem::size_of::<particles::ParticleInstance>()) as u64;
/// Additional recurring hand/UI writes and mapping-alignment padding per frame.
const FRAME_OTHER_BYTES: u64 = 64 * 1024;
const FRAME_SLOTS_PER_FRAME: u64 = 2;
const FRAME_IN_FLIGHT_FRAMES: usize = 4;
const FRAME_SLOT_COUNT: usize = FRAME_IN_FLIGHT_FRAMES * FRAME_SLOTS_PER_FRAME as usize;
const FRAME_SLOT_GRANULE: u64 = 64 * 1024;
const FRAME_SLOT_BYTES: u64 = (PARTICLE_FRAME_BYTES + FRAME_OTHER_BYTES)
    .div_ceil(FRAME_SLOTS_PER_FRAME * FRAME_SLOT_GRANULE)
    * FRAME_SLOT_GRANULE;

/// Retained frame storage stops at 64 MiB even under sustained overflow.
const FRAME_MAX_SLOTS: usize = (64_u64 * 1024 * 1024 / FRAME_SLOT_BYTES) as usize;

/// Initial storage covers particles and small recurring writes; missed frames can grow
/// toward a bounded retained budget for actors, models and the other frame owners.
pub(super) const FRAME: Shape = Shape {
    name: "retained frame upload staging",
    slots: FRAME_SLOT_COUNT,
    max_slots: FRAME_MAX_SLOTS,
    growth_slots: FRAME_SLOT_COUNT,
    slot_bytes: FRAME_SLOT_BYTES,
    max_copies: 128 + 2 * render_model::MAX_ACTOR_RENDER_INSTANCES + FRAME_MAX_SLOTS,
};

const TERRAIN_SLOT_BYTES: u64 = 256 * 1024;
const TERRAIN_TARGET_BYTES_PER_SECOND: u64 =
    PublicationServiceConfig::DEFAULT.target_bytes_per_second;
/// Terrain arena writes. A 32 blocks/s flight at 1080p and 12 chunks measured about 13.5 MB/s
/// of terrain, arriving as bursts above 3 MiB per buffer in single frames; the 128 blocks/s
/// streaming gate scales that to about 54 MB/s. The pool starts at three 120 Hz frames of the
/// publication target rate and grows after missed frames up to a quarter second of it
/// (32 MiB), so slots, which recycle a few frames after submission, absorb bursts of several
/// MiB per frame; larger bursts stage what fits and queue the rest.
pub(super) const TERRAIN: Shape = Shape {
    name: "retained terrain upload staging",
    slots: (TERRAIN_TARGET_BYTES_PER_SECOND * 3 / 120).div_ceil(TERRAIN_SLOT_BYTES) as usize,
    max_slots: (TERRAIN_TARGET_BYTES_PER_SECOND / 4).div_ceil(TERRAIN_SLOT_BYTES) as usize,
    growth_slots: 8,
    slot_bytes: TERRAIN_SLOT_BYTES,
    max_copies: 2048,
};

const READY: u8 = 0;
const ACTIVE: u8 = 1;
const PENDING: u8 = 2;
const FAILED: u8 = 3;

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

/// Where one staged piece lands and whether it extends the newest copy.
struct Piece {
    slot: usize,
    start: u64,
    bytes: u64,
    extends: bool,
}

pub(super) struct Pool {
    shape: Shape,
    device: RenderDevice,
    slots: Vec<Slot>,
    copies: Vec<Copy>,
    /// Fallback bytes since the last growth check.
    missed_bytes: u64,
    pub(super) stats: Stats,
}

impl Pool {
    /// Allocates the initial mapped slots and all copy metadata before recurring preparation.
    pub(super) fn new(device: &RenderDevice, shape: Shape) -> Self {
        let mut slots = Vec::with_capacity(shape.max_slots);
        slots.extend((0..shape.slots).map(|_| Self::slot(device, shape)));
        Self {
            shape,
            device: device.clone(),
            slots,
            copies: Vec::with_capacity(shape.max_copies),
            missed_bytes: 0,
            stats: Stats::default(),
        }
    }

    /// Creates one slot, mapped and ready to stage.
    fn slot(device: &RenderDevice, shape: Shape) -> Slot {
        Slot {
            buffer: device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
                label: Some(shape.name),
                size: shape.slot_bytes,
                usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: true,
            }),
            state: Arc::new(AtomicU8::new(READY)),
            offset: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn slot_bytes(&self) -> u64 {
        self.shape.slot_bytes
    }

    /// Names the pool in trace zones.
    #[cfg(feature = "tracy")]
    pub(super) fn name(&self) -> &'static str {
        self.shape.name
    }

    /// Stages writes in issue order until the pool fills, without allocating staging storage or polling.
    /// Returns the first write not wholly staged and how many of its bytes were.
    #[track_caller]
    pub(super) fn stage_prefix(&mut self, writes: &[BufferWrite<'_>]) -> Option<(usize, u64)> {
        for (index, &(target, offset, bytes)) in writes.iter().enumerate() {
            if bytes.is_empty() {
                continue;
            }
            let staged = self.stage(target, offset, bytes);
            render_scene::runtime_profile_uploads::record(
                render_scene::runtime_profile_uploads::UploadKind::Buffer,
                render_scene::runtime_profile_uploads::UploadPath::Staged,
                staged,
            );
            if staged < bytes.len() as u64 {
                return Some((index, staged));
            }
            self.stats.staged_writes += 1;
        }
        None
    }

    /// Counts bytes that took the queue path toward the next growth check.
    pub(super) fn record_miss(&mut self, writes: u64, bytes: u64) {
        self.stats.fallback_writes += writes;
        self.stats.fallback_bytes += bytes;
        self.missed_bytes += bytes;
    }

    /// After a frame with misses, adds slots for the missed bytes, at most the shape's
    /// per-frame growth and never past its ceiling.
    pub(super) fn grow(&mut self) {
        let missed = std::mem::take(&mut self.missed_bytes);
        let wanted = usize::try_from(missed.div_ceil(self.shape.slot_bytes)).unwrap_or(usize::MAX);
        let added = wanted
            .min(self.shape.growth_slots)
            .min(self.shape.max_slots.saturating_sub(self.slots.len()));
        if added == 0 {
            return;
        }
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "uploads.grow",
            pool = self.shape.name,
            missed,
            added,
            slots = self.slots.len() + added,
        )
        .entered();
        for _ in 0..added {
            // Out of memory: writes keep taking the queue path and growth retries after misses.
            let Some(slot) = crate::gpu_allocation::checked(
                &self.device,
                self.shape.name,
                self.shape.slot_bytes,
                || Self::slot(&self.device, self.shape),
            ) else {
                break;
            };
            self.slots.push(slot);
            self.stats.grown_slots += 1;
        }
    }

    /// Copies as many of `bytes` as fit, extending the newest copy when it abuts; returns
    /// the staged length, always a whole number of copy-aligned words.
    fn stage(&mut self, target: &wgpu::Buffer, mut offset: u64, mut bytes: &[u8]) -> u64 {
        let mut staged = 0;
        while !bytes.is_empty() {
            let Some(piece) = self.reserve(target, offset, bytes.len() as u64) else {
                break;
            };
            if !piece.extends && self.copies.len() == self.shape.max_copies {
                break;
            }
            let (head, rest) = bytes.split_at(piece.bytes as usize);
            let slot = &mut self.slots[piece.slot];
            slot.state.store(ACTIVE, Ordering::Release);
            slot.offset = piece.start + piece.bytes;
            // An extending piece may start mid-word of the mapping granularity.
            let mapped = piece.start - piece.start % wgpu::MAP_ALIGNMENT;
            slot.buffer
                .slice(mapped..slot.offset)
                .get_mapped_range_mut()
                .expect("staging range is mapped and unaliased")
                .slice((piece.start - mapped) as usize..)
                .copy_from_slice(head);
            if piece.extends {
                let newest = self
                    .copies
                    .last_mut()
                    .expect("an extending piece follows a copy");
                newest.bytes += piece.bytes;
            } else {
                self.copies.push(Copy {
                    slot: piece.slot,
                    source: piece.start,
                    target: target.clone(),
                    offset,
                    bytes: piece.bytes,
                });
            }
            self.stats.staged_bytes += piece.bytes;
            staged += piece.bytes;
            offset += piece.bytes;
            bytes = rest;
        }
        staged
    }

    /// Reports pending command work without creating an encoder for an empty frame.
    pub(super) fn has_copies(&self) -> bool {
        !self.copies.is_empty()
    }

    /// Samples ready, active, pending and failed slot counts alongside queued copies.
    #[cfg(any(test, feature = "tracy"))]
    pub(super) fn diagnostic_counts(&self) -> ([usize; 4], usize) {
        let mut counts = [0; 4];
        for slot in self.slots.iter() {
            counts[usize::from(slot.state.load(Ordering::Acquire))] += 1;
        }
        (counts, self.copies.len())
    }

    /// Uses only mapped slots; incomplete and failed mappings cannot be reused.
    fn cursor(slot: &Slot) -> Option<u64> {
        match slot.state.load(Ordering::Acquire) {
            READY => Some(0),
            ACTIVE => Some(slot.offset),
            _ => None,
        }
    }

    /// Continues the newest copy in place, else packs a whole write at WebGPU's mapping
    /// alignment preferring active slots, else splits it into the slot with the most room.
    fn reserve(&self, target: &wgpu::Buffer, offset: u64, bytes: u64) -> Option<Piece> {
        let slot_bytes = self.shape.slot_bytes;
        if let Some(newest) = self.copies.last()
            && newest.target == *target
            && newest.offset + newest.bytes == offset
            && self.slots[newest.slot].offset == newest.source + newest.bytes
        {
            let start = newest.source + newest.bytes;
            let room = slot_bytes.saturating_sub(start) & !(wgpu::COPY_BUFFER_ALIGNMENT - 1);
            if room > 0 {
                return Some(Piece {
                    slot: newest.slot,
                    start,
                    bytes: room.min(bytes),
                    extends: true,
                });
            }
        }
        let aligned = |slot: &Slot| {
            Self::cursor(slot).map(|cursor| {
                let occupied = cursor > 0;
                (
                    occupied,
                    cursor.div_ceil(wgpu::MAP_ALIGNMENT) * wgpu::MAP_ALIGNMENT,
                )
            })
        };
        for occupied in [true, false] {
            for (index, slot) in self.slots.iter().enumerate() {
                if let Some((used, start)) = aligned(slot)
                    && used == occupied
                    && start
                        .checked_add(bytes)
                        .is_some_and(|end| end <= slot_bytes)
                {
                    return Some(Piece {
                        slot: index,
                        start,
                        bytes,
                        extends: false,
                    });
                }
            }
        }
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let (_, start) = aligned(slot)?;
                let room = slot_bytes.checked_sub(start)? & !(wgpu::COPY_BUFFER_ALIGNMENT - 1);
                (room > 0).then_some((index, start, room))
            })
            .max_by_key(|&(index, _, room)| (room, std::cmp::Reverse(index)))
            .map(|(slot, start, room)| Piece {
                slot,
                start,
                bytes: room.min(bytes),
                extends: false,
            })
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
            pool = self.shape.name,
            copies = self.copies.len(),
            bytes = self.copies.iter().map(|copy| copy.bytes).sum::<u64>(),
            total_fallback_writes = self.stats.fallback_writes,
            total_fallback_bytes = self.stats.fallback_bytes,
        )
        .entered();
        for slot in self.slots.iter() {
            if slot.state.load(Ordering::Acquire) == ACTIVE {
                slot.buffer.unmap();
                slot.state.store(PENDING, Ordering::Release);
            }
        }
        self.stats.encoded_copies += self.copies.len() as u64;
        for copy in self.copies.drain(..) {
            encoder.copy_buffer_to_buffer(
                &self.slots[copy.slot].buffer,
                copy.source,
                &copy.target,
                copy.offset,
                copy.bytes,
            );
        }
        for slot in self.slots.iter_mut() {
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

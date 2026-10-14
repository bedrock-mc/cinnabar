//! Reads the culling counters and the pass timestamps back a few frames late, never waiting:
//! a frame whose readback slots are all in flight records none.

use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use render_scene::distant_terrain::{
    DISTANT_TERRAIN_SIZE_BINS, DistantTerrainPass, DistantTerrainRenderStats,
};

use super::{
    gpu::{LodGpu, READBACK_SLOTS, TIMESTAMP_MARKS},
    layout::{ARGS_CLUSTER_BINS, ARGS_DEGENERATE, ARGS_SECTION_BINS, ARGS_STATS},
};
use crate::gpu_timing::readback::ReadbackRing;

const PENDING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// What one mapped slot reported.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Readout {
    pub(crate) visible: u32,
    pub(crate) pass_b: u32,
    pub(crate) translucent: u32,
    pub(crate) overflow: u32,
    pub(crate) bins: SizeBins,
    /// Milliseconds of each pass, when timed.
    pub(crate) gpu_ms: Option<[f32; DistantTerrainPass::COUNT]>,
}

impl Readout {
    /// Copies the readout into `stats`.
    pub(crate) fn apply(&self, stats: &mut DistantTerrainRenderStats) {
        stats.visible_clusters = self.visible;
        stats.pass_b_clusters = self.pass_b;
        stats.translucent_clusters = self.translucent;
        stats.list_overflow = self.overflow;
        stats.section_size_bins = self.bins.sections;
        stats.cluster_size_bins = self.bins.clusters;
        stats.degenerate_slots = self.bins.degenerate;
        if let Some(gpu_ms) = self.gpu_ms {
            stats.gpu_ms = gpu_ms;
        }
    }
}

/// The counters at [`ARGS_STATS`] of a copied args buffer.
pub(crate) fn decode_counters(words: &[u32]) -> [u32; 4] {
    let at = ARGS_STATS as usize;
    [words[at], words[at + 1], words[at + 2], words[at + 3]]
}

/// The screen-size histograms and the empty quad slots of a copied args buffer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SizeBins {
    pub(crate) sections: [u32; DISTANT_TERRAIN_SIZE_BINS],
    pub(crate) clusters: [u32; DISTANT_TERRAIN_SIZE_BINS],
    pub(crate) degenerate: u32,
}

impl SizeBins {
    /// Reads the histograms from the words of a copied args buffer.
    pub(crate) fn decode(words: &[u32]) -> Self {
        let bins = |first: u32| std::array::from_fn(|bin| words[first as usize + bin]);
        Self {
            sections: bins(ARGS_SECTION_BINS),
            clusters: bins(ARGS_CLUSTER_BINS),
            degenerate: words[ARGS_DEGENERATE as usize],
        }
    }
}

/// Milliseconds between consecutive `ticks`, at `period` nanoseconds a tick; zero for a pair
/// that went backwards or was not written.
pub(crate) fn decode_timestamps(ticks: &[u64], period: f32) -> [f32; DistantTerrainPass::COUNT] {
    std::array::from_fn(|pass| {
        let (begin, end) = (ticks[pass], ticks[pass + 1]);
        if begin == 0 || end <= begin {
            0.0
        } else {
            (end - begin) as f32 * period / 1.0e6
        }
    })
}

/// The slots' lifecycle; see the module documentation.
#[derive(Default)]
pub(crate) struct LodReadbacks {
    ring: ReadbackRing,
    states: [Arc<AtomicU8>; READBACK_SLOTS],
}

impl LodReadbacks {
    /// Claims a slot for this frame's copies; `None` while every slot is in flight.
    pub(crate) fn acquire(&mut self) -> Option<usize> {
        self.ring.acquire()
    }

    /// Returns a claimed slot whose copies were never encoded.
    pub(crate) fn release(&mut self, slot: usize) {
        self.ring.release(slot);
    }

    /// Maps `slot`'s buffers once the frame that copied into them completes.
    pub(crate) fn submit(&mut self, slot: usize, gpu: &LodGpu) {
        self.ring.submit(slot);
        let state = Arc::clone(&self.states[slot]);
        state.store(PENDING, Ordering::Relaxed);
        let buffers = &gpu.readbacks[slot];
        let parts = 1 + u8::from(buffers.timestamps.is_some());
        let mapped = Arc::new(AtomicU8::new(0));
        for buffer in std::iter::once(&buffers.stats).chain(buffers.timestamps.as_ref()) {
            let state = Arc::clone(&state);
            let mapped = Arc::clone(&mapped);
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    if result.is_err() {
                        state.store(FAILED, Ordering::Release);
                    } else if mapped.fetch_add(1, Ordering::AcqRel) + 1 == parts {
                        let _ = state.compare_exchange(
                            PENDING,
                            MAPPED,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                    }
                });
        }
    }

    /// Reads and unmaps every completed slot oldest first; the newest readout, if any.
    pub(crate) fn drain(&mut self, gpu: &LodGpu, period: f32) -> Option<Readout> {
        let mut latest = None;
        while let Some(slot) = self.ring.oldest_in_flight() {
            let state = self.states[slot].load(Ordering::Acquire);
            if state == PENDING {
                break;
            }
            let buffers = &gpu.readbacks[slot];
            if state == MAPPED {
                let (counters, bins) = {
                    let view = buffers.stats.slice(..).get_mapped_range().expect("mapped");
                    let words: &[u32] = bytemuck::cast_slice(&view);
                    (decode_counters(words), SizeBins::decode(words))
                };
                let gpu_ms = buffers.timestamps.as_ref().map(|buffer| {
                    let view = buffer.slice(..).get_mapped_range().expect("mapped");
                    let ticks: &[u64] = bytemuck::cast_slice(&view);
                    decode_timestamps(&ticks[..TIMESTAMP_MARKS as usize], period)
                });
                latest = Some(Readout {
                    visible: counters[0],
                    pass_b: counters[1],
                    translucent: counters[2],
                    overflow: counters[3],
                    bins,
                    gpu_ms,
                });
            }
            buffers.stats.unmap();
            if let Some(buffer) = &buffers.timestamps {
                buffer.unmap();
            }
            self.ring.release(slot);
        }
        latest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distant_terrain::layout::ARGS_WORDS;

    #[test]
    fn counters_and_timestamps_decode_from_their_words() {
        let mut words = [0_u32; ARGS_WORDS as usize];
        words[ARGS_STATS as usize..ARGS_STATS as usize + 4].copy_from_slice(&[9, 4, 2, 1]);
        assert_eq!(decode_counters(&words), [9, 4, 2, 1]);
        words[ARGS_SECTION_BINS as usize + 1] = 5;
        words[ARGS_CLUSTER_BINS as usize + 7] = 6;
        words[ARGS_DEGENERATE as usize] = 7;
        let bins = SizeBins::decode(&words);
        assert_eq!(
            (bins.sections[1], bins.clusters[7], bins.degenerate),
            (5, 6, 7)
        );
        assert_eq!(bins.sections.iter().sum::<u32>(), 5);
        let ticks = [100, 1_100, 1_100, 3_100, 0, 10];
        let ms = decode_timestamps(&ticks, 1_000.0);
        assert_eq!(ms[0], 1.0);
        assert_eq!(ms[1], 0.0, "no time passed");
        assert_eq!(ms[2], 2.0);
        assert_eq!(ms[3], 0.0, "went backwards");
        assert_eq!(ms[4], 0.0, "not written");
    }
}

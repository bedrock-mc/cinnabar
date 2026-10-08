//! Bounded developer-only cave timings and deterministic traversal counters.

use std::{collections::VecDeque, time::Duration};

use chunk_pipeline::CaveVisibilityWork;
use serde_json::{Value, json};
use world::SubChunkKey;

use super::CaveVisibilityCache;

const FRAME_CAPACITY: usize = 2048;

#[derive(Clone, Copy)]
struct CaveFrame {
    frame: u64,
    camera: Option<SubChunkKey>,
    elapsed: Duration,
    work: CaveVisibilityWork,
}

pub(super) struct CaveFrameTrace {
    frames: VecDeque<CaveFrame>,
    next_frame: u64,
}

impl Default for CaveFrameTrace {
    /// Reserves the complete bounded trace before any measured updates.
    fn default() -> Self {
        Self {
            frames: VecDeque::with_capacity(FRAME_CAPACITY),
            next_frame: 0,
        }
    }
}

impl CaveFrameTrace {
    /// Retains the latest frames without allocation or file I/O during updates.
    pub(super) fn record(
        &mut self,
        camera: Option<SubChunkKey>,
        work: CaveVisibilityWork,
        elapsed: Duration,
    ) {
        if self.frames.len() == FRAME_CAPACITY {
            self.frames.pop_front();
        }
        self.frames.push_back(CaveFrame {
            frame: self.next_frame,
            camera,
            elapsed,
            work,
        });
        self.next_frame += 1;
    }
}

impl CaveVisibilityCache {
    /// Serializes the bounded trace only when a developer requests a state snapshot.
    pub(crate) fn telemetry_snapshot(&self) -> Value {
        let frames: Vec<_> = self
            .telemetry
            .frames
            .iter()
            .map(|sample| {
                json!({
                    "frame": sample.frame,
                    "camera": sample.camera.map(|key| [key.dimension, key.x, key.y, key.z]),
                    "elapsed_us": sample.elapsed.as_secs_f64() * 1e6,
                    "explored_exits": sample.work.explored_exits,
                    "proof_exits": sample.work.proof_exits,
                    "additions": sample.work.additions,
                    "rebuilt": sample.work.rebuilt,
                })
            })
            .collect();
        json!({
            "capacity": FRAME_CAPACITY,
            "next_frame": self.telemetry.next_frame,
            "frames": frames,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ring rollover preserves ordered samples and the reserved storage.
    #[test]
    fn rollover_retains_recent_frames_without_growing() {
        let mut trace = CaveFrameTrace::default();
        let capacity = trace.frames.capacity();
        for index in 0..FRAME_CAPACITY * 3 {
            trace.record(
                None,
                CaveVisibilityWork::default(),
                Duration::from_nanos(index as u64),
            );
        }
        assert_eq!(trace.frames.capacity(), capacity);
        assert_eq!(trace.frames.len(), FRAME_CAPACITY);
        assert_eq!(
            trace.frames.front().unwrap().frame,
            (FRAME_CAPACITY * 2) as u64
        );
        assert_eq!(
            trace.frames.back().unwrap().frame,
            (FRAME_CAPACITY * 3 - 1) as u64
        );
        assert!(
            trace
                .frames
                .iter()
                .all(|sample| sample.work == CaveVisibilityWork::default())
        );
    }
}

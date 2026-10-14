//! Metal completion latency is a CPU observation, not GPU execution time or utilization.
//! The interval includes submission, queued work and callback delivery by a device poll.

use super::{
    health::QueryHealth,
    readback::{ReadbackRing, SLOTS},
};
use bevy::{
    prelude::*,
    render::{
        Render, RenderStartup,
        renderer::{RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue},
    },
};
use render_scene::{GpuFrameTimes, RuntimeStageProfiler};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const PENDING_LATENCY: u64 = u64::MAX;

struct LatencySlot {
    elapsed: Arc<AtomicU64>,
    started: Option<Instant>,
}

#[derive(Resource)]
struct MetalFrames {
    ring: ReadbackRing,
    slots: [LatencySlot; SLOTS],
    recording: Option<usize>,
    health: Option<QueryHealth>,
}

impl Default for MetalFrames {
    /// Creates fixed-capacity callback storage, independent of GPU query support.
    fn default() -> Self {
        Self {
            ring: ReadbackRing::default(),
            slots: std::array::from_fn(|_| LatencySlot {
                elapsed: Arc::new(AtomicU64::new(PENDING_LATENCY)),
                started: None,
            }),
            recording: None,
            health: QueryHealth::requested("metal_completion_latency"),
        }
    }
}

impl MetalFrames {
    /// Publishes completed callbacks oldest first and claims one bounded slot for this frame.
    fn begin(&mut self, now: Instant, mut sink: impl FnMut(&GpuFrameTimes)) {
        while let Some(slot) = self.ring.oldest_in_flight() {
            let nanos = self.slots[slot].elapsed.load(Ordering::Acquire);
            if nanos == PENDING_LATENCY {
                break;
            }
            if let Some(health) = &mut self.health {
                health.completion_latency();
            }
            let frame = GpuFrameTimes::completion_latency(Duration::from_nanos(nanos));
            #[cfg(feature = "tracy")]
            super::tracy::record(&frame);
            sink(&frame);
            self.ring.release(slot);
        }
        self.recording = self.ring.acquire();
        if let Some(slot) = self.recording {
            self.slots[slot]
                .elapsed
                .store(PENDING_LATENCY, Ordering::Relaxed);
            self.slots[slot].started = Some(now);
        }
        if let Some(health) = &mut self.health {
            health.frame(self.recording.is_none());
        }
    }

    /// Retains a completion callback for submissions already issued, with no GPU wait.
    fn submit(&mut self, queue: &RenderQueue) {
        let Some(slot) = self.recording.take() else {
            return;
        };
        let started = self.slots[slot]
            .started
            .take()
            .expect("recording latency slot");
        let elapsed = self.slots[slot].elapsed.clone();
        crate::device_poll::on_frame_complete(queue, move || {
            complete(&elapsed, started, Instant::now());
        });
        self.ring.submit(slot);
    }
}

/// Publishes a callback receipt interval while reserving the pending sentinel.
fn complete(elapsed: &AtomicU64, started: Instant, now: Instant) {
    let nanos = u64::try_from(now.saturating_duration_since(started).as_nanos())
        .unwrap_or(PENDING_LATENCY - 1)
        .min(PENDING_LATENCY - 1);
    elapsed.store(nanos, Ordering::Release);
}

/// Adds delayed latency reporting only on Metal, without native command-buffer markers.
pub(super) fn install(app: &mut SubApp) {
    app.add_systems(RenderStartup, initialize);
}

/// Checks the backend and brackets the normal graph submission with its completion callback.
fn initialize(world: &mut World) {
    // SAFETY: This only checks the backend; no device state or ownership is changed.
    let metal = unsafe {
        world
            .resource::<RenderDevice>()
            .wgpu_device()
            .as_hal::<wgpu::hal::api::Metal>()
    }
    .is_some();
    if !metal {
        return;
    }
    #[cfg(feature = "tracy")]
    super::tracy::initialize();
    world.init_resource::<MetalFrames>();
    world.schedule_scope(RenderGraph, |_, schedule| {
        schedule.add_systems(
            begin_frame
                .after(RenderGraphSystems::Render)
                .before(RenderGraphSystems::Submit),
        );
    });
    world.schedule_scope(Render, |_, schedule| {
        schedule.add_systems(end_frame.in_set(crate::device_poll::FrameSubmissions));
    });
}

/// Records admission before submission; prior callbacks are consumed on this render thread.
fn begin_frame(mut frames: ResMut<MetalFrames>, profiler: Res<RuntimeStageProfiler>) {
    frames.begin(Instant::now(), |frame| profiler.record_gpu_frame(frame));
}

/// Requests completion after all frame submissions, before the render world's existing poll.
fn end_frame(mut frames: ResMut<MetalFrames>, queue: Res<RenderQueue>) {
    frames.submit(&queue);
}

#[cfg(test)]
mod tests;

//! Metal whole-frame GPU time comes from two empty marker command buffers that open and close the
//! render graph's submission. Metal stamps an empty buffer only once every earlier buffer in its
//! queue has finished, so the span runs from the later of submission and the end of earlier work
//! to the end of the frame's work. Completion latency stays a separate CPU observation that also
//! includes queueing and callback delivery by a device poll.

use super::{
    health::QueryHealth,
    readback::{ReadbackRing, SLOTS},
};
use bevy::{
    prelude::*,
    render::{
        Render, RenderStartup,
        diagnostic::resolve_encoder,
        renderer::{
            PendingCommandBuffers, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue,
        },
    },
};
use objc2::{Message, rc::Retained, runtime::ProtocolObject};
use objc2_metal::{MTLCommandBuffer, MTLCommandBufferStatus};
use render_scene::{GpuFrameTimes, RuntimeStage, RuntimeStageProfiler};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const PENDING_LATENCY: u64 = u64::MAX;

/// A retained marker command buffer, read only after wgpu commits it.
struct Marker(Retained<ProtocolObject<dyn MTLCommandBuffer>>);

// SAFETY: only `status` and the GPU host times are read, which Metal allows from any thread.
unsafe impl Send for Marker {}
// SAFETY: as above; the marker is never encoded into or committed through this handle.
unsafe impl Sync for Marker {}

/// A marker's host time in nanoseconds once Metal has finished with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerState {
    Pending,
    Failed,
    Done(u64),
}

impl Marker {
    /// Creates an empty command buffer to place first or last among a frame's submissions.
    fn new(device: &RenderDevice, label: &'static str) -> Option<(wgpu::CommandBuffer, Self)> {
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
        // SAFETY: nothing is encoded into the raw buffer; wgpu ends and commits it as usual.
        let raw = unsafe {
            encoder.as_hal_mut::<wgpu::hal::api::Metal, _, _>(|encoder| {
                encoder
                    .and_then(|encoder| encoder.raw_command_buffer())
                    .map(Message::retain)
            })
        }?;
        Some((encoder.finish(), Self(raw)))
    }

    /// Reads a completed marker's GPU time without waiting for outstanding work.
    fn read(&self, time: fn(&ProtocolObject<dyn MTLCommandBuffer>) -> f64) -> MarkerState {
        let status = self.0.status();
        if status == MTLCommandBufferStatus::Completed {
            MarkerState::Done(host_nanos(time(&self.0)))
        } else if status == MTLCommandBufferStatus::Error {
            MarkerState::Failed
        } else {
            MarkerState::Pending
        }
    }
}

/// Converts Metal's host seconds; a missing time stays zero so span validation rejects it.
fn host_nanos(seconds: f64) -> u64 {
    (seconds * 1e9).round() as u64
}

/// `None` while either marker is pending; then the span when both completed.
fn settled(begin: MarkerState, end: MarkerState) -> Option<Option<(u64, u64)>> {
    match (begin, end) {
        (MarkerState::Pending, _) | (_, MarkerState::Pending) => None,
        (MarkerState::Done(begin), MarkerState::Done(end)) => Some(Some((begin, end))),
        _ => Some(None),
    }
}

struct FrameSlot {
    latency: Arc<AtomicU64>,
    started: Option<Instant>,
    begin: Option<Marker>,
    end: Option<Marker>,
}

impl FrameSlot {
    /// The execution span once both markers settle; frames without markers settle at once.
    fn execution(&self) -> Option<Option<(u64, u64)>> {
        match (&self.begin, &self.end) {
            (Some(begin), Some(end)) => settled(
                begin.read(|buffer| buffer.GPUStartTime()),
                end.read(|buffer| buffer.GPUEndTime()),
            ),
            _ => Some(None),
        }
    }
}

#[derive(Resource)]
struct MetalFrames {
    ring: ReadbackRing,
    slots: [FrameSlot; SLOTS],
    recording: Option<usize>,
    health: Option<QueryHealth>,
}

impl Default for MetalFrames {
    /// Creates fixed-capacity frame storage, independent of GPU query support.
    fn default() -> Self {
        Self {
            ring: ReadbackRing::default(),
            slots: std::array::from_fn(|_| FrameSlot {
                latency: Arc::new(AtomicU64::new(PENDING_LATENCY)),
                started: None,
                begin: None,
                end: None,
            }),
            recording: None,
            health: QueryHealth::requested("metal_frame"),
        }
    }
}

impl MetalFrames {
    /// Publishes settled frames oldest first and claims one bounded slot for this frame.
    fn begin(&mut self, mut sink: impl FnMut(&GpuFrameTimes)) {
        while let Some(index) = self.ring.oldest_in_flight() {
            let slot = &mut self.slots[index];
            let nanos = slot.latency.load(Ordering::Acquire);
            if nanos == PENDING_LATENCY {
                break;
            }
            let Some(execution) = slot.execution() else {
                break;
            };
            (slot.begin, slot.end) = (None, None);
            let mut frame = GpuFrameTimes::completion_latency(Duration::from_nanos(nanos));
            if let Some((begin, end)) = execution {
                frame = frame.with_frame_span(begin, end);
            }
            if let Some(health) = &mut self.health {
                health.completion_latency();
                if let Some((begin, end)) = execution {
                    health.sample(RuntimeStage::GpuFrame, begin, end);
                }
            }
            #[cfg(feature = "tracy")]
            super::tracy::record(&frame);
            sink(&frame);
            self.ring.release(index);
        }
        self.recording = self.ring.acquire();
        if let Some(slot) = self.recording {
            let slot = &mut self.slots[slot];
            slot.latency.store(PENDING_LATENCY, Ordering::Relaxed);
            slot.started = None;
        }
        if let Some(health) = &mut self.health {
            health.frame(self.recording.is_none());
        }
    }

    /// Returns the slot admitted for this frame, or none when the ring is full.
    fn recording(&mut self) -> Option<&mut FrameSlot> {
        self.recording.map(|slot| &mut self.slots[slot])
    }

    /// Retains a completion callback for submissions already issued, with no GPU wait.
    fn submit(&mut self, queue: &RenderQueue) {
        let Some(slot) = self.recording.take() else {
            return;
        };
        let started = self.slots[slot].started.take().unwrap_or_else(Instant::now);
        let latency = self.slots[slot].latency.clone();
        crate::device_poll::on_frame_complete(queue, move || {
            complete(&latency, started, Instant::now());
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

/// Adds frame timing only on Metal.
pub(super) fn install(app: &mut SubApp) {
    app.add_systems(RenderStartup, initialize);
}

/// Checks the backend and brackets the graph's submissions with the frame markers.
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
        schedule.add_systems((
            open_frame.before(RenderGraphSystems::Begin),
            close_frame
                .after(RenderGraphSystems::Render)
                .after(resolve_encoder)
                .before(RenderGraphSystems::Submit),
        ));
    });
    world.schedule_scope(Render, |_, schedule| {
        schedule.add_systems(end_frame.in_set(crate::device_poll::FrameSubmissions));
    });
}

/// Publishes settled frames and queues the opening marker ahead of every graph command buffer.
fn open_frame(
    mut frames: ResMut<MetalFrames>,
    profiler: Res<RuntimeStageProfiler>,
    device: Res<RenderDevice>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    frames.begin(|frame| profiler.record_gpu_frame(frame));
    if let Some(slot) = frames.recording()
        && let Some((buffer, marker)) = Marker::new(&device, "gpu frame begin")
    {
        pending.push([buffer]);
        slot.begin = Some(marker);
    }
}

/// Starts the latency interval and queues the closing marker after every graph command buffer.
fn close_frame(
    mut frames: ResMut<MetalFrames>,
    device: Res<RenderDevice>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let Some(slot) = frames.recording() else {
        return;
    };
    slot.started = Some(Instant::now());
    if let Some((buffer, marker)) = Marker::new(&device, "gpu frame end") {
        pending.push([buffer]);
        slot.end = Some(marker);
    }
}

/// Requests completion after all frame submissions, before the render world's existing poll.
fn end_frame(mut frames: ResMut<MetalFrames>, queue: Res<RenderQueue>) {
    frames.submit(&queue);
}

#[cfg(test)]
mod tests;

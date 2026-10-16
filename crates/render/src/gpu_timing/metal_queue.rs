//! Opt-in queue diagnostics and a native completion bound, independent of callback delivery.

use super::metal_frame::Marker;
use bevy::{
    prelude::*,
    render::{
        Render,
        renderer::{RenderDevice, RenderQueue},
    },
};
use objc2_metal::{MTLCommandBuffer, MTLCommandBufferStatus};
use objc2_quartz_core::CACurrentMediaTime;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

const REPORT_EVERY: u64 = 16;

struct Frame {
    marker: Marker,
    submitted: Instant,
    host_submitted: f64,
    callback: Arc<AtomicU64>,
    native_ns: Option<u64>,
}

#[derive(Resource)]
struct QueueProbe {
    limit: Option<usize>,
    frames: VecDeque<Frame>,
    completed: u64,
}

/// Installs diagnostics only when explicitly requested; normal presentation is unchanged.
pub(super) fn install(world: &mut World) {
    let limit = env_compat::var("CINNABAR_GPU_FRAMES_IN_FLIGHT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| (1..=3).contains(n));
    if limit.is_none() && env_compat::var("CINNABAR_GPU_QUEUE_PROBE").as_deref() != Ok("1") {
        return;
    }
    world.insert_resource(QueueProbe {
        limit,
        frames: VecDeque::new(),
        completed: 0,
    });
    world.schedule_scope(Render, |_, schedule| {
        schedule.add_systems(probe.in_set(crate::device_poll::FrameSubmissions));
    });
}

/// Observes native completion separately from wgpu's device-poll callback and bounds queued work.
#[expect(clippy::print_stderr, reason = "opt-in GPU queue probe output")]
fn probe(mut state: ResMut<QueueProbe>, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let pending = state
        .frames
        .iter()
        .filter(|f| f.native_ns.is_none())
        .count();
    for frame in &mut state.frames {
        if frame.native_ns.is_none() && frame.marker.0.status() == MTLCommandBufferStatus::Completed
        {
            frame.native_ns = Some(frame.submitted.elapsed().as_nanos() as u64);
        }
    }
    while state
        .frames
        .front()
        .is_some_and(|f| f.native_ns.is_some() && f.callback.load(Ordering::Acquire) != 0)
    {
        let frame = state.frames.pop_front().unwrap();
        state.completed += 1;
        if state.completed.is_multiple_of(REPORT_EVERY) {
            eprintln!(
                "CINNABAR_GPU_QUEUE {}",
                serde_json::json!({
                    "host_unix_ns": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64,
                    "sample_every_frames": REPORT_EVERY,
                    "native_observed_ms": frame.native_ns.unwrap() as f64 / 1e6,
                    "callback_ms": frame.callback.load(Ordering::Acquire) as f64 / 1e6,
                    "gpu_finish_after_submit_ms": (frame.marker.0.GPUEndTime() >= frame.host_submitted).then(|| (frame.marker.0.GPUEndTime() - frame.host_submitted) * 1e3),
                    "gpu_marker_ms": (frame.marker.0.GPUEndTime() - frame.marker.0.GPUStartTime()) * 1e3,
                    "pending_frames": pending,
                    "limit": state.limit,
                })
            );
        }
    }
    // Bound diagnostic retention even if callbacks never arrive; this never bounds normal work.
    if state.frames.len() == 256 {
        state.frames.pop_front();
    }
    let Some((buffer, marker)) = Marker::new(&device, "queue latency probe") else {
        return;
    };
    let host_submitted = CACurrentMediaTime();
    let submitted = Instant::now();
    queue.submit([buffer]);
    let callback = Arc::new(AtomicU64::new(0));
    let receipt = callback.clone();
    crate::device_poll::on_frame_complete(&queue, move || {
        receipt.store(
            (submitted.elapsed().as_nanos() as u64).max(1),
            Ordering::Release,
        );
    });
    state.frames.push_back(Frame {
        marker,
        submitted,
        host_submitted,
        callback,
        native_ns: None,
    });
    if let Some(limit) = state.limit {
        while state
            .frames
            .iter()
            .filter(|f| f.native_ns.is_none())
            .count()
            >= limit
        {
            let frame = state
                .frames
                .iter_mut()
                .find(|f| f.native_ns.is_none())
                .unwrap();
            frame.marker.0.waitUntilCompleted();
            frame.native_ns = Some(frame.submitted.elapsed().as_nanos() as u64);
        }
    }
}

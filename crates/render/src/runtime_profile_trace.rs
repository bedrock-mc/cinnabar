//! Bounded, opt-in spans exported on the exit frame without per-frame file I/O.

use crate::runtime_profile::RuntimeStage;
use crate::runtime_profile_slow::SlowFrameEvent;
use bevy::platform::time::Instant;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::ThreadId,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const TRACE_CAPACITY: usize = 131_072;
const MAX_TRACE_CAPACITY: usize = 2_097_152;
const TRACE_CAPACITY_ENV: &str = "RUST_MCBE_STAGE_PROFILE_EVENTS";

/// Invalid or excessive requests retain the default bounded recording budget.
fn trace_capacity(value: Option<&str>) -> usize {
    value
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|capacity| (1..=MAX_TRACE_CAPACITY).contains(capacity))
        .unwrap_or(TRACE_CAPACITY)
}

#[derive(Debug)]
struct TraceEvent {
    name: &'static str,
    started: Duration,
    elapsed: Duration,
    thread: ThreadId,
    args: TraceArgs,
}

#[derive(Debug)]
enum TraceArgs {
    None,
    Focus {
        focused: bool,
        occluded: bool,
        game_seconds: f64,
    },
    SlowFrame(SlowFrameEvent),
    GpuSample {
        sequence: u64,
        nanos: u64,
    },
}

#[derive(Debug)]
pub(crate) struct FrameTrace {
    path: PathBuf,
    epoch: Instant,
    epoch_unix_nanos: u128,
    dropped: AtomicU64,
    gpu_sequence: AtomicU64,
    flushed: AtomicBool,
    events: Mutex<Vec<TraceEvent>>,
}

impl FrameTrace {
    /// Preallocates a fixed recording budget; full traces drop subsequent spans.
    pub(crate) fn new(path: PathBuf, epoch: Instant) -> Self {
        let capacity = trace_capacity(std::env::var(TRACE_CAPACITY_ENV).ok().as_deref());
        Self::with_capacity(path, epoch, capacity)
    }

    /// Allocates storage once and anchors its monotonic epoch to the capture's wall clock.
    fn with_capacity(path: PathBuf, epoch: Instant, capacity: usize) -> Self {
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            path,
            epoch,
            epoch_unix_nanos: unix.saturating_sub(epoch.elapsed()).as_nanos(),
            dropped: AtomicU64::new(0),
            gpu_sequence: AtomicU64::new(0),
            flushed: AtomicBool::new(false),
            events: Mutex::new(Vec::with_capacity(capacity)),
        }
    }

    /// Records a stage on its executing thread without doing file I/O.
    pub(crate) fn record(&self, stage: RuntimeStage, started: Instant, elapsed: Duration) {
        self.push(TraceEvent {
            name: stage.name(),
            started: started.saturating_duration_since(self.epoch),
            elapsed,
            thread: std::thread::current().id(),
            args: TraceArgs::None,
        });
    }

    /// Records focus at the start of each main frame, including time between updates.
    pub(crate) fn frame(&self, focused: bool, occluded: bool, game_seconds: f64) {
        self.push(TraceEvent {
            name: "frame_start",
            started: self.epoch.elapsed(),
            elapsed: Duration::ZERO,
            thread: std::thread::current().id(),
            args: TraceArgs::Focus {
                focused,
                occluded,
                game_seconds,
            },
        });
    }

    /// Marks every slow frame, including those whose text was rate-limited.
    pub(crate) fn slow_frame(&self, event: SlowFrameEvent) {
        self.push(TraceEvent {
            name: "slow_frame",
            started: self.epoch.elapsed(),
            elapsed: Duration::ZERO,
            thread: std::thread::current().id(),
            args: TraceArgs::SlowFrame(event),
        });
    }

    /// GPU samples share their readback receipt time; this is not a GPU execution timestamp.
    pub(crate) fn gpu_frame(&self, frame: &crate::GpuFrameTimes) {
        let sequence = self.gpu_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let started = self.epoch.elapsed();
        let thread = std::thread::current().id();
        for (stage, elapsed) in frame.iter() {
            self.push(TraceEvent {
                name: stage.name(),
                started,
                elapsed: Duration::ZERO,
                thread,
                args: TraceArgs::GpuSample {
                    sequence,
                    nanos: u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX),
                },
            });
        }
    }

    /// Appends only while the preallocated capacity still has room.
    fn push(&self, event: TraceEvent) {
        let mut events = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if events.len() < events.capacity() {
            events.push(event);
        } else {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    /// Writes the buffered spans once, after updates stop and before shutdown waits.
    pub(crate) fn flush(&self) {
        if self.flushed.swap(true, Ordering::AcqRel) {
            return;
        }
        let events = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = std::fs::File::create(&self.path).and_then(|file| {
            use std::io::Write;
            let mut writer = std::io::BufWriter::new(file);
            write!(
                writer,
                "{{\"capacity\":{},\"truncated\":{},\"dropped_events\":{},\"epoch_unix_nanos\":{},\"traceEvents\":[",
                events.capacity(),
                self.dropped.load(Ordering::Relaxed) != 0,
                self.dropped.load(Ordering::Relaxed),
                self.epoch_unix_nanos
            )?;
            let mut threads = Vec::new();
            for (index, event) in events.iter().enumerate() {
                if index != 0 {
                    writer.write_all(b",")?;
                }
                let tid = match threads.iter().position(|id| *id == event.thread) {
                    Some(index) => index,
                    None => {
                        threads.push(event.thread);
                        threads.len() - 1
                    }
                };
                let mut record = json!({"name": event.name, "ph": "X", "pid": std::process::id(),
                    "tid": tid, "ts": event.started.as_secs_f64() * 1e6,
                    "dur": event.elapsed.as_secs_f64() * 1e6});
                let args = match &event.args {
                    TraceArgs::None => None,
                    TraceArgs::Focus { focused, occluded, game_seconds } => {
                        Some(json!({"focused": focused, "occluded": occluded, "game_seconds": game_seconds}))
                    }
                    TraceArgs::SlowFrame(slow) => Some(json!({
                        "violations": slow.reasons(),
                        "frame_ms": slow.frame.as_secs_f64() * 1e3,
                    })),
                    TraceArgs::GpuSample { sequence, nanos } => Some(json!({
                        "sample_sequence": sequence,
                        "duration_ns": nanos,
                    })),
                };
                if let Some(args) = args {
                    if matches!(event.args, TraceArgs::GpuSample { .. }) {
                        record["ph"] = json!("C");
                        record["cat"] = json!("gpu_readback");
                        record["timestamp_kind"] = json!("readback_received");
                    } else {
                        record["ph"] = json!("i");
                        record["s"] = json!("t");
                    }
                    record["args"] = args;
                }
                serde_json::to_writer(&mut writer, &record).map_err(std::io::Error::other)?;
            }
            writer.write_all(b"]}")?;
            writer.flush()
        });
        if let Err(error) = result {
            eprintln!("write stage frame trace {}: {error}", self.path.display());
        }
    }
}

impl Drop for FrameTrace {
    fn drop(&mut self) {
        self.flush();
    }
}

/// Brackets surface preparation to distinguish drawable waits from game work.
pub(crate) fn install_surface_trace(app: &mut bevy::app::SubApp) {
    use bevy::prelude::*;
    use bevy::render::{
        Render, RenderSystems, renderer::render_system, view::window::prepare_windows,
    };
    const SUBMISSION: usize = RuntimeStage::RenderSubmission as usize;
    const SURFACE: usize = RuntimeStage::SurfacePreparation as usize;
    const FRAME: usize = RuntimeStage::RenderFrame as usize;
    app.init_resource::<crate::RuntimeStageSpans>()
        .add_systems(
            Render,
            (
                crate::begin_stage_span::<SURFACE>.before(prepare_windows),
                crate::end_stage_span::<SURFACE>.after(prepare_windows),
            )
                .in_set(RenderSystems::ManageViews),
        )
        // Spans all render-world work; the acquisition wait is subtracted at the end.
        .add_systems(
            Render,
            (
                crate::begin_stage_span::<FRAME>.before(RenderSystems::ExtractCommands),
                crate::runtime_profile::end_render_frame_span.in_set(RenderSystems::Cleanup),
            ),
        )
        .add_systems(
            Render,
            (
                crate::begin_stage_span::<SUBMISSION>.before(render_system),
                crate::end_stage_span::<SUBMISSION>.after(render_system),
            )
                .in_set(RenderSystems::Render),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Keeps the recording bounded while preserving timing and window state.
    fn trace_preserves_thread_spans_and_focus_without_growing() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("trace.json");
        let trace = FrameTrace::with_capacity(path.clone(), Instant::now(), TRACE_CAPACITY);
        trace.frame(false, false, 1.5);
        trace.slow_frame(SlowFrameEvent {
            reasons: 0b1001,
            frame: Duration::from_millis(12),
        });
        trace.record(
            RuntimeStage::WorldStream,
            Instant::now(),
            Duration::from_millis(2),
        );
        for _ in 0..TRACE_CAPACITY {
            trace.frame(true, false, 2.0);
        }
        assert_eq!(trace.events.lock().unwrap().len(), TRACE_CAPACITY);
        drop(trace);
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["truncated"], true);
        assert_eq!(saved["dropped_events"], 3);
        assert_eq!(saved["traceEvents"][0]["args"]["game_seconds"], 1.5);
        assert_eq!(saved["traceEvents"][0]["args"]["focused"], false);
        assert_eq!(
            saved["traceEvents"][1]["args"]["violations"],
            "interval+gpu"
        );
        assert_eq!(saved["traceEvents"][2]["dur"], 2000.0);
        assert_eq!(
            saved["traceEvents"][0]["tid"],
            saved["traceEvents"][2]["tid"]
        );
    }

    #[test]
    fn recording_capacity_is_bounded_and_never_grows() {
        assert_eq!(trace_capacity(Some("0")), TRACE_CAPACITY);
        assert_eq!(trace_capacity(Some("invalid")), TRACE_CAPACITY);
        assert_eq!(
            trace_capacity(Some(&(MAX_TRACE_CAPACITY + 1).to_string())),
            TRACE_CAPACITY
        );
        assert_eq!(
            trace_capacity(Some(&MAX_TRACE_CAPACITY.to_string())),
            MAX_TRACE_CAPACITY
        );
        let root = tempfile::tempdir().unwrap();
        let trace = FrameTrace::with_capacity(root.path().join("bounded.json"), Instant::now(), 2);
        let pointer = trace.events.lock().unwrap().as_ptr();
        for _ in 0..2 {
            trace.frame(false, false, 0.0);
        }
        assert_eq!(trace.dropped.load(Ordering::Relaxed), 0);
        trace.frame(false, false, 0.0);
        let events = trace.events.lock().unwrap();
        assert_eq!(events.capacity(), 2);
        assert_eq!(events.len(), 2);
        assert_eq!(events.as_ptr(), pointer);
        assert_eq!(trace.dropped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn gpu_samples_share_receipt_time_without_growing_or_faking_cpu_spans() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("gpu.json");
        let trace = FrameTrace::with_capacity(path.clone(), Instant::now(), 6);
        let pointer = trace.events.lock().unwrap().as_ptr();
        let frame = crate::gpu_timing::decode_spans(
            [
                (RuntimeStage::GpuUi, 10, 30),
                (RuntimeStage::GpuHand, 40, 70),
            ],
            2.0,
            true,
        );
        trace.gpu_frame(&frame);
        trace.gpu_frame(&frame);
        trace.gpu_frame(&frame);
        {
            let events = trace.events.lock().unwrap();
            assert_eq!(events.len(), 6);
            assert_eq!(events.capacity(), 6);
            assert_eq!(events.as_ptr(), pointer);
            assert_eq!(trace.dropped.load(Ordering::Relaxed), 3);
        }
        trace.flush();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let samples = saved["traceEvents"].as_array().unwrap();
        for frame in samples.chunks_exact(3) {
            assert!(frame.iter().all(|sample| sample["ts"] == frame[0]["ts"]));
            assert!(frame.iter().all(|sample| sample["ph"] == "C"));
            assert!(frame.iter().all(|sample| sample["dur"] == 0.0));
            assert!(
                frame
                    .iter()
                    .all(|sample| sample["timestamp_kind"] == "readback_received")
            );
        }
        assert_eq!(samples[0]["name"], "gpu_frame");
        assert_eq!(samples[0]["args"]["duration_ns"], 120);
        assert_eq!(samples[1]["args"]["duration_ns"], 40);
        assert_eq!(samples[2]["args"]["duration_ns"], 60);
        assert_eq!(samples[0]["args"]["sample_sequence"], 1);
        assert_eq!(samples[3]["args"]["sample_sequence"], 2);
    }
}

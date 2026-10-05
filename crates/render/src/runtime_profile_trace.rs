//! Bounded, opt-in spans exported on the exit frame without per-frame file I/O.

use crate::runtime_profile::RuntimeStage;
use crate::runtime_profile_slow::SlowFrameEvent;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::ThreadId,
    time::{Duration, Instant},
};

const TRACE_CAPACITY: usize = 131_072;

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
    Focus { focused: bool, occluded: bool },
    SlowFrame(SlowFrameEvent),
}

#[derive(Debug)]
pub(crate) struct FrameTrace {
    path: PathBuf,
    epoch: Instant,
    flushed: AtomicBool,
    events: Mutex<Vec<TraceEvent>>,
}

impl FrameTrace {
    /// Preallocates a fixed recording budget; full traces drop subsequent spans.
    pub(crate) fn new(path: PathBuf, epoch: Instant) -> Self {
        Self {
            path,
            epoch,
            flushed: AtomicBool::new(false),
            events: Mutex::new(Vec::with_capacity(TRACE_CAPACITY)),
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
    pub(crate) fn frame(&self, focused: bool, occluded: bool) {
        self.push(TraceEvent {
            name: "frame_start",
            started: self.epoch.elapsed(),
            elapsed: Duration::ZERO,
            thread: std::thread::current().id(),
            args: TraceArgs::Focus { focused, occluded },
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

    /// Appends only while the preallocated capacity still has room.
    fn push(&self, event: TraceEvent) {
        let mut events = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if events.len() < TRACE_CAPACITY {
            events.push(event);
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
                "{{\"capacity\":{TRACE_CAPACITY},\"truncated\":{},\"traceEvents\":[",
                events.len() == TRACE_CAPACITY
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
                    TraceArgs::Focus { focused, occluded } => {
                        Some(json!({"focused": focused, "occluded": occluded}))
                    }
                    TraceArgs::SlowFrame(slow) => Some(json!({
                        "violations": slow.reasons(),
                        "frame_ms": slow.frame.as_secs_f64() * 1e3,
                    })),
                };
                if let Some(args) = args {
                    record["ph"] = json!("i");
                    record["s"] = json!("t");
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
                (
                    crate::end_stage_span::<SURFACE>,
                    crate::begin_stage_span::<FRAME>,
                )
                    .chain()
                    .after(prepare_windows),
            )
                .in_set(RenderSystems::ManageViews),
        )
        .add_systems(
            Render,
            crate::end_stage_span::<FRAME>.in_set(RenderSystems::Cleanup),
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
        let trace = FrameTrace::new(path.clone(), Instant::now());
        trace.frame(false, false);
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
            trace.frame(true, false);
        }
        assert_eq!(trace.events.lock().unwrap().len(), TRACE_CAPACITY);
        drop(trace);
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["truncated"], true);
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
}

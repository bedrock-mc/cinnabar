//! Bounded rolling spans and hitch windows; formatting and output happen after unlocking.

use crate::RuntimeStage;
use crate::runtime_profile_slow::SlowFrameEvent;
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::json;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{JoinHandle, ThreadId},
    time::Duration,
};
use web_time::{Instant, SystemTime, UNIX_EPOCH};

const TRACE_CAPACITY: usize = 131_072;
const MAX_TRACE_CAPACITY: usize = 2_097_152;
const TRACE_CAPACITY_ENV: &str = "CINNABAR_STAGE_PROFILE_EVENTS";
/// Normal play retains recent completed spans even without an export path.
const GAMEPLAY_CAPACITY: usize = 4096;
const HITCH_PRE_FRAMES: u64 = 4;
const HITCH_POST_FRAMES: u64 = 2;
const MAX_PENDING_HITCHES: usize = HITCH_POST_FRAMES as usize + 1;
/// Hitch reports go to the session log, so a machine that hitches every frame must not flood it:
/// a burst of reports, then one more per refill period of frames.
const HITCH_REPORT_BURST: u32 = 4;
const HITCH_REPORT_REFILL_FRAMES: u64 = 1800;

/// Invalid or excessive requests retain the default bounded recording budget.
fn trace_capacity(value: Option<&str>) -> usize {
    value
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|capacity| (1..=MAX_TRACE_CAPACITY).contains(capacity))
        .unwrap_or(TRACE_CAPACITY)
}

#[derive(Debug, Clone)]
struct TraceEvent {
    name: &'static str,
    started: Duration,
    elapsed: Duration,
    thread: ThreadId,
    /// Unknown for delayed GPU samples and accumulated worker receipts.
    frame_id: Option<u64>,
    args: TraceArgs,
}

#[derive(Debug, Clone)]
enum TraceArgs {
    None,
    Focus {
        focused: bool,
        occluded: bool,
        game_seconds: f64,
    },
    SlowFrame(SlowFrameEvent),
    BackgroundSample {
        nanos: u64,
    },
    GpuSample {
        sequence: u64,
        nanos: u64,
    },
}

#[derive(Debug)]
struct PendingHitch {
    frame_id: u64,
    sequence: u64,
}

#[derive(Debug)]
struct TraceWindow {
    events: VecDeque<TraceEvent>,
    next_sequence: u64,
    pending: VecDeque<PendingHitch>,
    dropped_hitches: u64,
    report_budget: u32,
    frames_until_refill: u64,
    suppressed_reports: u64,
}

impl TraceWindow {
    /// Counts evicted records independently of the retained window's contents.
    fn overwritten(&self) -> u64 {
        self.next_sequence.saturating_sub(self.events.len() as u64)
    }

    /// Evicts the oldest completed record without growing the ring.
    fn push(&mut self, event: TraceEvent) {
        if self.events.capacity() == 0 {
            return; // Export has taken ownership of this recording.
        }
        if self.events.len() == self.events.capacity() {
            self.events.pop_front();
        }
        self.events.push_back(event);
        self.next_sequence += 1;
    }
}

#[derive(Debug)]
pub struct FrameTrace {
    path: Option<PathBuf>,
    epoch: Instant,
    epoch_unix_nanos: u128,
    gpu_sequence: AtomicU64,
    flushed: AtomicBool,
    events: Mutex<TraceWindow>,
    export: Mutex<Option<JoinHandle<()>>>,
}

impl FrameTrace {
    /// Preallocates recent history, using the larger recording budget for explicit exports.
    pub fn new(path: Option<PathBuf>, epoch: Instant) -> Self {
        let capacity = if path.is_some() {
            trace_capacity(env_compat::var(TRACE_CAPACITY_ENV).ok().as_deref())
        } else {
            GAMEPLAY_CAPACITY
        };
        Self::build(path, epoch, capacity)
    }

    /// Makes a small export ring for deterministic tests.
    #[cfg(test)]
    fn with_capacity(path: PathBuf, epoch: Instant, capacity: usize) -> Self {
        Self::build(Some(path), epoch, capacity)
    }

    /// Allocates recording storage once and anchors its monotonic epoch to wall time.
    fn build(path: Option<PathBuf>, epoch: Instant, capacity: usize) -> Self {
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            path,
            epoch,
            epoch_unix_nanos: unix.saturating_sub(epoch.elapsed()).as_nanos(),
            gpu_sequence: AtomicU64::new(0),
            flushed: AtomicBool::new(false),
            export: Mutex::new(None),
            events: crate::runtime_profile::initialized_mutex(TraceWindow {
                events: VecDeque::with_capacity(capacity),
                next_sequence: 0,
                pending: VecDeque::with_capacity(MAX_PENDING_HITCHES),
                dropped_hitches: 0,
                report_budget: HITCH_REPORT_BURST,
                frames_until_refill: HITCH_REPORT_REFILL_FRAMES,
                suppressed_reports: 0,
            }),
        }
    }

    /// Records a completed CPU stage with the frame captured when it began.
    pub fn record(&self, frame_id: u64, stage: RuntimeStage, started: Instant, elapsed: Duration) {
        self.push(TraceEvent {
            name: stage.name(),
            started: started.saturating_duration_since(self.epoch),
            elapsed,
            thread: std::thread::current().id(),
            frame_id: Some(frame_id),
            args: TraceArgs::None,
        });
    }

    /// Records accumulated worker time at receipt without inventing a causal frame or span.
    pub fn background_sample(&self, stage: RuntimeStage, elapsed: Duration) {
        self.push(TraceEvent {
            name: stage.name(),
            started: self.epoch.elapsed(),
            elapsed: Duration::ZERO,
            thread: std::thread::current().id(),
            frame_id: None,
            args: TraceArgs::BackgroundSample {
                nanos: nanos(elapsed),
            },
        });
    }

    /// Records input-frame focus and returns every hitch window whose following frames finished.
    pub fn frame(
        &self,
        frame_id: u64,
        focused: bool,
        occluded: bool,
        game_seconds: f64,
    ) -> Vec<HitchReport> {
        self.push(TraceEvent {
            name: "frame_start",
            started: self.epoch.elapsed(),
            elapsed: Duration::ZERO,
            thread: std::thread::current().id(),
            frame_id: Some(frame_id),
            args: TraceArgs::Focus {
                focused,
                occluded,
                game_seconds,
            },
        });
        self.take_hitches(Some(frame_id))
    }

    /// Marks every slow frame and reserves a window for every hard CPU hitch.
    pub fn slow_frame(&self, event: SlowFrameEvent) {
        let mut window = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if event.hard_hitch {
            if window.pending.len() == MAX_PENDING_HITCHES {
                window.pending.pop_front();
                window.dropped_hitches += 1;
            }
            let sequence = window.next_sequence;
            window.pending.push_back(PendingHitch {
                frame_id: event.frame_id,
                sequence,
            });
        }
        window.push(TraceEvent {
            name: "slow_frame",
            started: self.epoch.elapsed(),
            elapsed: Duration::ZERO,
            thread: std::thread::current().id(),
            frame_id: (event.frame_id != 0).then_some(event.frame_id),
            args: TraceArgs::SlowFrame(event),
        });
    }

    /// GPU samples share receipt time; their originating CPU frame is unavailable.
    pub fn gpu_frame(&self, frame: &crate::GpuFrameTimes) {
        let sequence = self.gpu_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let started = self.epoch.elapsed();
        let thread = std::thread::current().id();
        for (stage, elapsed) in frame.iter() {
            self.push(TraceEvent {
                name: stage.name(),
                started,
                elapsed: Duration::ZERO,
                thread,
                frame_id: None,
                args: TraceArgs::GpuSample {
                    sequence,
                    nanos: nanos(elapsed),
                },
            });
        }
    }

    /// Appends one record while holding only the ring lock.
    fn push(&self, event: TraceEvent) {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event);
    }

    /// Copies bounded hitch snapshots; `None` drains incomplete windows during shutdown.
    pub fn take_hitches(&self, boundary: Option<u64>) -> Vec<HitchReport> {
        let mut window = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if boundary.is_some() {
            window.frames_until_refill -= 1;
            if window.frames_until_refill == 0 {
                window.frames_until_refill = HITCH_REPORT_REFILL_FRAMES;
                window.report_budget = (window.report_budget + 1).min(HITCH_REPORT_BURST);
            }
        }
        let mut reports = Vec::new();
        while window
            .pending
            .front()
            .is_some_and(|hitch| boundary.is_none_or(|id| id > hitch.frame_id + HITCH_POST_FRAMES))
        {
            let hitch = window.pending.pop_front().expect("pending hitch");
            if window.report_budget == 0 {
                window.suppressed_reports += 1;
                continue;
            }
            window.report_budget -= 1;
            let first_frame = hitch.frame_id.saturating_sub(HITCH_PRE_FRAMES);
            let snapshot = self.snapshot(&window, GAMEPLAY_CAPACITY, Some(first_frame));
            window.suppressed_reports = 0;
            let pre_complete = hitch.frame_id > HITCH_PRE_FRAMES
                && snapshot.events.iter().any(|event| {
                    event.name == "frame_start"
                        && event.frame_id == Some(hitch.frame_id - HITCH_PRE_FRAMES)
                });
            let post_complete = boundary.is_some()
                && (1..=HITCH_POST_FRAMES).all(|offset| {
                    snapshot.events.iter().any(|event| {
                        event.name == "frame_start"
                            && event.frame_id == Some(hitch.frame_id + offset)
                    })
                });
            reports.push(HitchReport {
                frame_id: hitch.frame_id,
                pre_complete,
                post_complete,
                hitch_retained: hitch.sequence >= snapshot.first_sequence,
                snapshot,
            });
        }
        reports
    }

    /// Copies completed records and loss metadata; serialization never holds the ring lock.
    /// With `first_frame`, the copy starts at that frame's start marker when it is still retained.
    fn snapshot(
        &self,
        window: &TraceWindow,
        limit: usize,
        first_frame: Option<u64>,
    ) -> TraceSnapshot {
        let since_first_frame = first_frame.and_then(|frame_id| {
            window
                .events
                .iter()
                .rposition(|event| event.name == "frame_start" && event.frame_id == Some(frame_id))
        });
        let retained = since_first_frame
            .map_or(window.events.len(), |index| window.events.len() - index)
            .min(limit);
        TraceSnapshot {
            capacity: window.events.capacity().min(limit),
            first_sequence: window.next_sequence - retained as u64,
            overwritten: window.overwritten(),
            dropped_hitches: window.dropped_hitches,
            suppressed_reports: window.suppressed_reports,
            next_sequence: window.next_sequence,
            epoch_unix_nanos: self.epoch_unix_nanos,
            events: window
                .events
                .iter()
                .skip(window.events.len() - retained)
                .cloned()
                .collect(),
        }
    }

    /// Hands the recording to a worker once, without copying events or waiting for file output.
    pub fn flush(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let mut export = self
            .export
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.flushed.swap(true, Ordering::AcqRel) {
            return;
        }
        let snapshot = {
            let mut window = self
                .events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let capacity = window.events.capacity();
            let overwritten = window.overwritten();
            TraceSnapshot {
                capacity,
                first_sequence: overwritten,
                overwritten,
                dropped_hitches: window.dropped_hitches,
                suppressed_reports: window.suppressed_reports,
                next_sequence: window.next_sequence,
                epoch_unix_nanos: self.epoch_unix_nanos,
                events: std::mem::take(&mut window.events),
            }
        };
        let path = path.clone();
        match std::thread::Builder::new()
            .name("frame-trace-export".into())
            .spawn(move || {
                let result = std::fs::File::create(&path).and_then(|file| {
                    use std::io::Write;
                    let mut writer = std::io::BufWriter::new(file);
                    serde_json::to_writer(&mut writer, &snapshot).map_err(std::io::Error::other)?;
                    writer.flush()
                });
                if let Err(error) = result {
                    bevy::log::warn!("write stage frame trace {}: {error}", path.display());
                }
            }) {
            Ok(worker) => *export = Some(worker),
            Err(error) => bevy::log::warn!("start stage frame trace export: {error}"),
        }
    }

    /// Waits for an already requested export during teardown or before reading its file.
    pub fn wait_for_export(&self) {
        if let Some(worker) = self
            .export
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = worker.join();
        }
    }
}

impl Drop for FrameTrace {
    fn drop(&mut self) {
        self.flush();
        self.wait_for_export();
    }
}

/// An owned hitch window suitable for background formatting and queued output.
#[derive(Debug)]
pub struct HitchReport {
    frame_id: u64,
    pre_complete: bool,
    post_complete: bool,
    hitch_retained: bool,
    snapshot: TraceSnapshot,
}

impl std::fmt::Display for HitchReport {
    /// Serializes the captured window without touching live recording state.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut value = self.snapshot.json();
        value["hitch"] = json!({
            "frame_id": self.frame_id, "requested_pre_frames": HITCH_PRE_FRAMES,
            "requested_post_frames": HITCH_POST_FRAMES, "pre_complete": self.pre_complete,
            "post_complete": self.post_complete, "hitch_retained": self.hitch_retained,
            "complete": self.pre_complete && self.post_complete && self.hitch_retained,
            "scope": "completed_spans_and_received_counters", "post_boundary": "main_frame_start",
            "gpu_frame_identity": "unavailable", "presentation_feedback": "unavailable",
        });
        write!(f, "CINNABAR_HITCH {value}")
    }
}

#[derive(Debug)]
struct TraceSnapshot {
    capacity: usize,
    first_sequence: u64,
    overwritten: u64,
    dropped_hitches: u64,
    suppressed_reports: u64,
    next_sequence: u64,
    epoch_unix_nanos: u128,
    events: VecDeque<TraceEvent>,
}

impl TraceSnapshot {
    /// Materializes only the small, independently bounded hitch-report document.
    fn json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("trace values serialize")
    }
}

impl Serialize for TraceSnapshot {
    /// Streams metadata and the event sequence without building a document tree.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("capacity", &self.capacity)?;
        map.serialize_entry("truncated", &(self.first_sequence != 0))?;
        map.serialize_entry("dropped_events", &self.overwritten)?;
        map.serialize_entry("overwritten_events", &self.overwritten)?;
        map.serialize_entry("dropped_hitch_windows", &self.dropped_hitches)?;
        map.serialize_entry("suppressed_hitch_reports", &self.suppressed_reports)?;
        map.serialize_entry("first_event_sequence", &self.first_sequence)?;
        map.serialize_entry("omitted_events", &(self.first_sequence - self.overwritten))?;
        map.serialize_entry("next_event_sequence", &self.next_sequence)?;
        map.serialize_entry("retained_events", &self.events.len())?;
        map.serialize_entry("epoch_unix_nanos", &self.epoch_unix_nanos)?;
        map.serialize_entry("traceEvents", &TraceRecords(self))?;
        map.end()
    }
}

/// Borrows a snapshot while serializing one event at a time.
struct TraceRecords<'a>(&'a TraceSnapshot);

impl Serialize for TraceRecords<'_> {
    /// Keeps only one event's JSON representation alive as the writer consumes it.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.events.len()))?;
        let mut threads = Vec::new();
        for (index, event) in self.0.events.iter().enumerate() {
            let tid = match threads.iter().position(|id| *id == event.thread) {
                Some(index) => index,
                None => {
                    threads.push(event.thread);
                    threads.len() - 1
                }
            };
            let mut record = json!({"name": event.name, "ph": "X", "pid": std::process::id(),
                "tid": tid, "ts": event.started.as_secs_f64() * 1e6, "dur": event.elapsed.as_secs_f64() * 1e6,
                "frame_id": event.frame_id, "event_sequence": self.0.first_sequence + index as u64, "args": {}});
            let args = &mut record["args"];
            if let Some(frame_id) = event.frame_id {
                args["frame_id"] = json!(frame_id);
            }
            match &event.args {
                TraceArgs::None => {}
                TraceArgs::Focus {
                    focused,
                    occluded,
                    game_seconds,
                } => {
                    args["focused"] = json!(focused);
                    args["occluded"] = json!(occluded);
                    args["game_seconds"] = json!(game_seconds);
                }
                TraceArgs::SlowFrame(slow) => {
                    args["violations"] = json!(slow.reasons());
                    args["frame_ms"] = json!(slow.frame.as_secs_f64() * 1e3);
                    args["hard_hitch"] = json!(slow.hard_hitch);
                }
                TraceArgs::BackgroundSample { nanos } => {
                    args["duration_ns"] = json!(nanos);
                }
                TraceArgs::GpuSample { sequence, nanos } => {
                    args["sample_sequence"] = json!(sequence);
                    args["duration_ns"] = json!(nanos);
                }
            }
            match event.args {
                TraceArgs::GpuSample { .. } => {
                    record["ph"] = json!("C");
                    record["cat"] = json!("gpu_readback");
                    record["timestamp_kind"] = json!("readback_received");
                }
                TraceArgs::BackgroundSample { .. } => {
                    record["ph"] = json!("C");
                    record["cat"] = json!("background_work");
                    record["timestamp_kind"] = json!("sample_received");
                }
                TraceArgs::None => {}
                _ => {
                    record["ph"] = json!("i");
                    record["s"] = json!("t");
                }
            }
            sequence.serialize_element(&record)?;
        }
        sequence.end()
    }
}

/// Saturates diagnostic nanoseconds rather than truncating long durations.
fn nanos(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;

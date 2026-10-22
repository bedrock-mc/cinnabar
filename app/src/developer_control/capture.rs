//! Screenshots and recordings. A fixed-clock recording steps game time exactly 1/fps per
//! rendered frame and captures every frame, so the video is smooth however slow rendering is.
//! The shared readback observer never waits on the encoder, so screenshots answer however far
//! a recording falls behind: a real-time capture the encoder has no room for is dropped (the
//! video repeats the previous frame), and a fixed-clock one is parked until the next update,
//! which waits for room before the game advances.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use bevy::{prelude::*, render::render_resource::TextureFormat, time::TimeUpdateStrategy};
use developer_control::{
    clock::{FixedStepClock, RealTimePacer},
    protocol::RecordSettings,
    recorder::{Frame, FrameSink, PixelLayout, Recorder, Summary, VideoSettings},
    server::Reply,
    wav::{WavWriter, samples_for_frame},
};
use serde_json::{Value, json};

use client_audio::named_audio::{AudioDevice, CAPTURE_CHANNELS, CaptureMixer};

use crate::hud_tools::{FrameCapture, FrameCaptureSet};

/// How long a stopping recording waits for in-flight GPU readbacks.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

struct AudioCapture {
    mixer: Mutex<CaptureMixer>,
    wav: WavWriter,
    frames: u64,
}

#[derive(Resource)]
pub(super) struct Recording {
    recorder: Option<Recorder>,
    delivery: Delivery,
    fps: u32,
    fixed_clock: bool,
    pacer: RealTimePacer,
    started: Instant,
    next_index: u64,
    outstanding: Arc<AtomicU64>,
    audio: Option<AudioCapture>,
    stopping: Option<(Reply, Instant)>,
    /// A fixed clock lifted the frame-rate cadence, which stopping restores.
    lifted_cadence: bool,
    previous_max_delta: Duration,
}

impl Recording {
    fn new(
        recorder: Recorder,
        fps: u32,
        fixed_clock: bool,
        audio: Option<AudioCapture>,
        previous_max_delta: Duration,
    ) -> Self {
        Self {
            delivery: Delivery {
                sink: recorder.sink(),
                fixed_clock,
                parked: Arc::default(),
                dropped: Arc::default(),
                failure: Arc::default(),
            },
            recorder: Some(recorder),
            fps,
            fixed_clock,
            pacer: RealTimePacer::new(fps),
            started: Instant::now(),
            next_index: 0,
            outstanding: Arc::default(),
            audio,
            stopping: None,
            lifted_cadence: false,
            previous_max_delta,
        }
    }

    pub(super) fn summary(&self) -> Value {
        json!({
            "path": self.recorder.as_ref().map(|recorder| recorder.settings().path.clone()),
            "fps": self.fps,
            "fixed_clock": self.fixed_clock,
            "frames": self.next_index,
            "dropped_frames": self.delivery.dropped.load(Ordering::Relaxed),
            "audio": self.audio.is_some(),
            "stopping": self.stopping.is_some(),
        })
    }
}

/// Present for a fixed-clock recording: each update sets the next frame's exact time step.
#[derive(Resource)]
struct FixedClock(FixedStepClock);

pub(super) fn configure(app: &mut App) {
    app.add_systems(
        Last,
        (record_frame, step_fixed_clock)
            .chain()
            .before(FrameCaptureSet),
    );
}

fn step_fixed_clock(
    clock: Option<ResMut<FixedClock>>,
    mut strategy: ResMut<TimeUpdateStrategy>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    if let Some(mut clock) = clock {
        let step = clock.0.step();
        // Below 4 fps a frame outlasts the virtual clock's 250 ms clamp; `finish` restores it.
        if virtual_time.max_delta() < step {
            virtual_time.set_max_delta(step);
        }
        *strategy = TimeUpdateStrategy::ManualDuration(step);
    }
}

/// Saves the next rendered frame as a PNG, replying once it is on disk.
pub(super) fn screenshot(world: &mut World, path: PathBuf, reply: Reply) {
    world.resource_mut::<FrameCapture>().request(move |image| {
        let image = image.clone();
        std::thread::spawn(move || {
            let outcome =
                crate::hud_tools::write_png(image, &path).map(|_| json!({ "path": path }));
            reply.send(outcome);
        });
    });
}

pub(super) fn start(world: &mut World, settings: &RecordSettings) -> Result<Value, String> {
    if world.contains_resource::<Recording>() {
        return Err("a recording is already running; stop it first".into());
    }
    let clock = if settings.fixed_clock {
        Some(FixedStepClock::new(settings.fps).ok_or("fps must be positive")?)
    } else {
        None
    };
    let recorder = Recorder::start(VideoSettings {
        path: settings.path.clone(),
        fps: settings.fps,
        codec: settings.codec,
    })?;
    let audio = if settings.audio {
        let wav_path = settings.path.with_extension("wav");
        let wav = WavWriter::create(
            &wav_path,
            CAPTURE_CHANNELS,
            client_audio::audio::OUTPUT_RATE,
        )
        .map_err(|error| format!("{}: {error}", wav_path.display()))?;
        world
            .get_non_send_mut::<AudioDevice>()
            .map(|mut device| AudioCapture {
                mixer: Mutex::new(device.start_capture()),
                wav,
                frames: 0,
            })
    } else {
        None
    };
    let fixed_clock = clock.is_some();
    let lifted_cadence = fixed_clock
        && world
            .get_resource_mut::<crate::frame_pacing::FramePacingRuntime>()
            .map(|mut pacing| pacing.set_suspended(true))
            .is_some();
    let summary = json!({
        "path": settings.path,
        "fps": settings.fps,
        "fixed_clock": fixed_clock,
        "audio": audio.is_some(),
    });
    if let Some(clock) = clock {
        world.insert_resource(FixedClock(clock));
    }
    let previous_max_delta = world.resource::<Time<Virtual>>().max_delta();
    let mut recording = Recording::new(
        recorder,
        settings.fps,
        fixed_clock,
        audio,
        previous_max_delta,
    );
    recording.lifted_cadence = lifted_cadence;
    world.insert_resource(recording);
    Ok(summary)
}

pub(super) fn stop(world: &mut World, reply: Reply) {
    match world.get_resource_mut::<Recording>() {
        Some(mut recording) if recording.stopping.is_none() => {
            recording.stopping = Some((reply, Instant::now()));
        }
        Some(_) => reply.send(Err("the recording is already stopping".into())),
        None => reply.send(Err("no recording is running".into())),
    }
}

fn record_frame(world: &mut World) {
    let Some(mut recording) = world.remove_resource::<Recording>() else {
        return;
    };
    if let Some((_, since)) = &recording.stopping {
        let drained = recording.outstanding.load(Ordering::Acquire) == 0;
        if drained || since.elapsed() >= DRAIN_TIMEOUT {
            finish(world, recording);
        } else {
            world.insert_resource(recording);
        }
        return;
    }
    let copies = if recording.fixed_clock {
        // Fixed-clock backpressure: the game advances only once the encoder has room.
        recording.delivery.flush_parked();
        1
    } else {
        recording.pacer.frames_for(recording.started.elapsed())
    };
    if copies > 0 {
        capture_frame(world, &mut recording, copies);
        if let Some(audio) = recording.audio.as_mut() {
            pull_audio(audio, recording.fps, copies);
        }
    }
    world.insert_resource(recording);
}

fn capture_frame(world: &mut World, recording: &mut Recording, copies: u64) {
    let first = recording.next_index;
    recording.next_index += copies;
    let pending = InFlight::new(&recording.outstanding);
    let delivery = recording.delivery.clone();
    world.resource_mut::<FrameCapture>().request(move |image| {
        delivery.deliver(frame(image, first, copies));
        drop(pending);
    });
}

/// Hands readbacks to the encoder from the shared capture observer without ever waiting.
#[derive(Clone)]
struct Delivery {
    sink: FrameSink,
    fixed_clock: bool,
    /// Fixed-clock captures the encoder had no room for, flushed by the next update. Once
    /// anything is parked, later captures queue behind it so none overtakes a parked frame.
    parked: Arc<Mutex<VecDeque<Frame>>>,
    /// Real-time video slots dropped while the encoder was behind.
    dropped: Arc<AtomicU64>,
    failure: Arc<Mutex<Option<String>>>,
}

impl Delivery {
    fn deliver(&self, frame: Result<Frame, String>) {
        if self.fixed_clock {
            return self.deliver_in_order(frame);
        }
        match frame.and_then(|frame| self.sink.try_push(frame)) {
            Ok(None) => {}
            Ok(Some(frame)) => {
                if self.dropped.fetch_add(frame.copies, Ordering::Relaxed) == 0 {
                    warn!("developer recording: the encoder is behind; dropping real-time frames");
                }
            }
            Err(error) => self.fail(error),
        }
    }

    /// Queues a fixed-clock capture without waiting, behind any parked capture.
    fn deliver_in_order(&self, frame: Result<Frame, String>) {
        let Ok(mut parked) = self.parked.lock() else {
            return;
        };
        let pushed = frame.and_then(|frame| {
            if parked.is_empty() {
                self.sink.try_push(frame)
            } else {
                Ok(Some(frame))
            }
        });
        match pushed {
            Ok(None) => {}
            Ok(Some(frame)) => parked.push_back(frame),
            Err(error) => {
                drop(parked);
                self.fail(error);
            }
        }
    }

    /// Waits for the encoder to accept every parked capture, in capture order.
    fn flush_parked(&self) {
        loop {
            let Some(frame) = self
                .parked
                .lock()
                .ok()
                .and_then(|mut parked| parked.pop_front())
            else {
                return;
            };
            if let Err(error) = self.sink.push(frame) {
                self.fail(error);
            }
        }
    }

    fn fail(&self, error: String) {
        if let Ok(mut failure) = self.failure.lock() {
            failure.get_or_insert(error);
        }
    }
}

/// Counts a requested readback until it is served or dropped unserved.
struct InFlight(Arc<AtomicU64>);

impl InFlight {
    fn new(outstanding: &Arc<AtomicU64>) -> Self {
        outstanding.fetch_add(1, Ordering::AcqRel);
        Self(Arc::clone(outstanding))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One capture filling `copies` consecutive video slots; real-time pacing repeats a slow frame
/// without copying its pixels again.
fn frame(image: &Image, index: u64, copies: u64) -> Result<Frame, String> {
    let layout = match image.texture_descriptor.format {
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => PixelLayout::Bgra,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => PixelLayout::Rgba,
        other => return Err(format!("cannot record a {other:?} surface")),
    };
    let size = [image.width(), image.height()];
    let data = image.data.as_ref().ok_or("the capture has no pixel data")?;
    Ok(Frame {
        index,
        copies,
        size,
        layout,
        data: Arc::from(data.as_slice()),
    })
}

fn pull_audio(audio: &mut AudioCapture, fps: u32, copies: u64) {
    let Ok(mut mixer) = audio.mixer.lock() else {
        return;
    };
    let rate = client_audio::audio::OUTPUT_RATE;
    for _ in 0..copies {
        let count = samples_for_frame(rate, CAPTURE_CHANNELS, fps, audio.frames);
        audio.frames += 1;
        // An empty mixer yields nothing; silence keeps the track aligned with the video.
        let samples = (0..count).map(|_| mixer.next().unwrap_or(0.0));
        if let Err(error) = audio.wav.write(samples) {
            warn!("developer recording: audio write failed: {error}");
        }
    }
}

fn finish(world: &mut World, mut recording: Recording) {
    world.remove_resource::<FixedClock>();
    world.insert_resource(TimeUpdateStrategy::Automatic);
    world
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(recording.previous_max_delta);
    if std::mem::take(&mut recording.lifted_cadence)
        && let Some(mut pacing) =
            world.get_resource_mut::<crate::frame_pacing::FramePacingRuntime>()
    {
        pacing.set_suspended(false);
    }
    if recording.audio.is_some()
        && let Some(mut device) = world.get_non_send_mut::<AudioDevice>()
    {
        device.stop_capture();
    }
    let Some((reply, _)) = recording.stopping.take() else {
        return;
    };
    std::thread::spawn(move || {
        let mut recording = recording;
        reply.send(recording.finalize());
    });
}

impl Recording {
    /// Closes the stream, finishes the WAV and the MP4, and muxes them.
    fn finalize(&mut self) -> Result<Value, String> {
        self.delivery.flush_parked();
        let failure = self.delivery.failure.lock().ok().and_then(|mut f| f.take());
        let audio = self
            .audio
            .take()
            .map(|audio| {
                let path = audio.wav.path().to_owned();
                audio
                    .wav
                    .finish()
                    .map_err(|error| format!("finish audio {}: {error}", path.display()))
            })
            .transpose();
        let recorder = self.recorder.take();
        let scheduled = self.next_index;
        let (summary, wav) = complete_recording(failure, audio, |wav| {
            recorder
                .ok_or_else(|| "the recording had no encoder".to_owned())?
                .finish(scheduled, wav)
        })?;
        let dropped = self.delivery.dropped.load(Ordering::Relaxed);
        if dropped > 0 || summary.filled > 0 {
            warn!(
                dropped,
                filled = summary.filled,
                "developer recording: the encoder fell behind; missing frames repeat the previous one"
            );
        }
        Ok(json!({
            "path": summary.path,
            "frames": summary.frames,
            "size": summary.size,
            "fixed_clock": self.fixed_clock,
            "dropped_frames": dropped,
            "filled_frames": summary.filled,
            "audio": wav,
        }))
    }
}

/// Joins the encoder even after capture or audio failure and retains every completion error.
fn complete_recording(
    failure: Option<String>,
    audio: Result<Option<PathBuf>, String>,
    finish: impl FnOnce(Option<&Path>) -> Result<Summary, String>,
) -> Result<(Summary, Option<PathBuf>), String> {
    let mut errors: Vec<_> = failure.into_iter().collect();
    let wav = match audio {
        Ok(path) => path,
        Err(error) => {
            errors.push(error);
            None
        }
    };
    let result = finish(wav.as_deref());
    if let Err(error) = &result {
        errors.push(error.clone());
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    result.map(|summary| (summary, wav))
}

#[cfg(test)]
#[path = "capture/finalize_tests.rs"]
mod finalize_tests;

/// Quitting mid-recording still leaves a playable file instead of wedging shutdown.
impl Drop for Recording {
    fn drop(&mut self) {
        if self.recorder.is_some()
            && let Err(error) = self.finalize()
        {
            warn!("developer recording: finishing on shutdown failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::{
        prelude::*,
        render::{
            render_resource::TextureFormat,
            view::screenshot::{Screenshot, ScreenshotCaptured},
        },
        time::TimePlugin,
    };
    use developer_control::clock::FixedStepClock;

    use super::{FixedClock, step_fixed_clock};

    #[test]
    fn concurrent_captures_share_one_readback() {
        let mut app = App::new();
        app.add_plugins(TimePlugin);
        super::configure(&mut app);
        crate::hud_tools::configure_frame_capture(&mut app);
        let dir = std::env::temp_dir().join(format!("cinnabar-capture-{}", std::process::id()));
        let mut replies = Vec::new();
        for name in ["a.png", "b.png"] {
            let (reply, outcome) = developer_control::server::Reply::channel();
            super::screenshot(app.world_mut(), dir.join(name), reply);
            replies.push(outcome);
        }
        app.update();
        let mut shots = app.world_mut().query::<(Entity, &Screenshot)>();
        let entities: Vec<Entity> = shots.iter(app.world()).map(|(entity, _)| entity).collect();
        assert_eq!(entities.len(), 1, "one frame, one readback");
        let image = Image::new_fill(
            bevy::render::render_resource::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            &[255, 0, 0, 255],
            TextureFormat::Rgba8UnormSrgb,
            default(),
        );
        app.world_mut().trigger(ScreenshotCaptured {
            entity: entities[0],
            image,
        });
        for outcome in replies {
            let saved = outcome
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
            assert!(std::path::Path::new(saved["path"].as_str().unwrap()).is_file());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    fn red_image() -> Image {
        Image::new_fill(
            bevy::render::render_resource::Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            &[255, 0, 0, 255],
            TextureFormat::Rgba8UnormSrgb,
            default(),
        )
    }

    #[test]
    fn a_long_real_time_stall_keeps_one_image() {
        let frame = super::frame(&red_image(), 7, 100_000).unwrap();
        assert_eq!((frame.index, frame.copies), (7, 100_000));
        assert_eq!(
            frame.data.len(),
            2 * 2 * 4,
            "the repeated slots share one image"
        );
    }

    type Outcome = Result<serde_json::Value, String>;

    /// Captures `rounds` frames while the encoder is stalled, requesting a screenshot into
    /// `dir` with the last; yields the screenshot's outcome receiver once requested and, after
    /// the encoder is released, the recording's final reply and parked count. Runs on its own
    /// thread, so an observer blocked on the encoder fails the test's bounded waits instead of
    /// hanging it.
    fn screenshot_during_stalled_recording(
        fixed_clock: bool,
        rounds: u64,
        dir: std::path::PathBuf,
    ) -> (
        crossbeam_channel::Receiver<crossbeam_channel::Receiver<Outcome>>,
        crossbeam_channel::Receiver<(Outcome, usize)>,
    ) {
        let (shot_tx, shot_rx) = crossbeam_channel::bounded(1);
        let (done_tx, done_rx) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            let (release, gate) = crossbeam_channel::bounded::<()>(0);
            let recorder = developer_control::recorder::Recorder::with_encoder(
                developer_control::recorder::VideoSettings {
                    path: std::path::PathBuf::from("stalled.mp4"),
                    fps: 10,
                    codec: developer_control::recorder::Codec::H264,
                },
                move |settings, frames, _| {
                    let _ = gate.recv();
                    Ok(developer_control::recorder::Summary {
                        path: settings.path.clone(),
                        frames: frames.iter().map(|frame| frame.copies).sum(),
                        size: None,
                        filled: 0,
                    })
                },
            )
            .unwrap();
            let mut recording =
                super::Recording::new(recorder, 10, fixed_clock, None, Duration::ZERO);
            let mut app = App::new();
            crate::hud_tools::configure_frame_capture(&mut app);
            for round in 0..rounds {
                super::capture_frame(app.world_mut(), &mut recording, 1);
                if round + 1 == rounds {
                    let (reply, outcome) = developer_control::server::Reply::channel();
                    super::screenshot(app.world_mut(), dir.join("stalled.png"), reply);
                    let _ = shot_tx.send(outcome);
                }
                app.update();
                let mut shots = app.world_mut().query_filtered::<Entity, With<Screenshot>>();
                let entity = shots.single(app.world()).unwrap();
                app.world_mut().trigger(ScreenshotCaptured {
                    entity,
                    image: red_image(),
                });
                app.world_mut().entity_mut(entity).despawn();
            }
            let parked = recording.delivery.parked.lock().unwrap().len();
            drop(release);
            let reply = recording.finalize();
            let _ = done_tx.send((reply, parked));
        });
        (shot_rx, done_rx)
    }

    /// A per-test screenshot directory, so concurrent tests never remove each other's files.
    fn stalled_capture_dir(mode: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "cinnabar-stalled-capture-{mode}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn fixed_clock_captures_never_overtake_a_parked_frame() {
        let (release, gate) = crossbeam_channel::bounded::<()>(0);
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = std::sync::Arc::clone(&order);
        let recorder = developer_control::recorder::Recorder::with_encoder(
            developer_control::recorder::VideoSettings {
                path: std::path::PathBuf::from("ordered.mp4"),
                fps: 10,
                codec: developer_control::recorder::Codec::H264,
            },
            move |settings, frames, _| {
                let _ = gate.recv();
                for frame in frames {
                    received.lock().unwrap().push(frame.index);
                }
                Ok(developer_control::recorder::Summary {
                    path: settings.path.clone(),
                    frames: 0,
                    size: None,
                    filled: 0,
                })
            },
        )
        .unwrap();
        let mut recording = super::Recording::new(recorder, 10, true, None, Duration::ZERO);
        let capture = |index| super::frame(&red_image(), index, 1);
        // The stalled encoder's queue takes eight captures; the ninth is parked.
        for index in 0..=8 {
            recording.delivery.deliver(capture(index));
        }
        assert_eq!(recording.delivery.parked.lock().unwrap().len(), 1);
        // The encoder catches up before the next update flushes the parked capture.
        drop(release);
        test_time::eventually("the encoder drains its queue", || {
            order.lock().unwrap().len() == 8
        });
        for index in 9..=20 {
            recording.delivery.deliver(capture(index));
        }
        recording.next_index = 21;
        recording.finalize().unwrap();
        assert_eq!(*order.lock().unwrap(), (0..=20).collect::<Vec<_>>());
    }

    #[test]
    fn a_screenshot_answers_while_a_real_time_recording_is_stalled() {
        let rounds = 40;
        let dir = stalled_capture_dir("real-time");
        let (shot, done) = screenshot_during_stalled_recording(false, rounds, dir.clone());
        // The reply arrives once the PNG is on disk, so the directory is only removed after it.
        shot.recv_timeout(test_time::DEFAULT_TIMEOUT)
            .expect("capture blocked before the screenshot was requested")
            .recv_timeout(test_time::DEFAULT_TIMEOUT)
            .expect("the screenshot waited behind the stalled encoder")
            .unwrap();
        let (reply, parked) = done.recv_timeout(test_time::DEFAULT_TIMEOUT).unwrap();
        let _ = std::fs::remove_dir_all(dir);
        let reply = reply.unwrap();
        assert_eq!(parked, 0);
        let dropped = reply["dropped_frames"].as_u64().unwrap();
        assert!(dropped > 0, "{reply}");
        assert_eq!(reply["frames"].as_u64().unwrap() + dropped, rounds);
    }

    #[test]
    fn a_fixed_clock_recording_parks_frames_instead_of_blocking_the_observer() {
        let rounds = 40;
        let dir = stalled_capture_dir("fixed-clock");
        let (shot, done) = screenshot_during_stalled_recording(true, rounds, dir.clone());
        shot.recv_timeout(test_time::DEFAULT_TIMEOUT)
            .expect("capture blocked before the screenshot was requested")
            .recv_timeout(test_time::DEFAULT_TIMEOUT)
            .expect("the screenshot waited behind the stalled encoder")
            .unwrap();
        let (reply, parked) = done.recv_timeout(test_time::DEFAULT_TIMEOUT).unwrap();
        let _ = std::fs::remove_dir_all(dir);
        let reply = reply.unwrap();
        assert!(parked > 0, "the stalled encoder left no frame parked");
        assert_eq!(reply["dropped_frames"].as_u64(), Some(0));
        assert_eq!(
            reply["frames"].as_u64(),
            Some(rounds),
            "fixed clock keeps every frame"
        );
    }

    #[test]
    fn low_fixed_rates_are_not_clamped_by_the_virtual_clock() {
        let mut app = App::new();
        app.add_plugins(TimePlugin)
            .insert_resource(FixedClock(FixedStepClock::new(2).unwrap()))
            .add_systems(Last, step_fixed_clock);
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<Time>().delta(),
            Duration::from_millis(500)
        );
    }

    #[test]
    fn fixed_clock_advances_game_time_one_interval_per_update() {
        let mut app = App::new();
        app.add_plugins(TimePlugin)
            .insert_resource(FixedClock(FixedStepClock::new(60).unwrap()))
            .add_systems(Last, step_fixed_clock);
        app.update();
        let start = app.world().resource::<Time>().elapsed();
        let mut deltas = Vec::new();
        for _ in 0..60 {
            app.update();
            deltas.push(app.world().resource::<Time>().delta());
            assert_eq!(
                app.world().resource::<Time<Real>>().delta(),
                *deltas.last().unwrap()
            );
        }
        assert_eq!(
            app.world().resource::<Time>().elapsed() - start,
            Duration::from_secs(1)
        );
        assert!(
            deltas
                .iter()
                .all(|delta| delta.as_nanos().abs_diff(16_666_667) <= 1)
        );
    }
}

//! Screenshots and recordings. A fixed-clock recording steps game time exactly 1/fps per
//! rendered frame and captures every frame, so the video is smooth however slow rendering is.

use std::{
    path::PathBuf,
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
    recorder::{Frame, FrameSink, PixelLayout, Recorder, VideoSettings},
    server::Reply,
    wav::{WavWriter, samples_for_frame},
};
use serde_json::{Value, json};

use client_presentation::named_audio::{AudioDevice, CAPTURE_CHANNELS, CaptureMixer};

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
    sink: FrameSink,
    fps: u32,
    fixed_clock: bool,
    pacer: RealTimePacer,
    started: Instant,
    next_index: u64,
    outstanding: Arc<AtomicU64>,
    failure: Arc<Mutex<Option<String>>>,
    audio: Option<AudioCapture>,
    stopping: Option<(Reply, Instant)>,
    /// A fixed clock lifted the frame-rate cadence, which stopping restores.
    lifted_cadence: bool,
    previous_max_delta: Duration,
}

impl Recording {
    pub(super) fn summary(&self) -> Value {
        json!({
            "path": self.recorder.as_ref().map(|recorder| recorder.settings().path.clone()),
            "fps": self.fps,
            "fixed_clock": self.fixed_clock,
            "frames": self.next_index,
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
            client_presentation::audio::OUTPUT_RATE,
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
    world.insert_resource(Recording {
        sink: recorder.sink(),
        recorder: Some(recorder),
        fps: settings.fps,
        fixed_clock,
        pacer: RealTimePacer::new(settings.fps),
        started: Instant::now(),
        next_index: 0,
        outstanding: Arc::default(),
        failure: Arc::default(),
        audio,
        stopping: None,
        lifted_cadence,
        previous_max_delta: world.resource::<Time<Virtual>>().max_delta(),
    });
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
    let (sink, failure) = (recording.sink.clone(), Arc::clone(&recording.failure));
    world.resource_mut::<FrameCapture>().request(move |image| {
        let pushed = frames(image, first, copies)
            .and_then(|frames| frames.into_iter().try_for_each(|frame| sink.push(frame)));
        if let Err(error) = pushed
            && let Ok(mut failure) = failure.lock()
        {
            failure.get_or_insert(error);
        }
        drop(pending);
    });
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

/// `copies` consecutive frames from one capture; real-time pacing repeats a slow frame.
fn frames(image: &Image, first: u64, copies: u64) -> Result<Vec<Frame>, String> {
    let layout = match image.texture_descriptor.format {
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => PixelLayout::Bgra,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => PixelLayout::Rgba,
        other => return Err(format!("cannot record a {other:?} surface")),
    };
    let size = [image.width(), image.height()];
    let data = image.data.clone().ok_or("the capture has no pixel data")?;
    Ok((first..first + copies)
        .map(|index| Frame {
            index,
            size,
            layout,
            data: data.clone(),
        })
        .collect())
}

fn pull_audio(audio: &mut AudioCapture, fps: u32, copies: u64) {
    let Ok(mut mixer) = audio.mixer.lock() else {
        return;
    };
    let rate = client_presentation::audio::OUTPUT_RATE;
    for _ in 0..copies {
        let count = samples_for_frame(rate, CAPTURE_CHANNELS, fps, audio.frames);
        audio.frames += 1;
        // An empty mixer yields nothing; silence keeps the track aligned with the video.
        let samples = (0..count).map(|_| mixer.next().unwrap_or(0.0));
        if let Err(error) = audio.wav.write(samples) {
            eprintln!("developer recording: audio write failed: {error}");
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
        let failure = self.failure.lock().ok().and_then(|mut f| f.take());
        let audio_path = self.audio.take().map(|audio| audio.wav.finish());
        let recorder = self.recorder.take().ok_or("the recording had no encoder")?;
        if let Some(failure) = failure {
            return Err(failure);
        }
        let wav = audio_path.as_ref().and_then(|path| path.as_ref().ok());
        let summary = recorder.finish(wav.map(PathBuf::as_path))?;
        Ok(json!({
            "path": summary.path,
            "frames": summary.frames,
            "size": summary.size,
            "fixed_clock": self.fixed_clock,
            "audio": wav,
        }))
    }
}

/// Quitting mid-recording still leaves a playable file instead of wedging shutdown.
impl Drop for Recording {
    fn drop(&mut self) {
        if self.recorder.is_some()
            && let Err(error) = self.finalize()
        {
            eprintln!("developer recording: finishing on shutdown failed: {error}");
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

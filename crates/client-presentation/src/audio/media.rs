//! Timestamped PCM through the existing mixer; no I/O or allocation in Source::next.
//! The source resamples the 48 kHz stream to the device rate, scaled by the drift-correction
//! ratio, and publishes the PTS at its read head as the device clock.

use super::{
    engine::Listener,
    settings::{AudioCategory, AudioSettings},
    voice::{attenuation, pan_for},
};
use crossbeam_queue::ArrayQueue;
use rodio::Source;
use server_experience::media::{MAX_PCM_FRAMES, SAMPLE_RATE, clock::Correction, frames::PcmBlock};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

static MEDIA_VOICE: AtomicBool = AtomicBool::new(false);
const NO_POSITION: u64 = u64::MAX;

#[derive(Clone, Copy)]
struct StereoFrame {
    generation: u64,
    pts_us: u64,
    samples: [f32; 2],
}

pub struct MediaAudio {
    queue: ArrayQueue<StereoFrame>,
    generation: AtomicU64,
    cancelled: AtomicBool,
    paused: AtomicBool,
    gain: AtomicU32,
    pan: AtomicU32,
    spatial: AtomicBool,
    ratio: AtomicU64,         // f64 bits; drift-correction playback rate
    skip_until_us: AtomicU64, // drop queued audio before this PTS; 0 when idle
    hold_frames: AtomicU64,   // device frames of silence before resuming
    device_rate: u32,
    audible_us: AtomicU64,
    underruns: AtomicU64,
}

impl MediaAudio {
    /// Validates PCM on the producer; only fixed-size stereo frames enter the ring.
    pub fn push(&self, block: &PcmBlock) -> anyhow::Result<()> {
        let generation = self.generation.load(Ordering::Acquire);
        block.validate(generation)?;
        let channels = usize::from(block.channels);
        let frames = block.samples.len() / channels;
        anyhow::ensure!(self.has_room(frames), "media PCM ring full");
        for (index, samples) in block.samples.chunks_exact(channels).enumerate() {
            let frame = StereoFrame {
                generation,
                pts_us: block.pts_us + index as u64 * 1_000_000 / u64::from(SAMPLE_RATE),
                samples: [samples[0], samples[channels - 1]],
            };
            anyhow::ensure!(
                self.queue.push(frame).is_ok(),
                "media PCM producer conflict"
            );
        }
        Ok(())
    }

    /// Whether `frames` more stereo frames fit without blocking the producer.
    pub fn has_room(&self, frames: usize) -> bool {
        frames <= self.queue.capacity() - self.queue.len()
    }

    /// Starts a new decoder generation: queued audio and pending corrections are discarded.
    pub fn reset(&self, generation: u64) {
        self.generation.store(generation, Ordering::Release);
        while self.queue.pop().is_some() {}
        self.ratio.store(1.0f64.to_bits(), Ordering::Release);
        self.skip_until_us.store(0, Ordering::Release);
        self.hold_frames.store(0, Ordering::Release);
        self.audible_us.store(NO_POSITION, Ordering::Release);
    }

    /// Master/category sliders and the user's mute dominate the server's bounded gain.
    pub fn update(
        &self,
        settings: &AudioSettings,
        server_volume: u16,
        muted: bool,
        position: Option<[f32; 3]>,
        listener: Option<Listener>,
    ) {
        let (spatial, pan) = match (position, listener) {
            (Some(position), Some(listener)) if position.iter().all(|v| v.is_finite()) => {
                let delta = std::array::from_fn(|index| position[index] - listener.position[index]);
                let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
                (
                    attenuation(distance, 1.0, 32.0),
                    pan_for(delta, listener.right),
                )
            }
            (Some(_), _) => (0.0, 0.0),
            (None, _) => (1.0, 0.0),
        };
        let gain = if muted {
            0.0
        } else {
            settings.effective(AudioCategory::Records) * f32::from(server_volume.min(1000)) / 1000.0
                * spatial
        };
        self.spatial.store(position.is_some(), Ordering::Relaxed);
        self.gain.store(gain.to_bits(), Ordering::Relaxed);
        self.pan.store(pan.to_bits(), Ordering::Relaxed);
    }

    /// Pausing preserves queued samples; a seek replaces the generation and drains them.
    pub fn pause(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }

    /// Immediately ends playback on leave, transfer, disable or decoder failure.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// PTS at the mixer's read head; excludes the device's output latency, which Rodio hides.
    pub fn audible_position_us(&self) -> Option<u64> {
        let position = self.audible_us.load(Ordering::Acquire);
        (position != NO_POSITION).then_some(position)
    }

    /// Steers audio toward the media clock: small drift by rate, large drift by dropping
    /// late audio or holding early audio with silence. A pending jump is never stacked.
    pub fn correct(&self, correction: Correction, audible_us: u64) {
        match correction {
            Correction::Hold => self.ratio.store(1.0f64.to_bits(), Ordering::Release),
            Correction::Rate(rate) => self.ratio.store(rate.to_bits(), Ordering::Release),
            Correction::Seek(target) => {
                if self.skip_until_us.load(Ordering::Acquire) != 0
                    || self.hold_frames.load(Ordering::Acquire) != 0
                {
                    return;
                }
                self.ratio.store(1.0f64.to_bits(), Ordering::Release);
                if target > audible_us {
                    self.skip_until_us.store(target, Ordering::Release);
                } else {
                    let frames = (audible_us - target) * u64::from(self.device_rate) / 1_000_000;
                    self.hold_frames.store(frames.max(1), Ordering::Release);
                }
            }
        }
    }

    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }
}

struct MediaSource {
    control: Arc<MediaAudio>,
    device_rate: u32,
    right: Option<f32>,
    previous: StereoFrame,
    next: StereoFrame,
    phase: f64, // position between `previous` and `next`, in source frames
}

impl MediaSource {
    /// Pops the next current-generation frame, honouring a pending skip; None on underrun.
    fn pop(&mut self, generation: u64) -> Option<StereoFrame> {
        let skip = self.control.skip_until_us.load(Ordering::Acquire);
        loop {
            let frame = self.control.queue.pop()?;
            if frame.generation != generation {
                continue;
            }
            if skip != 0 {
                if frame.pts_us < skip {
                    continue;
                }
                self.control.skip_until_us.store(0, Ordering::Release);
            }
            return Some(frame);
        }
    }

    /// Advances the linear resampler by one device frame.
    fn frame(&mut self) -> [f32; 2] {
        let generation = self.control.generation.load(Ordering::Acquire);
        let ratio = f64::from_bits(self.control.ratio.load(Ordering::Relaxed)).clamp(0.99, 1.01);
        let step = f64::from(SAMPLE_RATE) / f64::from(self.device_rate) * ratio;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
            self.previous = self.next;
            match self.pop(generation) {
                Some(frame) => self.next = frame,
                None => {
                    self.control.underruns.fetch_add(1, Ordering::Relaxed);
                    self.next.samples = [0.0; 2];
                }
            }
        }
        let t = self.phase as f32;
        let mix = std::array::from_fn(|channel| {
            self.previous.samples[channel] * (1.0 - t) + self.next.samples[channel] * t
        });
        if self.previous.generation == generation {
            let offset = (self.phase * 1_000_000.0 / f64::from(SAMPLE_RATE)) as u64;
            self.control
                .audible_us
                .store(self.previous.pts_us + offset, Ordering::Release);
        }
        self.phase += step;
        mix
    }
}

impl Iterator for MediaSource {
    type Item = f32;

    /// Emits one interleaved sample, substituting silence on pause, hold or underrun.
    fn next(&mut self) -> Option<f32> {
        if self.control.cancelled.load(Ordering::Acquire) {
            return None;
        }
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        if self.control.paused.load(Ordering::Acquire)
            || self
                .control
                .hold_frames
                .try_update(Ordering::AcqRel, Ordering::Acquire, |left| {
                    left.checked_sub(1)
                })
                .is_ok()
        {
            self.right = Some(0.0);
            return Some(0.0);
        }
        let frame = self.frame();
        let gain = f32::from_bits(self.control.gain.load(Ordering::Relaxed));
        let pan = f32::from_bits(self.control.pan.load(Ordering::Relaxed)).clamp(-1.0, 1.0);
        let samples = if self.control.spatial.load(Ordering::Relaxed) {
            let mono = (frame[0] + frame[1]) * 0.5;
            [mono, mono]
        } else {
            frame
        };
        self.right = Some(samples[1] * gain * (1.0 + pan.min(0.0)));
        Some(samples[0] * gain * (1.0 - pan.max(0.0)))
    }
}

impl Source for MediaSource {
    /// One interleaved stereo stream at the device rate, so Rodio does no second conversion.
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    /// Output always has two channels; mono input is expanded before queuing.
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.device_rate
    }
    /// A cancelled source ends without a known whole-file duration.
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Drop for MediaSource {
    /// Releases the single-programme permit only after the actual mixer source retires.
    fn drop(&mut self) {
        MEDIA_VOICE.store(false, Ordering::Release);
    }
}

/// Builds the shared control and its source without touching a device.
fn source(generation: u64, device_rate: u32) -> (Arc<MediaAudio>, MediaSource) {
    let device_rate = device_rate.max(1);
    let control = Arc::new(MediaAudio {
        queue: ArrayQueue::new(MAX_PCM_FRAMES),
        generation: AtomicU64::new(generation),
        cancelled: AtomicBool::new(false),
        paused: AtomicBool::new(true),
        gain: AtomicU32::new(0.0f32.to_bits()),
        pan: AtomicU32::new(0.0f32.to_bits()),
        spatial: AtomicBool::new(false),
        ratio: AtomicU64::new(1.0f64.to_bits()),
        skip_until_us: AtomicU64::new(0),
        hold_frames: AtomicU64::new(0),
        device_rate,
        audible_us: AtomicU64::new(NO_POSITION),
        underruns: AtomicU64::new(0),
    });
    let silent = StereoFrame {
        generation: u64::MAX,
        pts_us: 0,
        samples: [0.0; 2],
    };
    let source = MediaSource {
        control: Arc::clone(&control),
        device_rate,
        right: None,
        previous: silent,
        next: silent,
        phase: 1.0,
    };
    (control, source)
}

/// Adds one paused, muted streaming source to Cinnabar's existing Rodio mixer.
pub fn start(
    device: &mut crate::named_audio::AudioDevice,
    generation: u64,
) -> Option<Arc<MediaAudio>> {
    if MEDIA_VOICE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return None;
    }
    let (control, source) = source(generation, device.sample_rate());
    device.play_source(source).then_some(control)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(generation: u64, pts_us: u64, frames: usize, value: f32) -> PcmBlock {
        PcmBlock {
            generation,
            pts_us,
            channels: 2,
            samples: vec![value; frames * 2],
        }
    }

    /// Unmutes and unpauses so samples reach the output.
    fn audible(control: &MediaAudio) {
        control.gain.store(1.0f32.to_bits(), Ordering::Relaxed);
        control.pause(false);
    }

    #[test]
    fn resampling_to_a_faster_device_stretches_time_and_tracks_the_read_head() {
        let (control, mut source) = source(1, 96_000);
        audible(&control);
        control.push(&block(1, 0, 960, 0.5)).unwrap();
        let samples: Vec<f32> = (&mut source).take(2 * 1900).collect();
        assert!(
            samples[4..]
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 1e-6)
        );
        let audible = control.audible_position_us().unwrap();
        assert!((19_500..=20_000).contains(&audible), "{audible}");
        assert_eq!(source.sample_rate(), 96_000);
    }

    #[test]
    fn rate_correction_consumes_audio_faster_or_slower() {
        let consumed = |rate: f64| {
            let (control, mut source) = source(1, 48_000);
            audible(&control);
            control.push(&block(1, 0, 4000, 0.1)).unwrap();
            control.correct(Correction::Rate(rate), 0);
            (&mut source).take(2 * 3000).for_each(drop);
            control.audible_position_us().unwrap()
        };
        assert!(consumed(1.005) > consumed(1.0));
        assert!(consumed(0.995) < consumed(1.0));
    }

    #[test]
    fn late_audio_is_dropped_and_early_audio_is_held_toward_the_media_clock() {
        let (control, mut source) = source(1, 48_000);
        audible(&control);
        control.push(&block(1, 0, 4800, 0.2)).unwrap();
        control.push(&block(1, 100_000, 4800, 0.4)).unwrap();
        control.correct(Correction::Seek(100_000), 0);
        let first: Vec<f32> = (&mut source).take(8).collect();
        assert!(first[4..].iter().all(|sample| (*sample - 0.4).abs() < 1e-6));
        let before = control.audible_position_us().unwrap();
        control.correct(Correction::Seek(before - 50_000), before);
        let held: Vec<f32> = (&mut source).take(2 * 2400).collect();
        assert!(held.iter().all(|sample| *sample == 0.0));
        assert_eq!(control.audible_position_us(), Some(before));
    }

    #[test]
    fn early_audio_holds_for_the_requested_time_at_any_device_rate() {
        let (control, mut source) = source(1, 96_000);
        audible(&control);
        control.push(&block(1, 0, 9600, 0.2)).unwrap();
        (&mut source).take(2 * 9600).for_each(drop);
        let before = control.audible_position_us().unwrap();
        control.correct(Correction::Seek(before - 50_000), before);
        let held: Vec<f32> = (&mut source).take(2 * 4800).collect();
        assert!(held.iter().all(|sample| *sample == 0.0), "50 ms at 96 kHz");
        assert_eq!(control.audible_position_us(), Some(before));
        let resumed: Vec<f32> = (&mut source).take(4).collect();
        assert!(resumed.iter().any(|sample| *sample != 0.0));
    }

    #[test]
    fn a_new_generation_discards_stale_audio_and_corrections() {
        let (control, mut source) = source(1, 48_000);
        audible(&control);
        control.push(&block(1, 0, 100, 0.3)).unwrap();
        control.correct(Correction::Seek(10), 500_000);
        control.reset(2);
        assert!(control.push(&block(1, 0, 10, 0.3)).is_err());
        control.push(&block(2, 7_000_000, 100, 0.6)).unwrap();
        let samples: Vec<f32> = (&mut source).take(8).collect();
        assert!(
            samples[4..]
                .iter()
                .all(|sample| (*sample - 0.6).abs() < 1e-6)
        );
        assert!(control.audible_position_us().unwrap() >= 7_000_000);
    }
}

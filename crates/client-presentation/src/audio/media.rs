//! Timestamped PCM through the existing mixer; no I/O or allocation in Source::next.

use super::{
    engine::Listener,
    settings::{AudioCategory, AudioSettings},
    voice::{attenuation, pan_for},
};
use crossbeam_queue::ArrayQueue;
use rodio::Source;
use server_experience::media::{MAX_PCM_FRAMES, SAMPLE_RATE, frames::PcmBlock};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

static MEDIA_VOICE: AtomicBool = AtomicBool::new(false);

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
    submitted_us: AtomicU64,
    underruns: AtomicU64,
}

impl MediaAudio {
    /// Validates PCM on the producer; only fixed-size stereo frames enter the ring.
    pub fn push(&self, block: &PcmBlock) -> anyhow::Result<()> {
        let generation = self.generation.load(Ordering::Acquire);
        block.validate(generation)?;
        let channels = usize::from(block.channels);
        let frames = block.samples.len() / channels;
        anyhow::ensure!(
            frames <= self.queue.capacity() - self.queue.len(),
            "media PCM ring full"
        );
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

    /// A submission diagnostic only; Rodio does not expose the audible device timestamp.
    pub fn submitted_position_us(&self) -> u64 {
        self.submitted_us.load(Ordering::Acquire)
    }

    /// Production synchronization must provide device latency before using audio as master.
    pub fn audible_position_us(&self) -> Option<u64> {
        None
    }
}

struct MediaSource {
    control: Arc<MediaAudio>,
    right: Option<f32>,
}

impl Iterator for MediaSource {
    type Item = f32;

    /// Pops one bounded sample, substituting silence on underrun without blocking.
    fn next(&mut self) -> Option<f32> {
        if self.control.cancelled.load(Ordering::Acquire) {
            return None;
        }
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        if self.control.paused.load(Ordering::Acquire) {
            self.right = Some(0.0);
            return Some(0.0);
        }
        let generation = self.control.generation.load(Ordering::Acquire);
        let Some(frame) = self
            .control
            .queue
            .pop()
            .filter(|frame| frame.generation == generation)
        else {
            self.control.underruns.fetch_add(1, Ordering::Relaxed);
            self.right = Some(0.0);
            return Some(0.0);
        };
        self.control
            .submitted_us
            .store(frame.pts_us, Ordering::Release);
        let gain = f32::from_bits(self.control.gain.load(Ordering::Relaxed));
        let pan = f32::from_bits(self.control.pan.load(Ordering::Relaxed)).clamp(-1.0, 1.0);
        let samples = if self.control.spatial.load(Ordering::Relaxed) {
            let mono = (frame.samples[0] + frame.samples[1]) * 0.5;
            [mono, mono]
        } else {
            frame.samples
        };
        self.right = Some(samples[1] * gain * (1.0 + pan.min(0.0)));
        Some(samples[0] * gain * (1.0 - pan.max(0.0)))
    }
}

impl Source for MediaSource {
    /// One interleaved stereo stream at the host's fixed media rate.
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    /// Output always has two channels; mono input is expanded before queuing.
    fn channels(&self) -> u16 {
        2
    }
    /// Rodio performs any final conversion to the output device rate.
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
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
    let control = Arc::new(MediaAudio {
        queue: ArrayQueue::new(MAX_PCM_FRAMES),
        generation: AtomicU64::new(generation),
        cancelled: AtomicBool::new(false),
        paused: AtomicBool::new(true),
        gain: AtomicU32::new(0.0f32.to_bits()),
        pan: AtomicU32::new(0.0f32.to_bits()),
        spatial: AtomicBool::new(false),
        submitted_us: AtomicU64::new(0),
        underruns: AtomicU64::new(0),
    });
    device
        .play_source(MediaSource {
            control: Arc::clone(&control),
            right: None,
        })
        .then_some(control)
}

//! One playing sound: a resampling stereo source driven by atomics the engine updates per frame.

use rodio::Source;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

/// Fixed voice output rate; rodio's mixer converts to the device rate.
pub(super) const OUTPUT_RATE: u32 = 48_000;
const GAIN_SLEW: f32 = 0.002;

/// Decoded PCM16 shared between the cache and every voice playing it.
#[derive(Debug)]
pub(crate) struct Pcm {
    pub channels: u8,
    pub rate: u32,
    pub samples: Arc<[i16]>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }
}

/// Engine-to-mixer control block for one voice.
#[derive(Debug)]
pub(crate) struct VoiceShared {
    cancel: AtomicBool,
    done: AtomicBool,
    gain: AtomicU32,
    pan: AtomicU32,
    pitch: AtomicU32,
}

impl VoiceShared {
    pub fn new(gain: f32, pan: f32, pitch: f32) -> Arc<Self> {
        Arc::new(Self {
            cancel: AtomicBool::new(false),
            done: AtomicBool::new(false),
            gain: AtomicU32::new(finite_or(gain, 0.0).to_bits()),
            pan: AtomicU32::new(finite_or(pan, 0.0).to_bits()),
            pitch: AtomicU32::new(finite_or(pitch, 1.0).to_bits()),
        })
    }

    pub fn set(&self, gain: f32, pan: f32) {
        self.gain
            .store(finite_or(gain, 0.0).to_bits(), Ordering::Relaxed);
        self.pan
            .store(finite_or(pan, 0.0).to_bits(), Ordering::Relaxed);
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub fn finished(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
}

/// Keeps malformed controls out of the mixer state.
fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

fn load(cell: &AtomicU32) -> f32 {
    f32::from_bits(cell.load(Ordering::Relaxed))
}

pub(crate) struct VoiceSource {
    pcm: Arc<Pcm>,
    shared: Arc<VoiceShared>,
    position: f64,
    looping: bool,
    gain: f32,
    pending_right: Option<f32>,
}

impl VoiceSource {
    pub fn new(pcm: Arc<Pcm>, shared: Arc<VoiceShared>, looping: bool) -> Self {
        let gain = load(&shared.gain);
        Self {
            pcm,
            shared,
            position: 0.0,
            looping,
            gain,
            pending_right: None,
        }
    }

    fn sample(&self, frame: usize, channel: usize) -> f32 {
        let channels = usize::from(self.pcm.channels);
        let frames = self.pcm.frames();
        let frame = if frame >= frames {
            frames.saturating_sub(1)
        } else {
            frame
        };
        f32::from(self.pcm.samples[frame * channels + channel.min(channels - 1)]) / 32768.0
    }

    fn finish(&self) {
        self.shared.done.store(true, Ordering::Release);
    }
}

impl Drop for VoiceSource {
    fn drop(&mut self) {
        self.finish();
    }
}

impl Iterator for VoiceSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.pending_right.take() {
            return Some(right);
        }
        if self.shared.cancel.load(Ordering::Acquire) {
            self.finish();
            return None;
        }
        let frames = self.pcm.frames();
        if frames == 0 {
            self.finish();
            return None;
        }
        let mut index = self.position.floor();
        if index >= frames as f64 {
            if !self.looping {
                self.finish();
                return None;
            }
            self.position %= frames as f64;
            index = self.position.floor();
        }
        self.gain += (load(&self.shared.gain) - self.gain) * GAIN_SLEW;
        let base = index as usize;
        let fraction = (self.position - index) as f32;
        let next = if base + 1 >= frames && self.looping {
            0
        } else {
            base + 1
        };
        let mix = |channel: usize| {
            let a = self.sample(base, channel);
            a + (self.sample(next, channel) - a) * fraction
        };
        let pan = load(&self.shared.pan).clamp(-1.0, 1.0);
        let (left, right) = if self.pcm.channels >= 2 {
            (mix(0), mix(1))
        } else {
            let mono = mix(0);
            (mono * (1.0 - pan.max(0.0)), mono * (1.0 + pan.min(0.0)))
        };
        let step = f64::from(self.pcm.rate) * f64::from(load(&self.shared.pitch).max(0.01))
            / f64::from(OUTPUT_RATE);
        self.position += step;
        self.pending_right = Some(right * self.gain);
        Some(left * self.gain)
    }
}

impl Source for VoiceSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        2
    }

    fn sample_rate(&self) -> u32 {
        OUTPUT_RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Linear falloff from full volume at `min` to silence at `max` blocks.
pub(super) fn attenuation(distance: f32, min: f32, max: f32) -> f32 {
    if !distance.is_finite() {
        return 0.0;
    }
    if distance <= min {
        return 1.0;
    }
    if max <= min {
        return 0.0;
    }
    ((max - distance) / (max - min)).clamp(0.0, 1.0)
}

/// Stereo pan in `[-1, 1]` for a source at `delta` from the listener, `right` the listener's right axis.
pub(super) fn pan_for(delta: [f32; 3], right: [f32; 3]) -> f32 {
    let distance = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
    if distance < 1.0e-3 {
        return 0.0;
    }
    let across = (delta[0] * right[0] + delta[1] * right[1] + delta[2] * right[2]) / distance;
    across * (distance / 2.0).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(samples: Vec<i16>, channels: u8, rate: u32) -> Arc<Pcm> {
        Arc::new(Pcm {
            channels,
            rate,
            samples: samples.into(),
        })
    }

    #[test]
    fn review_non_finite_controls_cannot_poison_a_voice() {
        let shared = VoiceShared::new(1.0, 0.0, f32::INFINITY);
        let mut source = VoiceSource::new(pcm(vec![1000; 4], 1, OUTPUT_RATE), shared.clone(), true);
        shared.set(f32::NAN, f32::NAN);
        assert!(source.by_ref().take(32).all(|sample| sample.is_finite()));
        shared.set(1.0, 0.0);
        assert!(source.take(32).all(|sample| sample.is_finite()));
    }

    #[test]
    fn attenuation_is_linear_between_bounds() {
        assert_eq!(attenuation(0.0, 0.0, 16.0), 1.0);
        assert!((attenuation(8.0, 0.0, 16.0) - 0.5).abs() < 1e-6);
        assert_eq!(attenuation(16.0, 0.0, 16.0), 0.0);
        assert_eq!(attenuation(40.0, 0.0, 16.0), 0.0);
        assert_eq!(attenuation(90.0, 100.0, 200.0), 1.0);
        assert_eq!(attenuation(f32::NAN, 0.0, 16.0), 0.0);
    }

    #[test]
    fn pan_follows_the_listener_right_axis() {
        assert!(pan_for([4.0, 0.0, 0.0], [1.0, 0.0, 0.0]) > 0.9);
        assert!(pan_for([-4.0, 0.0, 0.0], [1.0, 0.0, 0.0]) < -0.9);
        assert_eq!(pan_for([0.0; 3], [1.0, 0.0, 0.0]), 0.0);
        assert!(pan_for([0.2, 0.0, 0.0], [1.0, 0.0, 0.0]) < 0.2);
    }

    #[test]
    fn one_shot_ends_and_reports_done() {
        let shared = VoiceShared::new(1.0, 0.0, 1.0);
        // 48 kHz source at unit pitch advances one frame per output frame.
        let mut source =
            VoiceSource::new(pcm(vec![16384; 4], 1, OUTPUT_RATE), shared.clone(), false);
        let out: Vec<f32> = source.by_ref().collect();
        assert_eq!(out.len(), 8);
        assert!(shared.finished());
        assert!(out.iter().all(|value| *value > 0.0));
    }

    #[test]
    fn cancel_stops_a_loop_and_pan_shifts_energy() {
        let shared = VoiceShared::new(1.0, 1.0, 1.0);
        let mut source =
            VoiceSource::new(pcm(vec![16384; 4], 1, OUTPUT_RATE), shared.clone(), true);
        let (left, right) = (source.next().unwrap(), source.next().unwrap());
        assert!(right > left);
        for _ in 0..40 {
            source.next();
        }
        shared.cancel();
        assert!(source.next().is_none() || source.next().is_none());
        assert!(shared.finished());
    }

    #[test]
    fn pitch_two_consumes_twice_as_fast() {
        let shared = VoiceShared::new(1.0, 0.0, 2.0);
        let source = VoiceSource::new(pcm(vec![1000; 8], 1, OUTPUT_RATE), shared, false);
        assert_eq!(source.count(), 8);
    }
}

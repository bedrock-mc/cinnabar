//! Source-owned retirement. Cancellation NEVER releases a submitted reservation.
use assets::RuntimeAudioPcm;
use rodio::{OutputStream, OutputStreamHandle, Source};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
mod priority;

pub(super) const VOICE_LIMIT: usize = 16;
/// Captured audio is interleaved stereo.
pub const CAPTURE_CHANNELS: u16 = 2;
/// Pull-driven mix of everything played while capturing.
pub type CaptureMixer = rodio::dynamic_mixer::DynamicMixer<f32>;

pub(super) struct PermitPool {
    slots: [AtomicU64; VOICE_LIMIT],
}
impl Default for PermitPool {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}
impl PermitPool {
    pub fn acquire(self: &Arc<Self>) -> Option<Permit> {
        for (slot, state) in self.slots.iter().enumerate() {
            let old = state.load(Ordering::Acquire);
            if old & 1 == 0
                && old.checked_add(2).is_some()
                && state
                    .compare_exchange(old, old + 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                return Some(Permit {
                    pool: Arc::clone(self),
                    slot,
                    token: old + 1,
                });
            }
        }
        None
    }
    pub fn retired(&self, slot: usize, token: u64) -> bool {
        self.slots[slot].load(Ordering::Acquire) != token
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn occupied(&self) -> usize {
        self.slots
            .iter()
            .filter(|value| value.load(Ordering::Acquire) & 1 != 0)
            .count()
    }
}
pub(super) struct Permit {
    pool: Arc<PermitPool>,
    slot: usize,
    token: u64,
}
impl Drop for Permit {
    fn drop(&mut self) {
        let _ = self.pool.slots[self.slot].compare_exchange(
            self.token,
            self.token + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}
pub(super) struct VoiceControl {
    pub cancel: Arc<AtomicBool>,
    pub slot: usize,
    pub token: u64,
}
impl VoiceControl {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}
/// No Clone: the lease belongs to exactly one actual backend source.
pub(super) struct CancelablePcm {
    samples: Arc<[i16]>,
    channels: u16,
    rate: u32,
    frames: u32,
    offset: usize,
    cancel: Arc<AtomicBool>,
    _permit: Permit,
}
impl CancelablePcm {
    pub fn prepare(
        sample: &RuntimeAudioPcm,
        pool: &Arc<PermitPool>,
    ) -> Option<(Self, VoiceControl)> {
        let permit = pool.acquire()?;
        let cancel = Arc::new(AtomicBool::new(false));
        let control = VoiceControl {
            cancel: Arc::clone(&cancel),
            slot: permit.slot,
            token: permit.token,
        };
        Some((
            Self {
                samples: sample.shared_samples(),
                channels: u16::from(sample.channels()),
                rate: sample.sample_rate(),
                frames: sample.frames(),
                offset: 0,
                cancel,
                _permit: permit,
            },
            control,
        ))
    }
}
impl Iterator for CancelablePcm {
    type Item = i16;
    fn next(&mut self) -> Option<i16> {
        if self.cancel.load(Ordering::Acquire) {
            return None;
        }
        let value = *self.samples.get(self.offset)?;
        self.offset += 1;
        Some(value)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.samples.len() - self.offset))
    }
}
impl Source for CancelablePcm {
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.samples.len() - self.offset)
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(
            f64::from(self.frames) / f64::from(self.rate),
        ))
    }
}

const MIXER_RATE: u32 = 48_000;

/// Main-thread owner. No automatic reopen, alternate device enumeration or Sink.
pub struct AudioDevice {
    output: Option<(OutputStream, OutputStreamHandle)>,
    sample_rate: u32,
    /// While set, new sources mix into a recording instead of the device.
    capture: Option<Arc<rodio::dynamic_mixer::DynamicMixerController<f32>>>,
    #[cfg(any(test, feature = "test-support"))]
    test_output: Option<Arc<rodio::dynamic_mixer::DynamicMixerController<f32>>>,
}
impl AudioDevice {
    pub fn disabled() -> Self {
        Self {
            output: None,
            sample_rate: MIXER_RATE,
            capture: None,
            #[cfg(any(test, feature = "test-support"))]
            test_output: None,
        }
    }
    pub fn open_default_once() -> Self {
        use rodio::cpal::traits::{DeviceTrait, HostTrait};
        let Some(device) = rodio::cpal::default_host().default_output_device() else {
            eprintln!("named audio disabled: no default output device");
            return Self::disabled();
        };
        // Rodio may negotiate formats on this same device, never another device.
        // Rodio opens the device at its default config first; match that rate.
        let sample_rate = device
            .default_output_config()
            .map_or(MIXER_RATE, |config| config.sample_rate().0);
        match OutputStream::try_from_device(&device) {
            Ok(output) => Self {
                output: Some(output),
                sample_rate,
                capture: None,
                #[cfg(any(test, feature = "test-support"))]
                test_output: None,
            },
            Err(error) => {
                eprintln!("named audio disabled: default output initialization failed: {error}");
                Self::disabled()
            }
        }
    }
    /// Output rate that streaming sources should produce to avoid a second conversion.
    pub fn sample_rate(&self) -> u32 {
        if self.capture.is_some() {
            return crate::audio::OUTPUT_RATE;
        }
        self.sample_rate
    }
    pub fn available(&self) -> bool {
        if self.capture.is_some() {
            return true;
        }
        #[cfg(any(test, feature = "test-support"))]
        if self.test_output.is_some() {
            return true;
        }
        self.output.is_some()
    }
    /// Only replaces hardware transport; admission and source ownership are real.
    #[cfg(any(test, feature = "test-support"))]
    pub fn memory_mixer() -> (Self, rodio::dynamic_mixer::DynamicMixer<f32>) {
        let (controller, mixer) = rodio::dynamic_mixer::mixer::<f32>(2, MIXER_RATE);
        (
            Self {
                output: None,
                sample_rate: MIXER_RATE,
                capture: None,
                test_output: Some(controller),
            },
            mixer,
        )
    }
    /// Diverts every later source into the returned stereo mixer at
    /// [`crate::audio::OUTPUT_RATE`], which the caller pulls at its own pace, until
    /// [`Self::stop_capture`]; sources already playing stay on the device.
    pub fn start_capture(&mut self) -> CaptureMixer {
        let (controller, mixer) =
            rodio::dynamic_mixer::mixer::<f32>(CAPTURE_CHANNELS, crate::audio::OUTPUT_RATE);
        self.capture = Some(controller);
        mixer
    }
    pub fn stop_capture(&mut self) {
        self.capture = None;
    }
    /// Plays an already-stereo f32 source; false when the device is unavailable or rejects it.
    pub fn play_source(&mut self, source: impl Source<Item = f32> + Send + 'static) -> bool {
        if let Some(controller) = &self.capture {
            controller.add(source);
            return true;
        }
        #[cfg(any(test, feature = "test-support"))]
        if let Some(controller) = &self.test_output {
            controller.add(source);
            return true;
        }
        let Some((_, handle)) = &self.output else {
            return false;
        };
        if handle
            .play_raw(priority::CallbackSource::new(source))
            .is_err()
        {
            self.output = None;
            return false;
        }
        true
    }
    pub(super) fn submit(&mut self, source: CancelablePcm) -> bool {
        if let Some(controller) = &self.capture {
            controller.add(source.convert_samples::<f32>());
            return true;
        }
        #[cfg(any(test, feature = "test-support"))]
        if let Some(controller) = &self.test_output {
            controller.add(source.convert_samples::<f32>());
            return true;
        }
        let Some((_, handle)) = &self.output else {
            return false;
        };
        if handle
            .play_raw(priority::CallbackSource::new(
                source.convert_samples::<f32>(),
            ))
            .is_err()
        {
            // The rejected owned source drops normally. No permit is manually freed.
            self.output = None;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests;

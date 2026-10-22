//! Source-owned retirement. Cancellation NEVER releases a submitted reservation.
use assets::RuntimeAudioPcm;
#[cfg(not(target_arch = "wasm32"))]
use native::{Opening, Output, open_default_output, open_on_owner_thread};
use rodio::Source;
#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
pub use browser::BrowserAudio;
#[cfg(target_arch = "wasm32")]
use browser::Output;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::TryRecvError;
#[cfg(all(test, not(target_arch = "wasm32")))]
use std::sync::mpsc::{self, Sender};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
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

const MIXER_RATE: u32 = crate::audio::OUTPUT_RATE;

/// Main-thread access to the default output, capture or an in-memory test mixer.
pub struct AudioDevice {
    /// The default output while its owner thread is still opening it.
    #[cfg(not(target_arch = "wasm32"))]
    opening: Option<Opening>,
    output: Option<Output>,
    sample_rate: u32,
    /// While set, new sources mix into a recording instead of the device.
    capture: Option<Arc<rodio::dynamic_mixer::DynamicMixerController<f32>>>,
    #[cfg(any(test, feature = "test-support"))]
    test_output: Option<Arc<rodio::dynamic_mixer::DynamicMixerController<f32>>>,
}
impl AudioDevice {
    pub fn disabled() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            opening: None,
            output: None,
            sample_rate: MIXER_RATE,
            capture: None,
            #[cfg(any(test, feature = "test-support"))]
            test_output: None,
        }
    }
    /// Starts opening the default output on its own thread; later uses poll without waiting.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_default_once() -> Self {
        match open_on_owner_thread(open_default_output, native::refresh_output) {
            Ok((opening, _owner)) => Self {
                opening: Some(opening),
                ..Self::disabled()
            },
            Err(error) => {
                bevy::log::warn!(
                    "named audio disabled: audio output thread failed to start: {error}"
                );
                Self::disabled()
            }
        }
    }
    /// Opens Web Audio; a later user gesture resumes its retained context.
    #[cfg(target_arch = "wasm32")]
    pub fn open_default_once() -> Self {
        let Some((output, sample_rate)) = Output::open() else {
            return Self::disabled();
        };
        Self {
            output: Some(output),
            sample_rate,
            ..Self::disabled()
        }
    }
    /// Gives the page gesture and teardown control over the actual mixer stream.
    #[cfg(target_arch = "wasm32")]
    pub fn browser_audio(&self) -> Option<BrowserAudio> {
        self.output.as_ref().and_then(Output::control)
    }
    /// Takes the shared mixer once its owner thread finishes the initial output attempt.
    fn resolve(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(opening) = self.opening.take() else {
                return;
            };
            match opening.try_recv() {
                Ok(Some((mixer, release))) => {
                    self.sample_rate = MIXER_RATE;
                    self.output = Some(Output {
                        mixer,
                        _release: release,
                    });
                }
                Err(TryRecvError::Empty) => self.opening = Some(opening),
                Ok(None) | Err(TryRecvError::Disconnected) => {}
            }
        }
    }
    /// Whether requests must wait for the device; capture and test mixers are ready immediately.
    pub fn is_opening(&mut self) -> bool {
        if self.available() {
            return false;
        }
        #[cfg(not(target_arch = "wasm32"))]
        return self.opening.is_some();
        #[cfg(target_arch = "wasm32")]
        false
    }

    /// Creates a device whose opening fails only after the test releases its channel gate.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn delayed_open() -> (Self, Sender<()>, std::thread::JoinHandle<()>) {
        let (release, wait) = mpsc::channel();
        let (opening, owner) = open_on_owner_thread(
            move || {
                let _ = wait.recv();
                None::<((), native::SharedOutput)>
            },
            |_| {},
        )
        .expect("start test output owner");
        (
            Self {
                opening: Some(opening),
                ..Self::disabled()
            },
            release,
            owner,
        )
    }

    /// Mixer rate that streaming sources should produce before hardware-format conversion.
    pub fn sample_rate(&mut self) -> u32 {
        if self.capture.is_some() {
            return crate::audio::OUTPUT_RATE;
        }
        self.resolve();
        self.sample_rate
    }
    pub fn available(&mut self) -> bool {
        if self.capture.is_some() {
            return true;
        }
        #[cfg(any(test, feature = "test-support"))]
        if self.test_output.is_some() {
            return true;
        }
        self.resolve();
        // Native availability belongs to the retained mixer, including hardware handoffs.
        #[cfg(not(target_arch = "wasm32"))]
        return self.output.is_some();
        #[cfg(target_arch = "wasm32")]
        self.output.as_ref().is_some_and(Output::available)
    }
    /// Only replaces hardware transport; admission and source ownership are real.
    #[cfg(any(test, feature = "test-support"))]
    pub fn memory_mixer() -> (Self, rodio::dynamic_mixer::DynamicMixer<f32>) {
        let (controller, mixer) = rodio::dynamic_mixer::mixer::<f32>(2, MIXER_RATE);
        (
            Self {
                test_output: Some(controller),
                ..Self::disabled()
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
        self.resolve();
        let Some(output) = &self.output else {
            return false;
        };
        if output.play_raw(source).is_err() {
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
        self.resolve();
        let Some(output) = &self.output else {
            return false;
        };
        if output.play_raw(source.convert_samples::<f32>()).is_err() {
            // The rejected owned source drops normally. No permit is manually freed.
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests;

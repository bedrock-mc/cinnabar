//! Source-owned retirement. Cancellation NEVER releases a submitted reservation.
use assets::RuntimeAudioPcm;
use rodio::{OutputStream, OutputStreamHandle, Source};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
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

/// The default output, opened on its own thread; dropping the release sender lets that thread
/// drop the stream it opened.
struct Output {
    handle: OutputStreamHandle,
    _release: Sender<()>,
}

/// Receives the shared output value and the sender whose drop releases its owner's stream.
type Opening<H = (OutputStreamHandle, u32)> = Receiver<Option<(H, Sender<()>)>>;

/// Main-thread owner. No automatic reopen, alternate device enumeration or Sink.
pub struct AudioDevice {
    /// The default output while its owner thread is still opening it; taken on first use.
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
            opening: None,
            output: None,
            sample_rate: MIXER_RATE,
            capture: None,
            #[cfg(any(test, feature = "test-support"))]
            test_output: None,
        }
    }
    /// Starts opening the default output on its own thread and returns at once; the first use
    /// waits for the open to finish.
    pub fn open_default_once() -> Self {
        match open_on_owner_thread(open_default_stream) {
            Ok((opening, _owner)) => Self {
                opening: Some(opening),
                ..Self::disabled()
            },
            Err(error) => {
                eprintln!("named audio disabled: audio output thread failed to start: {error}");
                Self::disabled()
            }
        }
    }
    /// Takes the output once its owner thread finishes opening it; a failed open stays disabled.
    fn resolve(&mut self) {
        let Some(opening) = self.opening.take() else {
            return;
        };
        if let Ok(Some(((handle, sample_rate), release))) = opening.recv() {
            self.sample_rate = sample_rate;
            self.output = Some(Output {
                handle,
                _release: release,
            });
        }
    }
    /// Output rate that streaming sources should produce to avoid a second conversion.
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
        self.output.is_some()
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
        let Some(Output { handle, .. }) = &self.output else {
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
        self.resolve();
        let Some(Output { handle, .. }) = &self.output else {
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

/// Opens the default output at its default rate, or `None` with a logged reason.
fn open_default_stream() -> Option<(OutputStream, (OutputStreamHandle, u32))> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    let Some(device) = rodio::cpal::default_host().default_output_device() else {
        eprintln!("named audio disabled: no default output device");
        return None;
    };
    // Rodio may negotiate formats on this same device, never another device.
    // Rodio opens the device at its default config first; match that rate.
    let sample_rate = device
        .default_output_config()
        .map_or(MIXER_RATE, |config| config.sample_rate().0);
    match OutputStream::try_from_device(&device) {
        Ok((stream, handle)) => Some((stream, (handle, sample_rate))),
        Err(error) => {
            eprintln!("named audio disabled: default output initialization failed: {error}");
            None
        }
    }
}

/// Runs `open` on a new thread that keeps the stream it opened until the sender sent back with
/// the shared half drops; the stream never leaves that thread. `None` reports a failed open.
fn open_on_owner_thread<S: 'static, H: Send + 'static>(
    open: impl FnOnce() -> Option<(S, H)> + Send + 'static,
) -> std::io::Result<(Opening<H>, std::thread::JoinHandle<()>)> {
    let (opened, opening) = mpsc::channel();
    let owner = std::thread::Builder::new()
        .name("audio-output".to_owned())
        .spawn(move || {
            let Some((stream, shared)) = open() else {
                let _ = opened.send(None);
                return;
            };
            let (release, released) = mpsc::channel::<()>();
            if opened.send(Some((shared, release))).is_ok() {
                // Ends once the device drops its sender.
                let _ = released.recv();
            }
            drop(stream);
        })?;
    Ok((opening, owner))
}

#[cfg(test)]
mod tests;

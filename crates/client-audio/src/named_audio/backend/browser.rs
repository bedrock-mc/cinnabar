//! Web Audio output for the existing admitted Rodio sources.

use rodio::{
    Source,
    cpal::{
        self,
        traits::{DeviceTrait, HostTrait, StreamTrait},
    },
    dynamic_mixer::{DynamicMixerController, mixer},
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

/// Page control of the mixer; dropping it releases the same stream as the audio device.
pub struct BrowserAudio {
    stream: Rc<RefCell<Option<cpal::Stream>>>,
    context: web_sys::AudioContext,
}

impl BrowserAudio {
    /// Borrows the stream's context for gesture resume and cached-page suspension.
    pub fn context(&self) -> &web_sys::AudioContext {
        &self.context
    }
}

impl Drop for BrowserAudio {
    fn drop(&mut self) {
        self.stream.borrow_mut().take();
    }
}

/// Retains CPAL's scheduled Web Audio callbacks and the shared source mixer.
pub(super) struct Output {
    stream: Rc<RefCell<Option<cpal::Stream>>>,
    mixer: Arc<DynamicMixerController<f32>>,
}

impl Output {
    /// Starts scheduling once; later user gestures resume the same AudioContext.
    pub(super) fn open() -> Option<(Self, u32)> {
        let open = || {
            let device = cpal::default_host().default_output_device()?;
            let config = device.default_output_config().ok()?.config();
            let rate = config.sample_rate.0;
            let (controller, mut mixer) = mixer::<f32>(config.channels, rate);
            let stream = device
                .build_output_stream(
                    &config,
                    move |data: &mut [f32], _| {
                        for sample in data {
                            *sample = mixer.next().unwrap_or(0.0);
                        }
                    },
                    |error| bevy::log::warn!(%error, "browser audio output failed"),
                    None,
                )
                .ok()?;
            stream.play().ok()?;
            Some((
                Self {
                    stream: Rc::new(RefCell::new(Some(stream))),
                    mixer: controller,
                },
                rate,
            ))
        };
        let output = open();
        if output.is_none() {
            bevy::log::warn!("browser audio output is unavailable");
        }
        output
    }

    /// Shares control of the output's real context without rescheduling callbacks.
    pub(super) fn control(&self) -> Option<BrowserAudio> {
        let stream = self.stream.borrow();
        let cpal::platform::StreamInner::WebAudio(stream) = stream.as_ref()?.as_inner();
        Some(BrowserAudio {
            stream: Rc::clone(&self.stream),
            context: stream.audio_context().clone(),
        })
    }

    /// Reports whether the retained browser stream is still alive.
    pub(super) fn available(&self) -> bool {
        self.stream.borrow().is_some()
    }

    /// Adds an admitted source to the same mixer used by the native audio engine.
    pub(super) fn play_raw(
        &self,
        source: impl Source<Item = f32> + Send + 'static,
    ) -> Result<(), rodio::PlayError> {
        if self.stream.borrow().is_none() {
            return Err(rodio::PlayError::NoDevice);
        }
        self.mixer.add(source);
        Ok(())
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.stream.borrow_mut().take();
    }
}

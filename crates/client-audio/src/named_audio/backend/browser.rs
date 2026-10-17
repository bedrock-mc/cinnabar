//! Web Audio output for the existing admitted Rodio sources.

use rodio::{
    Source,
    cpal::{
        self,
        traits::{DeviceTrait, HostTrait, StreamTrait},
    },
    dynamic_mixer::{DynamicMixerController, mixer},
};
use std::sync::Arc;

/// Retains CPAL's scheduled Web Audio callbacks and the shared source mixer.
pub(super) struct Output {
    stream: cpal::Stream,
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
                    stream,
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
    pub(super) fn context(&self) -> web_sys::AudioContext {
        let cpal::platform::StreamInner::WebAudio(stream) = self.stream.as_inner();
        stream.audio_context().clone()
    }

    /// Adds an admitted source to the same mixer used by the native audio engine.
    pub(super) fn play_raw(
        &self,
        source: impl Source<Item = f32> + Send + 'static,
    ) -> Result<(), rodio::PlayError> {
        if self.context().state() == web_sys::AudioContextState::Closed {
            return Err(rodio::PlayError::NoDevice);
        }
        self.mixer.add(source);
        Ok(())
    }
}

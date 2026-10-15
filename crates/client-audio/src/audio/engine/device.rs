//! Admitted sources wait here without advancing until an output transport is ready.

use super::{AudioEngine, VoiceSource};
use crate::named_audio::AudioDevice;

impl AudioEngine {
    /// Retains live sources during opening; cancellation and voice admission bound the queue.
    /// Ready transports consume each source once; absence or failure drops the remaining sources.
    pub(in crate::audio) fn submit_sources(
        &mut self,
        sources: Vec<VoiceSource>,
        device: Option<&mut AudioDevice>,
    ) {
        self.staged.extend(sources);
        self.staged.retain(|source| !source.cancelled());
        let Some(device) = device else {
            self.staged.clear();
            return;
        };
        if device.is_opening() {
            return;
        }
        for source in self.staged.drain(..) {
            if !device.play_source(source) {
                self.stats.backend_failed += 1;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{engine::SoundRequest, settings::AudioSettings};

    #[test]
    fn staged_cancellations_retire_before_admitting_replacements() {
        let (mut device, release, owner) = AudioDevice::delayed_open();
        let mut engine = super::super::tests::engine(&[("waiting", "ui")]);
        for count in 1..=super::super::MAX_VOICES * 2 {
            engine.stop_named("waiting");
            engine.enqueue(SoundRequest::new("waiting"));
            let sources = engine.pump(None, 0.0, &AudioSettings::default());
            engine.submit_sources(sources, Some(&mut device));
            assert_eq!(engine.staged.len(), 1);
            assert_eq!(engine.stats.started, count as u64);
            assert_eq!(engine.stats.voice_limit, 0);
        }
        engine.stop_all();
        let sources = engine.pump(None, 0.0, &AudioSettings::default());
        engine.submit_sources(sources, Some(&mut device));
        assert!(engine.staged.is_empty());
        assert!(engine.voices.is_empty());
        drop(release);
        owner.join().unwrap();
    }

    #[test]
    fn priority_stealing_keeps_staged_sources_within_the_voice_limit() {
        let limit = super::super::MAX_VOICES;
        let names: Vec<_> = (0..limit * 2)
            .map(|index| format!("voice{index}"))
            .collect();
        let definitions: Vec<_> = names
            .iter()
            .enumerate()
            .map(|(index, name)| (name.as_str(), if index < limit { "ambient" } else { "ui" }))
            .collect();
        let mut engine = super::super::tests::engine(&definitions);
        let (mut device, release, owner) = AudioDevice::delayed_open();
        for (index, name) in names.iter().enumerate() {
            engine.enqueue(SoundRequest::new(name.as_str()));
            let sources = engine.pump(None, 0.0, &AudioSettings::default());
            engine.submit_sources(sources, Some(&mut device));
            assert_eq!(engine.staged.len(), (index + 1).min(limit));
        }
        assert_eq!(engine.stats.started, names.len() as u64);
        assert_eq!(engine.stats.voice_limit, 0);
        drop(release);
        owner.join().unwrap();
    }
}

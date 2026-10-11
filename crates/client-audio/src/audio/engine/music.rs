use super::{AudioCategory, AudioEngine};

impl AudioEngine {
    pub(in crate::audio) fn is_category_active(&self, category: AudioCategory) -> bool {
        self.is_playing_category(category)
            || self.queue.iter().any(|request| {
                self.bank
                    .as_ref()
                    .and_then(|bank| bank.definition(&request.name))
                    .is_some_and(|definition| {
                        AudioCategory::from_definition(definition.category.as_deref()) == category
                    })
            })
    }

    pub(in crate::audio) fn fade_out_music(&mut self, seconds: f32) {
        if !seconds.is_finite() || seconds <= 0.0 {
            self.stop_category(AudioCategory::Music);
            return;
        }
        for voice in self
            .voices
            .iter_mut()
            .filter(|voice| voice.category == AudioCategory::Music)
        {
            if voice.target_level != 0.0 {
                voice.target_level = 0.0;
                voice.fade_per_second = voice.level / seconds;
                voice.key = None;
            }
        }
        self.pending
            .retain(|start| start.category != AudioCategory::Music);
        if let Some(bank) = self.bank.as_ref() {
            self.queue.retain(|request| {
                bank.definition(&request.name).is_none_or(|definition| {
                    AudioCategory::from_definition(definition.category.as_deref())
                        != AudioCategory::Music
                })
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{
        ambient::MusicScheduler, bank::SoundBank, engine::SoundRequest, music::drive_music,
        settings::AudioSettings, voice::Pcm,
    };
    use assets::{
        AudioAlternative, AudioDefinition, RuntimeAudioCatalog, SoundBankIndex, SoundEventTables,
    };
    use std::sync::Arc;

    fn music_engine() -> AudioEngine {
        let definitions: Vec<_> = ["music.fixture.game", "music.fixture.credits"]
            .into_iter()
            .map(|name| AudioDefinition {
                identifier: name.into(),
                category: Some("music".into()),
                subtitle: None,
                min_distance: None,
                max_distance: None,
                volume: Some(0.1),
                pitch: None,
                use_legacy_max_distance: None,
                alternatives: vec![AudioAlternative {
                    object_form: false,
                    name: format!("sounds/{name}").into(),
                    weight: 1,
                    volume: None,
                    pitch: None,
                    is_3d: None,
                    stream: None,
                    load_on_low_memory: None,
                }]
                .into(),
            })
            .collect();
        let catalog = RuntimeAudioCatalog::decode(
            &assets::encode_audio_catalog([0; 32], [0; 32], &definitions).unwrap(),
        )
        .unwrap();
        let bytes = assets::encode_sound_bank(b"{}", b"{}", br#"{"game":{"event_name":"music.fixture.game","min_delay":10,"max_delay":10},"credits":{"event_name":"music.fixture.credits","min_delay":0,"max_delay":0}}"#, &[]).unwrap();
        let prefix = assets::sound_bank_prefix_len(&bytes).unwrap();
        let mut bank = SoundBank::from_parts(
            SoundBankIndex::decode_prefix(&bytes[..prefix]).unwrap(),
            SoundEventTables::default(),
            Some(Arc::new(catalog)),
        );
        for definition in definitions {
            bank.insert_test_pcm(
                &definition.alternatives[0].name,
                Arc::new(Pcm {
                    channels: 1,
                    rate: 48_000,
                    samples: vec![1000; 4800].into(),
                }),
            );
        }
        AudioEngine::new(Some(bank))
    }

    #[test]
    fn credits_music_starts_once_after_the_old_context_and_fades_on_completion() {
        let mut engine = music_engine();
        let mut scheduler = MusicScheduler::default();
        let settings = AudioSettings::default();
        drive_music(&mut engine, &mut scheduler, "game", 10.0);
        let mut game = engine.pump(None, 0.0, &settings);
        assert_eq!(game.len(), 1);
        drive_music(&mut engine, &mut scheduler, "credits", 0.0);
        assert!(engine.pump(None, 1.5, &settings).is_empty());
        assert_eq!(engine.voices[0].level, 0.5);
        drive_music(&mut engine, &mut scheduler, "credits", 1.5);
        assert!(engine.pump(None, 1.5, &settings).is_empty());
        assert!(game[0].next().is_none());
        drive_music(&mut engine, &mut scheduler, "credits", 0.0);
        let mut credits = engine.pump(None, 0.0, &settings);
        assert_eq!(credits.len(), 1);
        assert_eq!(engine.voices[0].name.as_ref(), "music.fixture.credits");
        assert!((engine.voices[0].base - 0.1).abs() < f32::EPSILON);
        drive_music(&mut engine, &mut scheduler, "credits", 1.0);
        assert!(engine.pump(None, 0.0, &settings).is_empty());
        drive_music(&mut engine, &mut scheduler, "game", 0.0);
        engine.pump(None, 3.0, &settings);
        assert!(credits[0].next().is_none());
        drive_music(&mut engine, &mut scheduler, "game", 4.0);
        assert!(engine.pump(None, 0.0, &settings).is_empty());
        drive_music(&mut engine, &mut scheduler, "game", 6.0);
        assert_eq!(engine.pump(None, 0.0, &settings).len(), 1);
    }

    #[test]
    fn credits_music_transition_fades_only_music_and_cancels_queued_starts() {
        let mut engine = super::super::tests::engine(&[
            ("music.game", "music"),
            ("music.credits", "music"),
            ("dragon.death", "hostile"),
        ]);
        engine.enqueue(SoundRequest::new("music.game"));
        engine.enqueue(SoundRequest::new("dragon.death"));
        let settings = AudioSettings::default();
        let mut sources = engine.pump(None, 0.0, &settings);
        engine.enqueue(SoundRequest::new("music.credits"));
        engine.fade_out_music(3.0);
        assert!(engine.pump(None, 1.5, &settings).is_empty());
        assert_eq!(engine.voices[0].level, 0.5);
        assert_eq!(engine.voices[1].level, 1.0);
        assert!(sources[0].next().is_some());
        assert!(sources[0].next().is_some());
        assert!(sources[1].next().is_some());
        engine.fade_out_music(3.0);
        engine.pump(None, 1.5, &settings);
        assert!(
            sources[0].next().is_none(),
            "observing the context again must not restart the fade"
        );
        assert!(sources[1].next().is_some());
        assert!(!engine.is_playing_category(AudioCategory::Music));
    }

    #[test]
    fn credits_music_has_one_start_while_queued_or_waiting_for_decode() {
        let mut engine = music_engine();
        let mut scheduler = MusicScheduler::default();
        drive_music(&mut engine, &mut scheduler, "credits", 0.0);
        assert_eq!(engine.queue.len(), 1);
        drive_music(&mut engine, &mut scheduler, "credits", 5.0);
        assert_eq!(
            engine.queue.len(),
            1,
            "an unpumped start already owns the music context"
        );
        let request = engine.queue.pop().unwrap();
        engine.pending.push(super::super::PendingStart {
            request,
            managed: None,
            roll: [0.0; 3],
            path: "sounds/music.fixture.credits".into(),
            category: AudioCategory::Music,
            patient: true,
            queued_at: engine.clock,
        });
        for _ in 0..20 {
            drive_music(&mut engine, &mut scheduler, "credits", 10.0);
            assert!(
                engine.queue.is_empty(),
                "a streamed decode remains active without an age timeout"
            );
        }
        assert_eq!(engine.pending.len(), 1);
    }
}

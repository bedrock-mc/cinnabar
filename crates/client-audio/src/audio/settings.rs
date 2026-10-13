//! Per-category volume sliders and the sound-definition category they gate.

use bevy::prelude::Resource;

use assets::audio_category::AudioCategory;

/// Volume sliders in `[0, 1]`; the master slider scales every other category.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct AudioSettings {
    volumes: [f32; AudioCategory::COUNT],
    /// Skips definitions marked `load_on_low_memory: false`.
    pub low_memory: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            volumes: [1.0; AudioCategory::COUNT],
            low_memory: false,
        }
    }
}

impl AudioSettings {
    pub fn set(&mut self, category: AudioCategory, volume: f32) {
        if volume.is_finite() {
            self.volumes[category.slot()] = volume.clamp(0.0, 1.0);
        }
    }

    /// The category's own slider, before master scaling.
    pub fn volume(&self, category: AudioCategory) -> f32 {
        self.volumes[category.slot()]
    }

    /// Effective gain of `category` through vanilla's channel groups: music sits under master;
    /// every other sound sits under the sound group, and each category group under that.
    pub fn effective(&self, category: AudioCategory) -> f32 {
        let master = self.volumes[AudioCategory::Master.slot()];
        let sound = self.volumes[AudioCategory::Sound.slot()];
        match category {
            AudioCategory::Master => master,
            AudioCategory::Music | AudioCategory::Sound => master * self.volumes[category.slot()],
            _ => master * sound * self.volumes[category.slot()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_scales_every_category() {
        let mut settings = AudioSettings::default();
        settings.set(AudioCategory::Master, 0.5);
        settings.set(AudioCategory::Music, 0.4);
        assert!((settings.effective(AudioCategory::Music) - 0.2).abs() < 1e-6);
        assert!((settings.effective(AudioCategory::Blocks) - 0.5).abs() < 1e-6);
        settings.set(AudioCategory::Ui, f32::NAN);
        assert_eq!(settings.effective(AudioCategory::Ui), 0.5);
    }

    // The sound slider silenced only uncategorized definitions while music ignored it.
    #[test]
    fn sound_slider_gates_every_effect_but_not_music() {
        let mut settings = AudioSettings::default();
        settings.set(AudioCategory::Sound, 0.0);
        for category in [
            AudioCategory::Blocks,
            AudioCategory::Players,
            AudioCategory::Weather,
            AudioCategory::Records,
            AudioCategory::Ui,
            AudioCategory::Sound,
        ] {
            assert_eq!(settings.effective(category), 0.0, "{category:?}");
        }
        assert_eq!(settings.effective(AudioCategory::Music), 1.0);
        settings.set(AudioCategory::Sound, 0.5);
        settings.set(AudioCategory::Hostile, 0.5);
        assert!((settings.effective(AudioCategory::Hostile) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn definition_categories_map_to_buckets() {
        assert_eq!(
            AudioCategory::from_definition(Some("record")),
            AudioCategory::Records
        );
        assert_eq!(
            AudioCategory::from_definition(Some("bottle")),
            AudioCategory::Sound
        );
        assert_eq!(AudioCategory::from_definition(None), AudioCategory::Sound);
    }
}

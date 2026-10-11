use crate::audio::{AudioSettings, LocalBlockCue, UiSoundCue};

/// Installs audio's settings and local cue queues without changing frame ordering.
pub struct ClientAudioPlugin;
impl bevy::prelude::Plugin for ClientAudioPlugin {
    /// Initializes retained audio state and local cue queues.
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_resource::<crate::session_audio::SessionAudio>()
            .init_resource::<crate::named_audio::NamedAudio>()
            .init_resource::<crate::audio::AudioEngine>()
            .init_resource::<AudioSettings>()
            .add_message::<UiSoundCue>()
            .add_message::<LocalBlockCue>();
    }
}

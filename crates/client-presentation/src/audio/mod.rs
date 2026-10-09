//! Positional, category-mixed sound playback driven by packets, local motion and ambience.

pub mod ambient;
mod bank;
pub mod echo;
pub mod engine;
pub mod inventory;
mod listener;
pub mod local;
pub mod media;
mod music;
pub mod predicted;
mod route;
mod server;
pub mod settings;
pub mod systems;
mod voice;
mod water;

pub use bank::{SoundBank, sound_bank_path};
pub use engine::AudioEngine;
pub use predicted::LocalBlockCue;
#[cfg(any(test, feature = "test-support"))]
pub use server::{SERVER_SOUNDS_TEST_LOCK, current_generation as server_sounds_generation};
pub use server::{ServerSoundPack, publish_server_sounds};
#[allow(unused_imports)]
pub use settings::{AudioCategory, AudioSettings};
#[allow(unused_imports)]
pub use systems::{UiSoundCue, ui_sound};
pub use voice::OUTPUT_RATE;

pub use echo::{EchoLedger, EchoOrigin, EchoSubject};
pub use systems::BLOCK_ECHO_SECONDS;

/// Installs audio's settings and local cue queues without changing frame ordering.
pub struct AudioPresentationPlugin;
impl bevy::prelude::Plugin for AudioPresentationPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_resource::<AudioSettings>()
            .add_message::<UiSoundCue>()
            .add_message::<LocalBlockCue>();
    }
}

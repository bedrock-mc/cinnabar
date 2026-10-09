//! Composition adapters for the presentation audio plugin.
mod predicted;
mod synchronized;
mod systems;
pub use client_presentation::audio::{
    AudioCategory, AudioEngine, AudioSettings, BLOCK_ECHO_SECONDS, EchoLedger, EchoOrigin,
    EchoSubject, LocalBlockCue, ServerSoundPack, SoundBank, publish_server_sounds, sound_bank_path,
};
pub(crate) use systems::configure;

#[cfg(test)]
mod local_bank_tests;

#[cfg(test)]
pub(crate) use predicted::drive_block_cues;

#[cfg(test)]
pub(crate) use client_presentation::audio::{SERVER_SOUNDS_TEST_LOCK, server_sounds_generation};

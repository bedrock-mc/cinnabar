//! Audio playback, local sound cues and session sound admission.
pub mod audio;
pub mod audio_ingress;
pub mod named_audio;
pub mod observations;
pub mod session_audio;

mod plugin;
pub use plugin::ClientAudioPlugin;

#[cfg(test)]
mod test_allocations;

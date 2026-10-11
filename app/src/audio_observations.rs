//! Borrows retained particle notices for audio.
use client_audio::observations::ParticleAudioObservation;

impl ParticleAudioObservation for crate::particles::ParticleInbox {
    /// Drains only audio's existing copy of actor status notices.
    fn take_status_audio(&mut self) -> Vec<client_world::ActorStatusNotice> {
        self.take_status_audio()
    }
    /// Drains only audio's existing copy of level events.
    fn take_level_audio(&mut self) -> Vec<(i32, [f32; 3], i32)> {
        self.take_level_audio()
    }
}

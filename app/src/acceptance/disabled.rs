//! Keeps the existing movement deadline observation available without evidence support.
//!
//! The gameplay extraction owns that system's signature. Until it consumes a separate
//! deadline observation, the host supplies this empty resource when acceptance is absent.
//! Argument validation rejects every acceptance timer in this build configuration.

#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct AcceptanceRun;

impl AcceptanceRun {
    /// Reports the absence of a configured acceptance deadline to the movement adapter.
    pub(crate) fn deadline_reached(&self, _now: std::time::Instant) -> bool {
        false
    }
}

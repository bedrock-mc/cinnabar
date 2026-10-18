use bevy::prelude::Resource;

use client_presentation::environment::WorldClock;

/// Validated mod output, applied only to a copy used by atmosphere rendering.
#[derive(Resource, Default)]
pub(crate) struct VisualTimeOverride(pub(crate) Option<u32>);

impl VisualTimeOverride {
    /// Keeps the server snapshot intact while fixing the rendered clock.
    pub(crate) fn rendering_clock(&self, server: WorldClock) -> WorldClock {
        match self.0 {
            Some(ticks) => server.with_visual_time(ticks),
            None => server,
        }
    }
}

#[cfg(test)]
mod tests;

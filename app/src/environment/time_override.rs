use bevy::prelude::Resource;

use super::WorldClock;

/// Validated mod output, applied only to a copy used by atmosphere rendering.
#[derive(Resource, Default)]
pub(crate) struct VisualTimeOverride(pub(crate) Option<u32>);

impl VisualTimeOverride {
    /// Keeps the server snapshot intact while fixing the rendered clock.
    pub(crate) fn rendering_clock(&self, server: WorldClock) -> WorldClock {
        match self.0 {
            Some(ticks) => WorldClock {
                server_time: Some(f64::from(ticks)),
                server_time_anchor_seconds: Some(0.0),
                daylight_cycle_enabled: false,
                ..server
            },
            None => server,
        }
    }
}

/// Local renderer-only time used by the Enhanced-mode lighting probe.
///
/// This stays separate from mod output so a local component cannot accidentally
/// claim ownership of the built-in debug control.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DebugTimeOverride {
    pub(crate) ticks: Option<u32>,
}

#[cfg(test)]
mod tests;

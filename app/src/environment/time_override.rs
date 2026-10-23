use bevy::prelude::Resource;

use client_presentation::environment::WorldClock;

/// Validated personal-mod time override; the authoritative clock keeps receiving updates.
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

/// Developer-only `--fixed-time` pin for captures that compare against a reference renderer.
///
/// Set once at launch from the command line and never changed; it outranks mod output so a
/// mod's time changer cannot move a capture's clock.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FixedTime(pub(crate) Option<u32>);

/// Selects the rendered clock: capture pin, personal mod, client part, then server time.
pub(crate) fn rendered_clock(
    server: WorldClock,
    fixed: Option<&FixedTime>,
    visual: Option<&VisualTimeOverride>,
    client_part: Option<&super::ClientPartEnvironment>,
) -> WorldClock {
    let ticks = fixed
        .and_then(|fixed| fixed.0)
        .or_else(|| visual.and_then(|visual| visual.0))
        .or_else(|| client_part.and_then(|client_part| client_part.time));
    VisualTimeOverride(ticks).rendering_clock(server)
}

#[cfg(test)]
mod tests;

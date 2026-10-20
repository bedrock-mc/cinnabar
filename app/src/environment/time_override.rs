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

/// Developer-only `--fixed-time` pin for captures that compare against a reference renderer.
///
/// Set once at launch from the command line and never changed; it outranks mod output so a
/// mod's time changer cannot move a capture's clock.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FixedTime(pub(crate) Option<u32>);

/// The clock the renderer draws with. `--fixed-time` wins, then mod output; without either
/// the server clock passes through unchanged.
pub(crate) fn rendered_clock(
    server: WorldClock,
    fixed: Option<&FixedTime>,
    visual: Option<&VisualTimeOverride>,
) -> WorldClock {
    let ticks = fixed
        .and_then(|fixed| fixed.0)
        .or_else(|| visual.and_then(|visual| visual.0));
    VisualTimeOverride(ticks).rendering_clock(server)
}

#[cfg(test)]
mod tests;

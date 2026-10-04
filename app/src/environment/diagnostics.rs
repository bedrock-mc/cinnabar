//! Bounded session diagnostics for black terrain, sky and first-person hands.

use bevy::prelude::{Local, Res, Time};
use bevy::time::Real;
use render::{AtmosphereFrame, WorldLighting};

use super::{EnvironmentContext, EnvironmentProfileRoute, WorldClock};
use crate::{local_player::LocalPlayerFrameCarrier, runtime::world::ClientWorld};

const INTERVAL_SECONDS: f64 = 5.0;

#[derive(Default)]
pub(crate) struct LightingLogState {
    identity: Option<(u64, i32)>,
    next_seconds: f64,
    session: Option<u64>,
    mode_mask: u8,
}

impl LightingLogState {
    /// Emits immediately for a new session or dimension, then at most every five seconds.
    fn due(&mut self, identity: (u64, i32), seconds: f64) -> bool {
        if self.identity == Some(identity) && seconds < self.next_seconds {
            return false;
        }
        self.identity = Some(identity);
        self.next_seconds = seconds + INTERVAL_SECONDS;
        true
    }

    /// Starts the two session-only facts again when the network session changes.
    fn start_session(&mut self, session: u64) -> bool {
        if self.session == Some(session) {
            return false;
        }
        self.session = Some(session);
        self.mode_mask = 0;
        true
    }

    /// Logs a newly observed mode family once even if later headers repeat it.
    fn observe_mode(&mut self, index: usize) -> bool {
        let mask = 1 << index;
        if self.mode_mask & mask != 0 {
            return false;
        }
        self.mode_mask |= mask;
        true
    }
}

/// Logs solved eye light alongside the actual lightmap and atmosphere inputs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn log_world_lighting(
    world: Res<ClientWorld>,
    player: Res<LocalPlayerFrameCarrier>,
    clock: Res<WorldClock>,
    context: Res<EnvironmentContext>,
    route: Res<EnvironmentProfileRoute>,
    frame: Res<AtmosphereFrame>,
    light: Res<WorldLighting>,
    time: Res<Time<Real>>,
    mut state: Local<LightingLogState>,
) {
    let Some(stream) = world.stream.as_ref() else {
        return;
    };
    if !state.due(
        (clock.session_generation(), context.dimension),
        time.elapsed_secs_f64(),
    ) {
        return;
    }
    let eye = player.snapshot().map(|player| player.eye().to_array());
    let session = clock.session_generation();
    if state.start_session(session) {
        bevy::log::info!(session_generation = session, facts = %stream.lighting_session_facts(), "WORLD_LIGHTING_SESSION");
    }
    for (index, mode) in stream.lighting_request_modes().into_iter().enumerate() {
        if let Some(mode) = mode
            && state.observe_mode(index)
        {
            bevy::log::info!(session_generation = session, %mode, "WORLD_LIGHTING_REQUEST_MODE");
        }
    }
    let block_evidence = eye.map(|eye| {
        let positions =
            [0.0, 1.0, 2.0, 3.0, 4.0, 8.0, 12.0].map(|depth| [eye[0], eye[1] - depth, eye[2]]);
        stream.lighting_diagnostic(&positions)
    });
    let table = light.0.build();
    bevy::log::info!(
        session_generation = clock.session_generation(),
        dimension = context.dimension,
        ?eye,
        block_evidence = %block_evidence.as_deref().unwrap_or("eye-unavailable"),
        medium = ?frame.camera_medium(),
        sky_kind = ?frame.sky_kind(),
        daylight = frame.daylight(),
        sky_darken = light.0.sky_darken,
        brightness = light.0.brightness,
        darkness = light.0.darkness,
        darkness_pulse = light.0.darkness_pulse,
        night_vision = light.0.night_vision,
        fog_start = frame.fog_start(),
        fog_end = frame.fog_end(),
        fog_color = ?frame.fog_color(),
        sky_zenith = ?frame.sky_zenith(),
        biome = ?route.biome_identifier,
        fog = ?route.fog_identifier,
        lightmap_dark = ?table[0],
        lightmap_full = ?table[255],
        "WORLD_LIGHTING"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lighting_diagnostics_are_bounded_and_reset_for_session_or_dimension() {
        let mut state = LightingLogState::default();
        assert!(state.due((1, 0), 0.0));
        assert!(!state.due((1, 0), 0.01));
        assert!(!state.due((1, 0), 4.99));
        assert!(state.due((1, 0), INTERVAL_SECONDS));
        assert!(state.due((2, 0), INTERVAL_SECONDS));
        assert!(state.due((2, 1), INTERVAL_SECONDS));
        assert!(!state.due((2, 1), INTERVAL_SECONDS + 0.01));
    }

    #[test]
    fn session_facts_and_each_request_mode_log_once_even_across_dimension_changes() {
        let mut state = LightingLogState::default();
        assert!(state.start_session(1));
        for index in 0..3 {
            assert!(state.observe_mode(index));
        }
        assert!(!state.start_session(1));
        assert!(state.due((1, 1), 10.0));
        for index in 0..3 {
            assert!(!state.observe_mode(index));
        }
        assert!(state.start_session(2));
        for index in 0..3 {
            assert!(state.observe_mode(index));
        }
    }
}

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
    let solved_eye_light = eye.and_then(|eye| stream.solved_light_at(eye));
    let table = light.0.build();
    bevy::log::info!(
        session_generation = clock.session_generation(),
        dimension = context.dimension,
        ?eye,
        ?solved_eye_light,
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
}

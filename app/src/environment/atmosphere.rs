//! Per-frame atmosphere derivation: clock and weather into one shared GPU
//! frame, overlaid with client fog profiles and explicit boss requests.

use assets::{BiomeVisualProfile, FogMedium, FogProfile};
use bevy::{
    prelude::{Res, ResMut, Time},
    time::Real,
};
use meshing::CameraMedium;
use render::AtmosphereFrame;
use ui::BossBarView;

use crate::ui_runtime::UiRuntime;

use super::{
    CameraMediumState, EnvironmentContext, EnvironmentProfileRoute, WeatherState, WorldClock,
    profile_lookup::{dimension_fallback_biome, find_biome_profile},
    visual_world_time,
};

#[must_use]
#[cfg(test)]
pub(crate) fn derive_atmosphere_frame(
    clock: WorldClock,
    weather: WeatherState,
    elapsed_seconds: f64,
) -> AtmosphereFrame {
    derive_atmosphere_frame_for_medium(clock, weather, elapsed_seconds, CameraMedium::Air)
}

#[must_use]
pub(crate) fn derive_atmosphere_frame_for_medium(
    clock: WorldClock,
    weather: WeatherState,
    elapsed_seconds: f64,
    medium: CameraMedium,
) -> AtmosphereFrame {
    AtmosphereFrame::from_bedrock_time(
        visual_world_time(clock, elapsed_seconds),
        weather.rain_level,
        weather.lightning_level,
    )
    .with_camera_medium(medium)
}

#[must_use]
pub(crate) fn derive_profiled_atmosphere_frame(
    clock: WorldClock,
    weather: WeatherState,
    elapsed_seconds: f64,
    medium: CameraMedium,
    context: &EnvironmentContext,
    biome_profiles: &[BiomeVisualProfile],
    fog_profiles: &[FogProfile],
) -> (AtmosphereFrame, EnvironmentProfileRoute) {
    let base = derive_atmosphere_frame_for_medium(clock, weather, elapsed_seconds, medium);
    let profile = context
        .camera_biome_identifier
        .as_deref()
        .and_then(|identifier| find_biome_profile(biome_profiles, identifier))
        .or_else(|| {
            dimension_fallback_biome(context.dimension)
                .and_then(|identifier| find_biome_profile(biome_profiles, identifier))
        });
    let Some(profile) = profile else {
        return (base, EnvironmentProfileRoute::default());
    };
    let resolved_fog = context.render_distance_blocks.and_then(|render_distance| {
        let fog = fog_profiles
            .binary_search_by(|fog| fog.identifier.cmp(&profile.fog_identifier))
            .ok()
            .map(|index| &fog_profiles[index])?;
        let default_fog = fog_profiles
            .binary_search_by(|fog| fog.identifier.as_ref().cmp("minecraft:fog_default"))
            .ok()
            .map(|index| &fog_profiles[index]);
        let distance = |medium| {
            fog.distance(medium)
                .or_else(|| default_fog.and_then(|fallback| fallback.distance(medium)))
        };
        let requested = match medium {
            CameraMedium::Air
                if weather.rain_level > 0.0 && distance(FogMedium::Weather).is_some() =>
            {
                FogMedium::Weather
            }
            CameraMedium::Air => FogMedium::Air,
            CameraMedium::Water => FogMedium::Water,
            CameraMedium::Lava => FogMedium::Lava,
        };
        distance(requested)?.resolve(render_distance)
    });
    (
        base.with_environment_profile(profile.sky_rgb8, resolved_fog),
        EnvironmentProfileRoute {
            biome_identifier: Some(profile.biome_identifier.clone()),
            fog_identifier: Some(profile.fog_identifier.clone()),
            atmosphere_identifier: Some(profile.atmosphere_identifier.clone()),
            provisional_lighting_identifier: Some(profile.lighting_identifier.clone()),
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_atmosphere_frame(
    clock: Res<WorldClock>,
    weather: Res<WeatherState>,
    medium: Res<CameraMediumState>,
    context: Res<EnvironmentContext>,
    boss_bars: Res<UiRuntime>,
    atmosphere_assets: Res<render::AtmosphereTextureAssets>,
    time: Res<Time<Real>>,
    outputs: (ResMut<AtmosphereFrame>, ResMut<EnvironmentProfileRoute>),
) {
    let (mut frame, mut route) = outputs;
    let state = derive_boss_environment(&boss_bars.boss_bars().stacked());
    let Some(assets) = atmosphere_assets.runtime() else {
        *frame = apply_boss_environment(
            derive_atmosphere_frame_for_medium(*clock, *weather, time.elapsed_secs_f64(), medium.0),
            medium.0,
            state,
        );
        *route = EnvironmentProfileRoute::default();
        return;
    };
    let (next_frame, next_route) = derive_profiled_atmosphere_frame(
        *clock,
        *weather,
        time.elapsed_secs_f64(),
        medium.0,
        &context,
        assets.biome_profiles(),
        assets.fog_profiles(),
    );
    *frame = apply_boss_environment(next_frame, medium.0, state);
    *route = next_route;
}

/// Explicit environment requests retained by active boss bars.
///
/// The pinned protocol-2168 `BossEvent` wire carries no sky-darkening or
/// world-fog fields, so live servers leave both flags unset and this state
/// is inert until a bar explicitly requests an effect.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct BossEnvironmentState {
    pub(crate) darken_sky: bool,
    pub(crate) world_fog: bool,
}

pub(crate) fn derive_boss_environment(bars: &[BossBarView]) -> BossEnvironmentState {
    BossEnvironmentState {
        darken_sky: bars.iter().any(|bar| bar.style.darken_sky == Some(true)),
        world_fog: bars
            .iter()
            .any(|bar| bar.style.create_world_fog == Some(true)),
    }
}

/// Boss effects respond only in air; water and lava media own their fog
/// completely and must not be overridden by a boss flag.
pub(crate) fn apply_boss_environment(
    frame: AtmosphereFrame,
    medium: CameraMedium,
    state: BossEnvironmentState,
) -> AtmosphereFrame {
    match medium {
        CameraMedium::Air => frame.with_boss_environment(state.darken_sky, state.world_fog),
        CameraMedium::Water | CameraMedium::Lava => frame,
    }
}

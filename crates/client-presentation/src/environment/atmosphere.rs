//! Derives atmosphere frames from clock, weather, sampled profiles, and explicit boss requests.

use assets::{BiomeVisualProfile, FogMedium, FogProfile, facts::dimension_fallback_biome};
use meshing::CameraMedium;
use render_scene::{AtmosphereFrame, SkyKind};
use ui::BossBarView;

use super::{
    EnvironmentContext, EnvironmentProfileRoute, WeatherState, WorldClock,
    profile_lookup::find_biome_profile, visual_world_time,
};

/// Derives the air-medium atmosphere from server clock and weather state.
#[must_use]
#[cfg(test)]
pub(super) fn derive_atmosphere_frame(
    clock: WorldClock,
    weather: WeatherState,
    elapsed_seconds: f64,
) -> AtmosphereFrame {
    derive_atmosphere_frame_for_medium(clock, weather, elapsed_seconds, CameraMedium::Air)
}

/// Derives the unprofiled atmosphere for the requested camera medium.
#[must_use]
pub fn derive_atmosphere_frame_for_medium(
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

/// Clock, weather, dimension sky and biome temperature, before any client profile.
pub fn derive_base_frame(
    clock: WorldClock,
    weather: WeatherState,
    elapsed_seconds: f64,
    medium: CameraMedium,
    context: &EnvironmentContext,
) -> AtmosphereFrame {
    // Player's packet radius includes one extra chunk before the camera margin.
    // Vanilla's ordinary render parameters supply coefficient1; optional platform
    // caps need their own admission witness, not a quality multiplier.
    let adjusted_render_distance = context
        .render_distance_blocks
        .and_then(render_scene::adjusted_player_render_distance_blocks)
        .unwrap_or(0.0);
    let frame = derive_atmosphere_frame_for_medium(clock, weather, elapsed_seconds, medium)
        .with_sky_kind(SkyKind::from_dimension(context.dimension))
        .with_cloud_fade_distance(adjusted_render_distance)
        .with_liquid_render_distance(adjusted_render_distance);
    match context.camera_biome_temperature {
        Some(temperature) => frame.with_biome_temperature(temperature),
        None => frame,
    }
}

/// Resolves sampled indices only in their owning registry, using camera names after a reload.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn derive_profiled_atmosphere_frame(
    clock: WorldClock,
    weather: WeatherState,
    elapsed_seconds: f64,
    medium: CameraMedium,
    context: &EnvironmentContext,
    biome_profiles: &[BiomeVisualProfile],
    fog_profiles: &[FogProfile],
    registry_generation: [u8; 32],
    transition_seconds: Option<f32>,
    fog_weather_level: f32,
) -> (AtmosphereFrame, EnvironmentProfileRoute) {
    let base = derive_base_frame(clock, weather, elapsed_seconds, medium, context);
    // A pack can change after sampling. Use camera names until the next lattice sample.
    let indexed = context.fog_registry_generation == Some(registry_generation);
    let profile = context
        .camera_profile
        .filter(|_| indexed)
        .and_then(|index| biome_profiles.get(index.0))
        .or_else(|| {
            context
                .camera_biome_identifier
                .as_deref()
                .and_then(|identifier| find_biome_profile(biome_profiles, identifier))
                .or_else(|| {
                    dimension_fallback_biome(context.dimension)
                        .and_then(|identifier| find_biome_profile(biome_profiles, identifier))
                })
        });
    let Some(profile) = profile else {
        return (base, EnvironmentProfileRoute::default());
    };
    let samples = context
        .fog_biomes
        .filter(|_| indexed)
        .map(|samples| samples.map(|index| index.and_then(|index| fog_profiles.get(index.0))));
    let (camera_fog, default_fog) = if indexed && context.profile_route.is_some() {
        (
            context
                .camera_fog
                .and_then(|index| fog_profiles.get(index.0)),
            context
                .default_fog
                .and_then(|index| fog_profiles.get(index.0)),
        )
    } else {
        (
            fog_profiles
                .binary_search_by(|fog| fog.identifier.cmp(&profile.fog_identifier))
                .ok()
                .map(|index| &fog_profiles[index]),
            fog_profiles
                .binary_search_by(|fog| fog.identifier.as_ref().cmp("minecraft:fog_default"))
                .ok()
                .map(|index| &fog_profiles[index]),
        )
    };
    let resolve = |requested: FogMedium| {
        // Air fog profiles use the adjusted render distance in the
        // ordinary above-water route, not the raw packet.
        // The separate native submerged distance admission remains incomplete.
        let render_distance =
            render_scene::adjusted_player_render_distance_blocks(context.render_distance_blocks?)?;
        let camera = [Some(camera_fog?)];
        let layer = samples.as_ref().map_or(&camera[..], |samples| &samples[..]);
        assets::resolve_fog_layers(
            &[layer],
            default_fog,
            requested,
            render_distance,
            transition_seconds.filter(|_| requested == FogMedium::Water),
        )
    };
    let profiled = base.with_environment_profile(profile.sky_rgb8, None);
    let frame = match medium {
        CameraMedium::Air => profiled.with_blended_fog(
            resolve(FogMedium::Air),
            resolve(FogMedium::Weather),
            fog_weather_level,
        ),
        CameraMedium::Water => profiled.with_environment_profile(None, resolve(FogMedium::Water)),
        CameraMedium::Lava => profiled.with_environment_profile(None, resolve(FogMedium::Lava)),
    };
    (
        frame,
        context
            .profile_route
            .as_deref()
            .filter(|_| indexed)
            .cloned()
            .unwrap_or_else(|| EnvironmentProfileRoute {
                biome_identifier: Some(std::sync::Arc::from(profile.biome_identifier.as_ref())),
                fog_identifier: Some(std::sync::Arc::from(profile.fog_identifier.as_ref())),
                atmosphere_identifier: Some(std::sync::Arc::from(
                    profile.atmosphere_identifier.as_ref(),
                )),
                provisional_lighting_identifier: Some(std::sync::Arc::from(
                    profile.lighting_identifier.as_ref(),
                )),
            }),
    )
}

/// Explicit environment requests retained by active boss bars.
///
/// The pinned protocol-2168 `BossEvent` wire carries no sky-darkening or
/// world-fog fields, so live servers leave both flags unset and this state
/// is inert until a bar explicitly requests an effect.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BossEnvironmentState {
    pub darken_sky: bool,
    pub world_fog: bool,
}

/// Collects explicit boss effects from a borrowed bar slice.
#[cfg(test)]
pub(super) fn derive_boss_environment(bars: &[BossBarView]) -> BossEnvironmentState {
    derive_boss_environment_iter(bars)
}

/// Derives boss environment flags from an allocation-free boss-bar query.
pub fn derive_boss_environment_iter<I>(bars: I) -> BossEnvironmentState
where
    I: IntoIterator,
    I::Item: std::borrow::Borrow<BossBarView>,
{
    let mut state = BossEnvironmentState::default();
    for bar in bars {
        let bar = std::borrow::Borrow::borrow(&bar);
        state.darken_sky |= bar.style.darken_sky == Some(true);
        state.world_fog |= bar.style.create_world_fog == Some(true);
    }
    state
}

/// Boss effects respond only in air; water and lava media own their fog
/// completely and must not be overridden by a boss flag.
pub fn apply_boss_environment(
    frame: AtmosphereFrame,
    medium: CameraMedium,
    state: BossEnvironmentState,
) -> AtmosphereFrame {
    match medium {
        CameraMedium::Air => frame.with_boss_environment(state.darken_sky, state.world_fog),
        CameraMedium::Water | CameraMedium::Lava => frame,
    }
}

#[cfg(test)]
mod liquid_distance_tests {
    use super::*;

    #[test]
    fn base_frame_uses_native_player_margin_before_camera_distance_adjustment() {
        let clock = WorldClock::default();
        let weather = WeatherState::default();
        for (confirmed, adjusted) in [(32.0, 45.0), (160.0, 160.0), (256.0, 256.0)] {
            let context = EnvironmentContext {
                render_distance_blocks: Some(confirmed),
                ..Default::default()
            };
            let actual = derive_base_frame(clock, weather, 0.0, CameraMedium::Air, &context);
            let expected =
                derive_atmosphere_frame_for_medium(clock, weather, 0.0, CameraMedium::Air)
                    .with_cloud_fade_distance(adjusted)
                    .with_liquid_render_distance(adjusted);
            assert_eq!(actual, expected);
        }
    }
}

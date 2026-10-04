//! Per-frame atmosphere derivation: clock and weather into one shared GPU
//! frame, overlaid with client fog profiles and explicit boss requests.

use assets::{BiomeVisualProfile, FogMedium, FogProfile};
use bevy::{
    prelude::{Local, Query, Res, ResMut, Time, Transform, With},
    time::Real,
};
use meshing::CameraMedium;
use render::{AtmosphereFrame, SkyKind};
use ui::BossBarView;

use client_ui::ui_runtime::UiRuntime;

use super::{
    CameraMediumState, EnvironmentContext, EnvironmentProfileRoute, LightningFlashState,
    WeatherDisplay, WeatherState, WorldClock,
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

/// Clock, weather, dimension sky and biome temperature, before any client profile.
fn derive_base_frame(
    clock: WorldClock,
    weather: WeatherState,
    elapsed_seconds: f64,
    medium: CameraMedium,
    context: &EnvironmentContext,
) -> AtmosphereFrame {
    // Player's packet radius includes one extra chunk before the camera margin.
    // Ordinary preRenderParameters supplies coefficient1; optional platform
    // caps need their own admission witness, not a quality multiplier.
    let adjusted_render_distance = context
        .render_distance_blocks
        .and_then(render::adjusted_player_render_distance_blocks)
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

type AtmosphereOutputs<'w> = (
    ResMut<'w, AtmosphereFrame>,
    ResMut<'w, EnvironmentProfileRoute>,
    ResMut<'w, render::WorldLighting>,
    ResMut<'w, super::WeatherTickFrame>,
    ResMut<'w, render::AtmosphereViewInputs>,
);

#[must_use]
#[allow(clippy::too_many_arguments)]
pub(crate) fn derive_profiled_atmosphere_frame(
    clock: WorldClock,
    weather: WeatherState,
    elapsed_seconds: f64,
    medium: CameraMedium,
    context: &EnvironmentContext,
    biome_profiles: &[BiomeVisualProfile],
    fog_profiles: &[FogProfile],
    transition_seconds: Option<f32>,
    fog_weather_level: f32,
) -> (AtmosphereFrame, EnvironmentProfileRoute) {
    let base = derive_base_frame(clock, weather, elapsed_seconds, medium, context);
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
    let resolve = |requested: FogMedium| {
        // Air fog profiles use the adjusted render distance in the
        // ordinary above-water route, not the raw packet.
        // The separate native submerged distance admission remains incomplete.
        let render_distance =
            render::adjusted_player_render_distance_blocks(context.render_distance_blocks?)?;
        let fog = fog_profiles
            .binary_search_by(|fog| fog.identifier.cmp(&profile.fog_identifier))
            .ok()
            .map(|index| &fog_profiles[index])?;
        let default_fog = fog_profiles
            .binary_search_by(|fog| fog.identifier.as_ref().cmp("minecraft:fog_default"))
            .ok()
            .map(|index| &fog_profiles[index]);
        let samples: Vec<_> = context
            .fog_biomes
            .iter()
            .map(|id| {
                let profile = find_biome_profile(biome_profiles, id.as_deref()?)?;
                fog_profiles
                    .binary_search_by(|fog| fog.identifier.cmp(&profile.fog_identifier))
                    .ok()
                    .map(|index| &fog_profiles[index])
            })
            .collect();
        let camera = [Some(fog)];
        let layer = if samples.is_empty() {
            &camera[..]
        } else {
            &samples[..]
        };
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
    time_override: Option<Res<super::VisualTimeOverride>>,
    weather: Res<WeatherState>,
    medium: Res<CameraMediumState>,
    context: Res<EnvironmentContext>,
    boss_bars: Res<UiRuntime>,
    atmosphere_assets: Res<render::AtmosphereTextureAssets>,
    time: Res<Time<Real>>,
    flash: Res<LightningFlashState>,
    vision: Res<crate::camera::VisionEffects>,
    outputs: AtmosphereOutputs,
    settings: Res<crate::settings_runtime::RuntimeSettings>,
    mut display: Local<WeatherDisplay>,
    mut renderer_clock: Local<super::renderer_clock::RendererClock>,
    preferences: (
        Option<Res<crate::menu::MenuRuntime>>,
        Option<ResMut<render::CloudVisibility>>,
    ),
    cameras: Query<&Transform, With<crate::camera::FlyCamera>>,
) {
    let renderer_ticks = renderer_clock.advance(
        clock.server_time().map(|_| clock.session_generation),
        time.elapsed_secs_f64(),
    );
    let clock = time_override
        .as_ref()
        .map_or(*clock, |value| value.rendering_clock(*clock));
    let (menu, clouds) = preferences;
    let options = menu.as_ref().map(|menu| menu.settings_snapshot().0);
    if let Some(mut clouds) = clouds {
        clouds.0 = options
            .as_ref()
            .is_none_or(|options| options.value("render_clouds") != 0);
    }
    let darkness_scale = options
        .as_ref()
        .map_or(1.0, |options| options.value("darkness") as f32 / 100.0);
    let (mut frame, mut route, mut lighting, mut weather_ticks, mut view_inputs) = outputs;
    let elapsed = time.elapsed_secs_f64();
    display.set_precipitation_count(context.precipitation_sample_count);
    let shown = display.advance_in_dimension(*weather, elapsed, context.dimension);
    *view_inputs = render::AtmosphereViewInputs {
        forward: cameras
            .single()
            .map_or([0.0; 3], |transform| transform.forward().to_array()),
        fog_weather_level: display.fog_level(),
        current_rain_level: display.current_rain_level(),
    };
    display.publish_ticks(&mut weather_ticks);
    let state = derive_boss_environment_iter(boss_bars.boss_bars().stacked_iter());
    let submerged = display.submerged_seconds(medium.0);
    let (next_frame, next_route) = match atmosphere_assets.runtime() {
        Some(assets) => derive_profiled_atmosphere_frame(
            clock,
            shown,
            elapsed,
            medium.0,
            &context,
            assets.biome_profiles(),
            assets.fog_profiles(),
            Some(submerged),
            display.fog_level(),
        ),
        None => (
            derive_base_frame(clock, shown, elapsed, medium.0, &context),
            EnvironmentProfileRoute::default(),
        ),
    };
    let next_frame = next_frame
        .with_camera_environment(*view_inputs)
        .with_cloud_renderer_ticks(renderer_ticks)
        .with_lightning_flash(flash.level(elapsed))
        .with_vision_effects(
            vision.blindness,
            vision.darkness * darkness_scale,
            vision.night_vision,
        );
    *frame = apply_boss_environment(next_frame, medium.0, state);
    let mut sunrise = frame.sunrise_band();
    for c in &mut sunrise[..3] {
        *c = if *c <= 0.0031308 {
            *c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
    }
    lighting.0 = render::LightmapInputs {
        // Ordinary native renderer supplies flag1 to update.
        // buildImage uses it for both ambient stages around gamma;
        // omitting it crushes shaded terrain at night, despite matching gamma.
        ambient_adjustment: true,
        sky_darken: render::lightmap_sky_darken(
            frame.celestial_angle(),
            display.fog_level(),
            shown.lightning_level,
        ),
        sunrise,
        lightning: frame.lightning_flash() > 0.0,
        brightness: settings.user_settings_update().1.video.brightness,
        night_vision: vision.night_vision,
        darkness: vision.darkness * darkness_scale,
        darkness_pulse: render::darkness_pulse(
            visual_world_time(clock, elapsed) as f32,
            0.0,
            vision.darkness * darkness_scale,
            vision.darkness * darkness_scale,
            0.45,
        ),
        ..Default::default()
    };
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

#[cfg(test)]
pub(crate) fn derive_boss_environment(bars: &[BossBarView]) -> BossEnvironmentState {
    derive_boss_environment_iter(bars)
}

/// Derives boss environment flags from an allocation-free boss-bar query.
fn derive_boss_environment_iter<I>(bars: I) -> BossEnvironmentState
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

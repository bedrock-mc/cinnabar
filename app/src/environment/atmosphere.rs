//! Publishes atmosphere and lighting from app-owned UI, settings, and camera inputs.

use bevy::{
    prelude::{Local, Query, Res, ResMut, Time, Transform, With},
    time::Real,
};
use launcher::menu::settings_options::SettingId;
use render_scene::AtmosphereFrame;

use client_presentation::environment::{
    EnvironmentContext, EnvironmentProfileRoute, WeatherState, WorldClock,
    atmosphere::{
        apply_boss_environment, derive_base_frame, derive_boss_environment_iter,
        derive_profiled_atmosphere_frame,
    },
    renderer_clock::RendererClock,
    visual_world_time,
    weather::{LightningFlashState, WeatherDisplay, WeatherTickFrame},
};
use client_shell::ui_resources::UiRuntimeResource;

use super::CameraMediumState;

type AtmosphereOutputs<'w> = (
    ResMut<'w, AtmosphereFrame>,
    ResMut<'w, EnvironmentProfileRoute>,
    ResMut<'w, render_scene::WorldLighting>,
    ResMut<'w, WeatherTickFrame>,
    ResMut<'w, render_scene::AtmosphereViewInputs>,
);

/// Menu options and optional sky settings consumed by atmosphere updates.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct AtmospherePreferences<'w> {
    #[cfg(not(target_arch = "wasm32"))]
    menu: Option<Res<'w, client_shell::menu::MenuRuntime>>,
    #[cfg(target_arch = "wasm32")]
    options: Res<'w, crate::browser_input::BrowserOptions>,
    clouds: Option<ResMut<'w, render_scene::CloudVisibility>>,
    skybox: Option<ResMut<'w, render_scene::SkyboxSettings>>,
}

/// The renderer-clock overrides, in [`super::rendered_clock`] precedence order.
type TimeOverrides<'w> = (
    Option<Res<'w, super::FixedTime>>,
    Option<Res<'w, super::VisualTimeOverride>>,
);

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_atmosphere_frame(
    clock: Res<WorldClock>,
    time_overrides: TimeOverrides,
    weather: Res<WeatherState>,
    medium: Res<CameraMediumState>,
    context: Res<EnvironmentContext>,
    boss_bars: Res<UiRuntimeResource>,
    atmosphere_assets: Res<render_scene::AtmosphereTextureAssets>,
    time: Res<Time<Real>>,
    flash: Res<LightningFlashState>,
    vision: (
        Res<client_presentation::camera::VisionEffects>,
        Option<Res<client_presentation::camera::ServerCameraView>>,
    ),
    outputs: AtmosphereOutputs,
    settings: Res<client_state::RuntimeSettings>,
    mut display: Local<WeatherDisplay>,
    mut renderer_clock: Local<RendererClock>,
    preferences: AtmospherePreferences,
    cameras: Query<&Transform, With<client_presentation::camera::FlyCamera>>,
) {
    let vision = vision
        .1
        .as_deref()
        .map_or(*vision.0, |camera| vision.0.for_camera(camera));
    let renderer_ticks = renderer_clock.advance(
        clock.server_time().map(|_| clock.session_generation()),
        time.elapsed_secs_f64(),
    );
    let (fixed_time, time_override) = time_overrides;
    let clock = super::rendered_clock(*clock, fixed_time.as_deref(), time_override.as_deref());
    #[cfg(not(target_arch = "wasm32"))]
    let options = preferences
        .menu
        .as_ref()
        .map(|menu| menu.settings_snapshot().0);
    #[cfg(target_arch = "wasm32")]
    let options = Some(std::sync::Arc::clone(&preferences.options.0));
    let clouds = preferences.clouds;
    let skybox = preferences.skybox;
    if let Some(mut clouds) = clouds {
        clouds.0 = options
            .as_ref()
            .is_none_or(|options| options.value(SettingId::RenderClouds) != 0);
    }
    if let Some(mut skybox) = skybox {
        skybox.visible = options
            .as_ref()
            .is_none_or(|options| options.value(SettingId::FancySkies) != 0)
            && !vision.sky_hidden;
        skybox.native_vertical_offset = options.as_ref().map_or_else(
            || settings.user_settings_update().1.video.native_skybox_offset,
            |options| {
                options.value(launcher::menu::settings_options::NATIVE_SKYBOX_OFFSET_OPTION.id) != 0
            },
        );
    }
    let darkness_scale = options.as_ref().map_or(1.0, |options| {
        options.value(SettingId::Darkness) as f32 / 100.0
    });
    let (mut frame, mut route, mut lighting, mut weather_ticks, mut view_inputs) = outputs;
    let elapsed = time.elapsed_secs_f64();
    display.set_precipitation_count(context.precipitation_sample_count);
    let shown = display.advance_in_dimension(*weather, elapsed, context.dimension);
    *view_inputs = render_scene::AtmosphereViewInputs {
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
            atmosphere_assets.identity(),
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
    lighting.0 = render_model::LightmapInputs {
        // Vanilla's ordinary renderer sets the lightmap's ambient flag, which
        // applies both ambient stages around gamma;
        // omitting it crushes shaded terrain at night, despite matching gamma.
        ambient_adjustment: true,
        sky_darken: render_model::lightmap_sky_darken(
            frame.celestial_angle(),
            display.fog_level(),
            shown.lightning_level(),
        ),
        sunrise,
        lightning: frame.lightning_flash() > 0.0,
        brightness: settings.user_settings_update().1.video.brightness,
        night_vision: vision.night_vision,
        darkness: vision.darkness * darkness_scale,
        darkness_pulse: render_model::darkness_pulse(
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

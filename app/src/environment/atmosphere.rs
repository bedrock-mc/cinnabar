//! Per-frame atmosphere derivation: clock and weather into one shared GPU
//! frame, overlaid with client fog profiles and explicit boss requests.

use assets::{BiomeVisualProfile, FogMedium, FogProfile, facts::dimension_fallback_biome};
use bevy::{
    prelude::{Local, Query, Res, ResMut, Time, Transform, With},
    time::Real,
};
use launcher::menu::settings_options::SettingId;
use meshing::CameraMedium;
use render_scene::{AtmosphereFrame, SkyKind};
use ui::BossBarView;

use client_shell::ui_resources::UiRuntimeResource;

use super::{
    CameraMediumState, EnvironmentContext, EnvironmentProfileRoute, LightningFlashState,
    WeatherDisplay, WeatherState, WorldClock, profile_lookup::find_biome_profile,
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

type AtmosphereOutputs<'w> = (
    ResMut<'w, AtmosphereFrame>,
    ResMut<'w, EnvironmentProfileRoute>,
    ResMut<'w, render_scene::WorldLighting>,
    ResMut<'w, super::WeatherTickFrame>,
    ResMut<'w, render_scene::AtmosphereViewInputs>,
);

/// Resolves sampled indices only in their owning registry, using camera names after a reload.
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

/// Menu options and optional sky settings consumed by atmosphere updates.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct AtmospherePreferences<'w> {
    #[cfg(not(target_arch = "wasm32"))]
    menu: Option<ResMut<'w, client_shell::menu::MenuRuntime>>,
    #[cfg(target_arch = "wasm32")]
    options: Res<'w, crate::browser_input::BrowserOptions>,
    clouds: Option<ResMut<'w, render_scene::CloudVisibility>>,
    skybox: Option<ResMut<'w, render_scene::SkyboxSettings>>,
    java: Option<ResMut<'w, render_scene::JavaWorldVisuals>>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_atmosphere_frame(
    clock: Res<WorldClock>,
    time_override: Option<Res<super::VisualTimeOverride>>,
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
    renderer_state: (
        Local<super::renderer_clock::RendererClock>,
        Local<Option<render_model::java_world::JavaVisualState>>,
    ),
    preferences: AtmospherePreferences,
    cameras: Query<&Transform, With<client_presentation::camera::FlyCamera>>,
) {
    let (mut renderer_clock, mut java_state) = renderer_state;
    let vision = vision
        .1
        .as_deref()
        .map_or(*vision.0, |camera| vision.0.for_camera(camera));
    let renderer_ticks = renderer_clock.advance(
        clock.server_time().map(|_| clock.session_generation),
        time.elapsed_secs_f64(),
    );
    let clock = time_override
        .as_ref()
        .map_or(*clock, |value| value.rendering_clock(*clock));
    let mut java = preferences.java;
    #[cfg(not(target_arch = "wasm32"))]
    let options = preferences.menu.map(|mut menu| {
        menu.sync_java_world_visuals_available(java.as_ref().is_some_and(|java| java.available()));
        menu.settings_snapshot().0
    });
    #[cfg(target_arch = "wasm32")]
    let options = Some(std::sync::Arc::clone(&preferences.options.0));
    let clouds = preferences.clouds;
    let skybox = preferences.skybox;
    if let Some(java) = java.as_mut() {
        java.set_enabled(options.as_ref().map_or_else(
            || settings.user_settings_update().1.video.java_world_visuals,
            |options| options.value(SettingId::JavaWorldVisuals) != 0,
        ));
    }
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
            shown.lightning_level,
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
    let java_assets = java.as_ref().and_then(|java| java.assets());
    let java_state = active_java_state(&mut java_state, java_assets.is_some());
    if let Some((java, java_state)) = java_assets.zip(java_state) {
        let identifier = context.camera_biome_identifier.as_deref();
        let environment = java.environment(
            context.dimension,
            identifier,
            visual_world_time(clock, elapsed),
        );
        let now_tick = boss_bars
            .estimated_server_tick(u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX));
        let fire_resistance = boss_bars.gameplay_hud().effects().iter().any(|effect| {
            assets::effect_descriptor(effect.effect_id)
                .is_some_and(|descriptor| descriptor.identifier == "minecraft:fire_resistance")
                && effect.visible_at_tick(now_tick)
        });
        let requested_distance = f32::from(
            settings
                .user_settings_update()
                .1
                .video
                .render_distance_chunks,
        ) * 16.0;
        let confirmed_distance = frame.cloud_fade_distance();
        let camera = render_model::java_world::JavaCameraInputs {
            render_distance: if confirmed_distance > 0.0 {
                requested_distance.min(confirmed_distance)
            } else {
                requested_distance
            },
            forward: view_inputs.forward,
            rain: shown.rain_level,
            thunder: shown.lightning_level,
            water: medium.0 == CameraMedium::Water,
            lava: medium.0 == CameraMedium::Lava,
            spectator: context.spectator,
            fire_resistance,
            brightness: lighting.0.brightness,
            night_vision: vision.night_vision,
            blindness: vision.blindness,
            fog_darkness: vision.darkness,
            darkness: vision.darkness * darkness_scale,
            darkness_pulse: lighting.0.darkness_pulse,
            sky_exposure: context
                .camera_sky_light
                .map_or(1.0, |sky| ((f32::from(sky) - 8.0) / 7.0).clamp(0.0, 1.0)),
            precipitation: identifier
                .and_then(|identifier| java.biomes.get(identifier))
                .is_none_or(|biome| biome.precipitation),
        };
        let visual = java_state.evaluate(
            clock.session_generation(),
            renderer_ticks,
            environment,
            camera,
        );
        lighting.0.java = Some(visual.lightmap);
        *frame = frame.with_java_world(
            visual,
            renderer_ticks,
            java.atmosphere_texture(assets::AtmosphereRole::Clouds)
                .width,
        );
    }
    *route = next_route;
}

/// Starts adaptation when the preset is active and discards state while it is disabled.
fn active_java_state(
    state: &mut Option<render_model::java_world::JavaVisualState>,
    enabled: bool,
) -> Option<&mut render_model::java_world::JavaVisualState> {
    if enabled {
        Some(state.get_or_insert_default())
    } else {
        *state = None;
        None
    }
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
mod java_state_tests {
    use super::active_java_state;
    use assets::java_world::JavaEnvironment;
    use render_model::java_world::JavaCameraInputs;

    #[test]
    fn disabled_preset_time_cannot_become_underwater_adaptation() {
        let environment = JavaEnvironment::default();
        let mut camera = JavaCameraInputs {
            render_distance: 256.0,
            forward: [0.0, 0.0, 1.0],
            rain: 0.0,
            thunder: 0.0,
            water: false,
            lava: false,
            spectator: false,
            fire_resistance: false,
            brightness: 0.0,
            night_vision: 0.0,
            blindness: 0.0,
            fog_darkness: 0.0,
            darkness: 0.0,
            darkness_pulse: 0.0,
            sky_exposure: 1.0,
            precipitation: true,
        };
        let mut state = None;
        active_java_state(&mut state, true)
            .unwrap()
            .evaluate(1, 0.0, environment, camera);
        assert!(active_java_state(&mut state, false).is_none());

        camera.water = true;
        let resumed =
            active_java_state(&mut state, true)
                .unwrap()
                .evaluate(1, 600.0, environment, camera);
        assert_eq!(resumed.water_vision, 0.0);
        let adapting =
            active_java_state(&mut state, true)
                .unwrap()
                .evaluate(1, 601.0, environment, camera);
        assert!((adapting.water_vision - 0.006).abs() < 1.0e-6);
        let adapted =
            active_java_state(&mut state, true)
                .unwrap()
                .evaluate(1, 1200.0, environment, camera);
        assert!((adapted.water_vision - 1.0).abs() < f32::EPSILON);
        assert!(active_java_state(&mut state, false).is_none());
        let restarted =
            active_java_state(&mut state, true)
                .unwrap()
                .evaluate(1, 1200.0, environment, camera);
        assert_eq!(restarted.water_vision, 0.0);
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

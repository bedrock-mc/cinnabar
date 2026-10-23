use super::*;
use crate::environment::{self, ClientPartEnvironment};
use client_presentation::environment::{EnvironmentContext, EnvironmentProfileRoute, WeatherState};

/// Runs both environment publishers with an authoritative clock and no renderer or network.
fn app() -> App {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    client_presentation::environment::replace_session(
        &mut clock,
        &mut weather,
        protocol::WorldEnvironmentBootstrap {
            initial_time: 6_000,
            day_cycle_lock_time: 6_000,
            daylight_cycle_enabled: true,
            weather_cycle_enabled: true,
            rain_level: 0.0,
            lightning_level: 0.0,
        },
        0.0,
    );
    client_presentation::environment::apply_environment_control(
        client_world::CommittedControlEvent::SetTime {
            sequence: 1,
            update: protocol::SetTimeEvent { time: 6_000 },
        },
        &mut clock,
        &mut weather,
        0.0,
    );
    let mut app = App::new();
    app.insert_resource(clock)
        .insert_resource(weather)
        .init_resource::<FixedTime>()
        .init_resource::<VisualTimeOverride>()
        .init_resource::<ClientPartEnvironment>()
        .init_resource::<environment::CameraMediumState>()
        .init_resource::<EnvironmentContext>()
        .init_resource::<EnvironmentProfileRoute>()
        .init_resource::<client_presentation::environment::weather::LightningFlashState>()
        .init_resource::<client_presentation::environment::weather::WeatherTickFrame>()
        .init_resource::<render_scene::AtmosphereTextureAssets>()
        .init_resource::<AtmosphereFrame>()
        .init_resource::<render_scene::AtmosphereViewInputs>()
        .init_resource::<WorldLighting>()
        .init_resource::<VisionEffects>()
        .init_resource::<client_state::RuntimeSettings>()
        .init_resource::<Time<Real>>()
        .init_resource::<ClientWorld>()
        .init_resource::<FrameTimeCounterPin>()
        .init_resource::<ShaderPackIdMaps>()
        .init_resource::<ShaderPackFrameInputs>()
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(1)))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .add_systems(
            Update,
            (update_atmosphere_frame, publish_frame_inputs).chain(),
        );
    app
}

#[test]
fn client_part_time_keeps_sky_and_shader_clock_in_step_through_override_changes() {
    let mut app = app();
    let authoritative = *app.world().resource::<WorldClock>();
    for (fixed, personal, client_part, ticks) in [
        (None, None, Some(18_000), 18_000),
        (None, Some(1_000), Some(18_000), 1_000),
        (Some(13_013), Some(1_000), Some(18_000), 13_013),
        (Some(13_013), None, Some(18_000), 13_013),
        (None, None, Some(18_000), 18_000),
        (None, None, None, 6_000),
    ] {
        app.world_mut().resource_mut::<FixedTime>().0 = fixed;
        app.world_mut().resource_mut::<VisualTimeOverride>().0 = personal;
        app.world_mut().resource_mut::<ClientPartEnvironment>().time = client_part;
        app.update();
        let frame = app.world().resource::<AtmosphereFrame>();
        let inputs = app.world().resource::<ShaderPackFrameInputs>();
        assert_eq!(inputs.world_time, ticks);
        assert_eq!(inputs.world_day, 0);
        assert_eq!(inputs.moon_phase, 0);
        assert_eq!(inputs.celestial_angle, frame.celestial_angle());
        assert_eq!(*app.world().resource::<WorldClock>(), authoritative);
    }
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(std::time::Duration::from_secs(10));
    app.update();
    let inputs = app.world().resource::<ShaderPackFrameInputs>();
    assert_eq!(inputs.world_time, 6_200);
    assert_eq!(
        inputs.celestial_angle,
        app.world().resource::<AtmosphereFrame>().celestial_angle()
    );
}

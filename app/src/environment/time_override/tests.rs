use super::*;
use crate::environment::{self, WeatherState};
use bevy::{prelude::*, time::Real};
use client_world::CommittedControlEvent;
use protocol::{DaylightCycleUpdateEvent, SetTimeEvent, WorldEnvironmentBootstrap};
use render::{AtmosphereFrame, WorldLighting};

/// Runs the real atmosphere publisher without rendering or a network session.
fn atmosphere_app(cycle: bool) -> App {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    environment::replace_session(
        &mut clock,
        &mut weather,
        WorldEnvironmentBootstrap {
            initial_time: 6_000,
            day_cycle_lock_time: 6_000,
            daylight_cycle_enabled: cycle,
            weather_cycle_enabled: true,
            rain_level: 0.0,
            lightning_level: 0.0,
        },
        0.0,
    );
    let mut app = App::new();
    app.insert_resource(clock)
        .insert_resource(weather)
        .init_resource::<VisualTimeOverride>()
        .init_resource::<environment::CameraMediumState>()
        .init_resource::<environment::EnvironmentContext>()
        .init_resource::<environment::EnvironmentProfileRoute>()
        .init_resource::<environment::LightningFlashState>()
        .init_resource::<environment::WeatherTickFrame>()
        .init_resource::<render::AtmosphereTextureAssets>()
        .init_resource::<AtmosphereFrame>()
        .init_resource::<render::AtmosphereViewInputs>()
        .init_resource::<WorldLighting>()
        .init_resource::<crate::camera::VisionEffects>()
        .init_resource::<crate::settings_runtime::RuntimeSettings>()
        .init_resource::<Time<Real>>()
        .insert_resource(client_ui::ui_runtime::UiRuntime::new(1))
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .add_systems(Update, environment::update_atmosphere_frame);
    app
}

/// Reads both published rendering inputs after one scheduled update.
fn output(app: &mut App) -> (AtmosphereFrame, WorldLighting) {
    app.update();
    (
        *app.world().resource::<AtmosphereFrame>(),
        *app.world().resource::<WorldLighting>(),
    )
}

#[test]
fn ordinary_world_publisher_preserves_native_nighttime_ambient_floor() {
    // Vanilla's ordinary renderer sets the lightmap's ambient flag, which
    // applies both ambient stages.
    let mut app = atmosphere_app(false);
    app.world_mut().resource_mut::<VisualTimeOverride>().0 = Some(18_000);
    let (_, lighting) = output(&mut app);
    assert!(lighting.0.ambient_adjustment);
    assert_eq!(lighting.0.brightness, 0.5);
    assert!((lighting.0.sky_darken - 0.2).abs() < 0.000_002);
    // Sky/block zero under the canopy: (.96*0+.03), gamma50%, then
    // the second .96*C+.03 stage. This is not a texture brightening factor.
    let floor = lighting.0.build()[0];
    for channel in &floor[..3] {
        assert!((channel - 0.099_459_45).abs() < 0.000_002, "{floor:?}");
    }
    app.world_mut().resource_mut::<VisualTimeOverride>().0 = Some(6_000);
    let (_, day) = output(&mut app);
    assert!(day.0.ambient_adjustment);
    assert!(day.0.build()[15 << 4][0] > 0.98);
}

#[test]
fn override_replaces_atmosphere_and_light_and_clearing_restores_server() {
    for cycle in [true, false] {
        let mut app = atmosphere_app(cycle);
        let baseline = output(&mut app);
        let server = *app.world().resource::<WorldClock>();
        app.world_mut().resource_mut::<VisualTimeOverride>().0 = Some(18_000);
        let fixed = output(&mut app);
        assert_ne!(fixed.0.sun_direction(), baseline.0.sun_direction());
        assert_ne!(fixed.0.sky_zenith(), baseline.0.sky_zenith());
        assert_ne!(fixed.0.fog_color(), baseline.0.fog_color());
        assert_eq!(
            fixed.0.cloud_texture_offset(),
            baseline.0.cloud_texture_offset()
        );
        assert_ne!(fixed.1.0.sky_darken, baseline.1.0.sky_darken);
        assert_eq!(
            fixed.1.0.sky_darken,
            render::lightmap_sky_darken(fixed.0.celestial_angle(), 0.0, 0.0)
        );
        assert_eq!(*app.world().resource::<WorldClock>(), server);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(10));
        let advancing_clouds = output(&mut app);
        assert_eq!(advancing_clouds.0.with_cloud_renderer_ticks(0.0), fixed.0);
        assert_eq!(advancing_clouds.1, fixed.1);
        assert_ne!(
            advancing_clouds.0.cloud_texture_offset(),
            fixed.0.cloud_texture_offset()
        );

        app.world_mut().resource_mut::<VisualTimeOverride>().0 = None;
        let advancing_server = output(&mut app);
        assert_eq!(
            advancing_server.0.day_fraction(),
            render::AtmosphereFrame::from_bedrock_time(
                environment::visual_world_time(server, 10.0),
                0.0,
                0.0
            )
            .day_fraction()
        );
        app.world_mut().resource_mut::<VisualTimeOverride>().0 = Some(18_000);
        assert_eq!(output(&mut app), advancing_clouds);
        let mut clock = *app.world().resource::<WorldClock>();
        let mut weather = *app.world().resource::<WeatherState>();
        environment::apply_environment_control(
            CommittedControlEvent::SetTime {
                sequence: 1,
                update: SetTimeEvent { time: 1_000 },
            },
            &mut clock,
            &mut weather,
            10.0,
        );
        environment::apply_environment_control(
            CommittedControlEvent::DaylightCycle {
                sequence: 2,
                update: DaylightCycleUpdateEvent { enabled: false },
            },
            &mut clock,
            &mut weather,
            12.0,
        );
        app.insert_resource(clock).insert_resource(weather);
        assert_eq!(output(&mut app), advancing_clouds);
        app.world_mut().resource_mut::<VisualTimeOverride>().0 = None;
        let restored = output(&mut app);
        let expected = render::AtmosphereFrame::from_bedrock_time(
            environment::visual_world_time(clock, 12.0),
            0.0,
            0.0,
        )
        .with_cloud_fade_distance(0.0)
        .with_liquid_render_distance(0.0);
        assert_eq!(restored.0.with_cloud_renderer_ticks(0.0), expected);
        assert_eq!(
            restored.1.0.sky_darken,
            render::lightmap_sky_darken(expected.celestial_angle(), 0.0, 0.0)
        );
        assert_eq!(*app.world().resource::<WorldClock>(), clock);
    }
}

//! Native server-day capture stays separate from the atmosphere's visual time override.

use super::*;
use crate::environment::VisualTimeOverride;
use client_presentation::environment::{self, WeatherState, WorldClock};

#[derive(Resource, Default)]
struct Observed(Option<mod_host::module_observations::WorldDay>);

#[derive(Resource)]
struct Scope(mod_host::WorldObservation);

/// Captures through the production system parameter without requiring a camera or focused gameplay.
fn capture(context: GameplayContext, scope: Res<Scope>, mut observed: ResMut<Observed>) {
    observed.0 = context.world_day(&scope.0);
}

/// An always-day visual override cannot change the server day or cross a transport-session boundary.
#[test]
fn native_day_capture_ignores_time_changer_and_rejects_stale_sessions() {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    environment::replace_session(
        &mut clock,
        &mut weather,
        protocol::WorldEnvironmentBootstrap {
            initial_time: 0,
            day_cycle_lock_time: world::TICKS_PER_DAY as i32 * 3,
            daylight_cycle_enabled: false,
            weather_cycle_enabled: true,
            rain_level: 0.,
            lightning_level: 0.,
        },
        0.,
    );
    environment::bind_session_generation(&mut clock, &mut weather, 7);
    let mut app = App::new();
    app.insert_resource(clock)
        .insert_resource(Time::<()>::default())
        .insert_resource(VisualTimeOverride(Some(6000)))
        .insert_resource(Scope(mod_host::WorldObservation {
            session: 7,
            dimension: 0,
            world: "fixture".into(),
            position: (0., 64., 0.),
            alive: true,
        }))
        .init_resource::<Observed>()
        .add_systems(Update, capture);
    app.update();
    assert_eq!(app.world().resource::<Observed>().0.unwrap().day, 3);
    assert_eq!(*app.world().resource::<WorldClock>(), clock);
    app.world_mut().resource_mut::<Scope>().0.session = 8;
    app.update();
    assert!(app.world().resource::<Observed>().0.is_none());
}

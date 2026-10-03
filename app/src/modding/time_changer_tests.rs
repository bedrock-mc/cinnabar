use super::*;
use crate::environment::{self, WeatherState, WorldClock};

#[test]
fn configured_time_changer_is_visual_only_offline() {
    let Some(path) = std::env::var_os(COMPONENT_ENV) else {
        eprintln!(
            "skipping configured_time_changer_is_visual_only_offline: fixture unavailable; requires installed local carriers (make assets) and CINNABAR_MOD_COMPONENT"
        );
        return;
    };
    let mut app = App::new();
    let Some(presentation) =
        crate::ui_runtime::presentation::forms::pack_harness::engine_presentation()
    else {
        return;
    };
    app.insert_resource(presentation)
        .insert_resource(UiRuntime::new(1))
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(ButtonInput::<KeyCode>::default());
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    configure(&mut app, Some(Path::new(&path)));
    assert_eq!(
        app.world().resource::<ModRuntime>().host.label(),
        Some("Time: Server")
    );
    let (network, mut packets) = crate::runtime::network::NetworkHandle::stub_capturing_packets();
    app.insert_resource(network);
    let mut clock = WorldClock::default();
    environment::replace_session(
        &mut clock,
        &mut WeatherState::default(),
        protocol::WorldEnvironmentBootstrap {
            initial_time: 6_000,
            day_cycle_lock_time: 6_000,
            daylight_cycle_enabled: true,
            rain_level: 0.0,
            lightning_level: 0.0,
        },
        0.0,
    );
    app.insert_resource(clock);
    app.update();
    assert_eq!(app.world().resource::<VisualTimeOverride>().0, None);
    let mut modes = Vec::new();
    loop {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset(DEMO_KEY);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(DEMO_KEY);
        app.update();
        let runtime = app.world().resource::<ModRuntime>();
        let label = runtime.host.label().unwrap().to_owned();
        assert_eq!(
            app.world().resource::<VisualTimeOverride>().0,
            runtime.host.time_override()
        );
        assert_eq!(*app.world().resource::<WorldClock>(), clock);
        assert!(packets.drain().is_empty());
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(DEMO_KEY)
        );
        if runtime.host.time_override().is_none() {
            assert_eq!(label, "Time: Server");
            break;
        }
        assert!(!modes.contains(&label), "cycle must return to server time");
        modes.push(label);
        assert!(modes.len() < 16, "cycle must be bounded");
    }
    assert_eq!(
        modes,
        ["Time: Day", "Time: Sunset", "Time: Night", "Time: Midnight"]
    );
}

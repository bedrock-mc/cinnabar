//! Focus policy changes must reach the wait in the frame that observes them.

use super::*;
use render_model::UNFOCUSED_FRAME_RATE;

#[derive(Resource)]
struct NextPacing(FramePacing);

/// Uses the application's background limit without duplicating its rate.
fn background() -> FramePacing {
    FramePacing {
        rate: Some(UNFOCUSED_FRAME_RATE),
        ..Default::default()
    }
}

/// Models the app's focus policy publication after its ordinary frame work.
fn publish_policy(next: Res<NextPacing>, mut pacing: ResMut<FramePacing>) {
    *pacing = next.0;
}

/// Exercises both frame-wait paths with policy changes after input was sampled.
fn policy_app(pipelined: bool, clock: Arc<FakeClock>) -> App {
    let mut app = App::new();
    app.insert_resource(background())
        .insert_resource(NextPacing(background()))
        .add_systems(Last, publish_policy);
    if pipelined {
        let mut extract = SubApp::new();
        extract.set_extract(|_, _| {});
        app.insert_sub_app(RenderExtractApp, extract);
    }
    app.add_plugins(InputPacingPlugin::with_pacer(InputPacer::new(clock, true)));
    app
}

#[test]
fn focus_restoration_uses_the_foreground_limit_in_the_same_frame() {
    for pipelined in [false, true] {
        for foreground in [FramePacing::default(), hz(120)] {
            let clock = FakeClock::new();
            let mut app = policy_app(pipelined, clock.clone());
            app.update();
            let resumed = clock.now();
            app.world_mut().resource_mut::<NextPacing>().0 = foreground;
            app.update();
            let period = foreground.rate.map_or(Duration::ZERO, |rate| {
                Duration::from_nanos(rate.period_nanos())
            });
            assert_eq!(
                clock.now() - resumed,
                period,
                "pipelined={pipelined}: retained the background wait after focus returned"
            );
        }
    }
}

#[test]
fn focus_loss_uses_the_background_limit_in_the_same_frame() {
    for pipelined in [false, true] {
        let clock = FakeClock::new();
        let mut app = policy_app(pipelined, clock.clone());
        app.world_mut().resource_mut::<NextPacing>().0 = FramePacing::default();
        app.update();
        app.world_mut().resource_mut::<NextPacing>().0 = background();
        let unfocused = clock.now();
        app.update();
        assert_eq!(
            clock.now() - unfocused,
            Duration::from_nanos(UNFOCUSED_FRAME_RATE.period_nanos()),
            "pipelined={pipelined}: delayed the background limit"
        );
    }
}

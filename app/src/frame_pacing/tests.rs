use std::num::NonZeroU16;

use bevy::prelude::{App, DetectChanges, IntoScheduleConfigs, MinimalPlugins, Update};
use render_model::{OCCLUDED_FRAME_RATE, UNFOCUSED_FRAME_RATE, VrrStatus};

use crate::present_mode::apply_present_mode;

use super::*;

fn hz(rate: u32) -> Option<FrameRate> {
    FrameRate::from_hz(rate)
}

fn fixed(fps: u16) -> FrameRateLimit {
    FrameRateLimit::Fixed(NonZeroU16::new(fps).unwrap())
}

fn pacing_app(runtime: FramePacingRuntime, limit: FrameRateLimit) -> (App, Entity) {
    pacing_app_with(runtime, limit, |_| {})
}

fn pacing_app_with(
    runtime: FramePacingRuntime,
    limit: FrameRateLimit,
    video: impl FnOnce(&mut ui::VideoSettings),
) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<WindowOccluded>()
        .init_resource::<RuntimeSettings>()
        .init_resource::<FramePacing>()
        .insert_resource(DisplayRefresh(DisplayTiming {
            refresh: FrameRate::from_hz(120),
            vrr: VrrStatus::Unknown,
        }))
        .insert_resource(PresentModeRuntime::from_startup(false, false, false, false))
        .insert_resource(runtime)
        .add_systems(Update, (apply_present_mode, update_frame_pacing).chain());
    let mut settings = ui::UserSettings::default();
    settings.video.frame_rate_limit = limit;
    video(&mut settings.video);
    app.world_mut()
        .resource_mut::<RuntimeSettings>()
        .replace_user_settings(settings);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Window::default()
            },
            PrimaryWindow,
        ))
        .id();
    app.update();
    (app, window)
}

fn pacing(app: &App) -> FramePacing {
    *app.world().resource::<FramePacing>()
}

fn set_focus(app: &mut App, window: Entity, focused: bool) {
    app.world_mut().get_mut::<Window>(window).unwrap().focused = focused;
    app.update();
}

fn occlude(app: &mut App, window: Entity, occluded: bool) {
    app.world_mut()
        .write_message(WindowOccluded { window, occluded });
    app.update();
}

#[test]
fn the_saved_cap_paces_a_focused_window_and_a_launch_cap_outranks_it() {
    let (app, _) = pacing_app(FramePacingRuntime::new(None, false), fixed(144));
    assert_eq!(
        pacing(&app),
        FramePacing {
            rate: hz(144),
            precise: true
        }
    );
    let (app, _) = pacing_app(FramePacingRuntime::new(Some(60), false), fixed(144));
    assert_eq!(pacing(&app).rate, hz(60));
    let (app, _) = pacing_app(
        FramePacingRuntime::new(None, false),
        FrameRateLimit::Unlimited,
    );
    assert_eq!(pacing(&app).rate, None);
}

#[test]
fn background_windows_slow_down_without_spinning_and_recover() {
    let (mut app, window) = pacing_app(
        FramePacingRuntime::new(None, false),
        FrameRateLimit::Unlimited,
    );
    set_focus(&mut app, window, false);
    assert_eq!(
        pacing(&app),
        FramePacing {
            rate: Some(UNFOCUSED_FRAME_RATE),
            precise: false
        }
    );
    occlude(&mut app, window, true);
    assert_eq!(pacing(&app).rate, Some(OCCLUDED_FRAME_RATE));
    occlude(&mut app, window, false);
    set_focus(&mut app, window, true);
    assert_eq!(
        pacing(&app),
        FramePacing {
            rate: None,
            precise: true
        }
    );
}

#[test]
fn hidden_and_acceptance_runs_keep_their_cadence_in_the_background() {
    let (mut app, window) = pacing_app(
        FramePacingRuntime::new(Some(240), true),
        FrameRateLimit::Unlimited,
    );
    set_focus(&mut app, window, false);
    occlude(&mut app, window, true);
    assert_eq!(
        pacing(&app),
        FramePacing {
            rate: hz(240),
            precise: true
        }
    );
}

#[test]
fn a_fixed_clock_recording_lifts_the_cadence_until_it_stops() {
    let (mut app, _) = pacing_app(
        FramePacingRuntime::new(Some(30), false),
        FrameRateLimit::Unlimited,
    );
    app.world_mut()
        .resource_mut::<FramePacingRuntime>()
        .set_suspended(true);
    app.update();
    assert_eq!(pacing(&app).rate, None);
    app.world_mut()
        .resource_mut::<FramePacingRuntime>()
        .set_suspended(false);
    app.update();
    assert_eq!(pacing(&app).rate, hz(30));
}

#[test]
fn an_unchanged_cadence_is_not_republished() {
    let (mut app, _) = pacing_app(FramePacingRuntime::new(None, false), fixed(120));
    app.world_mut().clear_trackers();
    app.update();
    assert!(!app.world().resource_ref::<FramePacing>().is_changed());
}

/// Automatic leaves synchronized and tear-free pacing to the display, and renders tearing
/// frames at twice the refresh rate.
#[test]
fn automatic_limits_follow_the_presentation_intent() {
    let (app, _) = pacing_app(
        FramePacingRuntime::new(None, false),
        FrameRateLimit::Automatic,
    );
    assert_eq!(pacing(&app).rate, None);
    let (app, _) = pacing_app_with(
        FramePacingRuntime::new(None, false),
        FrameRateLimit::Automatic,
        |video| video.vsync = false,
    );
    assert_eq!(pacing(&app).rate, None);
    let (app, _) = pacing_app_with(
        FramePacingRuntime::new(None, false),
        FrameRateLimit::Automatic,
        |video| {
            video.vsync = false;
            video.allow_tearing = true;
        },
    );
    assert_eq!(pacing(&app).rate, hz(240));
}

use std::num::NonZeroU16;

use bevy::prelude::{App, DetectChanges, IntoScheduleConfigs, MinimalPlugins, Update};
use render_model::{OCCLUDED_FRAME_RATE, UNFOCUSED_FRAME_RATE, VrrStatus};

use {crate::present_mode::apply_present_mode, client_state::RuntimeSettings};

use super::*;

fn hz(rate: u32) -> Option<FrameRate> {
    FrameRate::from_hz(rate)
}

fn fixed(fps: u16) -> FrameRateLimit {
    FrameRateLimit::Fixed(NonZeroU16::new(fps).unwrap())
}

/// `(launch cap, ignore window state)` for the session under test.
type Session = (Option<u32>, bool);

/// Supplies an empty diagnostic world without carriers, networking or server fixtures.
fn playing_world() -> crate::runtime::world::ClientWorld {
    crate::runtime::world::ClientWorld {
        stream: Some(chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        })),
        ..Default::default()
    }
}

fn pacing_app(session: Session, limit: FrameRateLimit) -> (App, Entity) {
    pacing_app_with(session, limit, |_| {})
}

fn pacing_app_with(
    (launch_cap, ignore_window_state): Session,
    limit: FrameRateLimit,
    video: impl FnOnce(&mut ui::VideoSettings),
) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<WindowOccluded>()
        .init_resource::<RuntimeSettings>()
        .init_resource::<FramePacing>()
        .init_resource::<PowerSource>()
        .insert_resource(playing_world())
        .insert_resource(DisplayRefresh(DisplayTiming {
            refresh: FrameRate::from_hz(120),
            vrr: VrrStatus::Unknown,
        }))
        .insert_resource(
            PresentModeRuntime::from_startup(false, false, false, false)
                .with_launch_frame_cap(launch_cap),
        )
        .insert_resource(FramePacingRuntime::new(ignore_window_state))
        .add_systems(Update, (apply_present_mode, update_frame_pacing).chain());
    let mut settings = ui::UserSettings::default();
    settings.video.frame_rate_limit = limit;
    settings.video.vsync = render_api::VsyncMode::On;
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
    let (app, _) = pacing_app((None, false), fixed(144));
    assert_eq!(
        pacing(&app),
        FramePacing {
            rate: hz(144),
            precise: true,
            display_sync: true,
        }
    );
    let (app, _) = pacing_app((Some(60), false), fixed(144));
    assert_eq!(pacing(&app).rate, hz(60));
    let (app, _) = pacing_app((None, false), FrameRateLimit::Unlimited);
    assert_eq!(pacing(&app).rate, None);
}

#[test]
fn background_windows_slow_down_without_spinning_and_recover() {
    let (mut app, window) = pacing_app((None, false), FrameRateLimit::Unlimited);
    set_focus(&mut app, window, false);
    assert_eq!(
        pacing(&app),
        FramePacing {
            rate: Some(UNFOCUSED_FRAME_RATE),
            precise: false,
            display_sync: true,
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
            precise: true,
            display_sync: true,
        }
    );
}

#[test]
fn hidden_and_acceptance_runs_keep_their_cadence_in_the_background() {
    let (mut app, window) = pacing_app((Some(240), true), FrameRateLimit::Unlimited);
    set_focus(&mut app, window, false);
    occlude(&mut app, window, true);
    assert_eq!(
        pacing(&app),
        FramePacing {
            rate: hz(240),
            precise: true,
            display_sync: true,
        }
    );
}

#[test]
fn a_fixed_clock_recording_lifts_the_cadence_until_it_stops() {
    let (mut app, _) = pacing_app((Some(30), false), FrameRateLimit::Unlimited);
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
    let (mut app, _) = pacing_app((None, false), fixed(120));
    app.world_mut().clear_trackers();
    app.update();
    assert!(!app.world().resource_ref::<FramePacing>().is_changed());
}

/// Unlimited adds no application cadence; presentation or rendering determines the rate.
#[test]
fn unlimited_limits_leave_pacing_to_the_display_or_rendering() {
    let (app, _) = pacing_app((None, false), FrameRateLimit::Unlimited);
    assert_eq!(pacing(&app).rate, None);
    let (app, _) = pacing_app_with((None, false), FrameRateLimit::Unlimited, |video| {
        video.vsync = false.into()
    });
    assert_eq!(pacing(&app).rate, None);
}

#[test]
fn leaving_a_world_caps_menus_and_hidden_runs_keep_the_requested_limit() {
    for hidden in [false, true] {
        let (mut app, _) = pacing_app((None, hidden), FrameRateLimit::Unlimited);
        app.insert_resource(crate::runtime::world::ClientWorld::default());
        app.update();
        let expected = if hidden {
            None
        } else {
            app.world().resource::<DisplayRefresh>().0.refresh
        };
        assert_eq!(pacing(&app).rate, expected);
        app.world_mut()
            .remove_resource::<crate::runtime::world::ClientWorld>();
        app.update();
        assert_eq!(pacing(&app).rate, expected);
    }
}

#[test]
fn changing_power_source_only_changes_an_untouched_gameplay_limit() {
    for limit in [
        FrameRateLimit::Automatic,
        FrameRateLimit::Unlimited,
        fixed(75),
    ] {
        let (mut app, _) = pacing_app((None, false), limit);
        let mains = pacing(&app).rate;
        app.world_mut().resource_mut::<PowerSource>().on_battery = true;
        app.update();
        assert_eq!(
            pacing(&app).rate,
            if limit == FrameRateLimit::Automatic {
                app.world().resource::<DisplayRefresh>().0.refresh
            } else {
                mains
            }
        );
        app.world_mut().resource_mut::<PowerSource>().on_battery = false;
        app.update();
        assert_eq!(pacing(&app).rate, mains);
    }
}

#[test]
fn a_disconnected_session_returns_to_the_menu_ceiling() {
    let (mut app, _) = pacing_app((None, false), FrameRateLimit::Unlimited);
    app.insert_resource(crate::runtime::shutdown::FatalError(Some(
        "disconnected".into(),
    )));
    app.update();
    assert_eq!(
        pacing(&app).rate,
        app.world().resource::<DisplayRefresh>().0.refresh
    );
}

#[test]
fn gpu_prediction_follows_the_selected_surface_when_vsync_changes() {
    use render_model::{PresentModeKind, SurfacePresentModes};

    let (mut app, _) = pacing_app((None, false), FrameRateLimit::Unlimited);
    let policy = app.world().resource::<PresentModeRuntime>().policy();
    policy.publish_capabilities(Some(
        SurfacePresentModes::FIFO_ONLY.with(PresentModeKind::Immediate),
    ));
    app.update();
    assert!(pacing(&app).display_sync);
    for vsync in [false, true, false] {
        let mut settings = ui::UserSettings::default();
        settings.video.vsync = vsync.into();
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .replace_user_settings(settings);
        app.update();
        assert_eq!(pacing(&app).display_sync, vsync);
    }
    policy.publish_capabilities(Some(SurfacePresentModes::FIFO_ONLY));
    app.update();
    assert!(
        pacing(&app).display_sync,
        "a FIFO-only surface still owns backpressure"
    );
}

/// Present mode and cadence must agree on one limit, the launch cap outranking the saved one.
#[test]
fn a_launch_cap_drives_both_the_present_mode_and_the_cadence() {
    use bevy::window::PresentMode;
    use render_model::{PresentModeKind, SurfacePresentModes};

    let mailbox = SurfacePresentModes::FIFO_ONLY.with(PresentModeKind::Mailbox);
    for (launch_cap, saved, mode, rate) in [
        (
            Some(144),
            FrameRateLimit::Unlimited,
            PresentMode::Mailbox,
            hz(144),
        ),
        (
            Some(30),
            FrameRateLimit::Unlimited,
            PresentMode::Mailbox,
            hz(30),
        ),
    ] {
        let (mut app, window) = pacing_app_with((launch_cap, false), saved, |video| {
            video.vsync = false.into()
        });
        app.insert_resource(DisplayRefresh(DisplayTiming {
            refresh: FrameRate::from_hz(60),
            vrr: VrrStatus::Unknown,
        }));
        app.world()
            .resource::<PresentModeRuntime>()
            .policy()
            .publish_capabilities(Some(mailbox));
        app.update();
        assert_eq!(
            app.world().get::<Window>(window).unwrap().present_mode,
            mode,
            "launch cap {launch_cap:?} over saved {saved:?}"
        );
        assert_eq!(pacing(&app).rate, rate, "launch cap {launch_cap:?}");
    }
}

#[test]
fn diagnostic_limits_survive_recording_and_unlimited_settings() {
    let (mut app, _) = pacing_app((Some(30), true), FrameRateLimit::Unlimited);
    let mut runtime = app.world_mut().resource_mut::<FramePacingRuntime>();
    runtime.require_limit(hz(30).unwrap());
    runtime.set_suspended(true);
    app.update();
    assert_eq!(pacing(&app).rate, hz(30));
}

#[test]
fn vrr_preference_controls_low_latency_pacing_for_each_reported_state() {
    use render_api::VrrPreference;
    for status in [VrrStatus::Active, VrrStatus::Inactive, VrrStatus::Unknown] {
        for preference in [
            VrrPreference::Automatic,
            VrrPreference::On,
            VrrPreference::Off,
        ] {
            let (mut app, _) = pacing_app_with((None, false), FrameRateLimit::Unlimited, |video| {
                video.vsync = false.into();
                video.vrr = preference;
            });
            app.world_mut().resource_mut::<DisplayRefresh>().0.vrr = status;
            app.update();
            let active = preference == VrrPreference::On
                || (preference == VrrPreference::Automatic && status == VrrStatus::Active);
            assert_eq!(
                pacing(&app).rate,
                if active { hz(116) } else { None },
                "{status:?} {preference:?}"
            );
        }
    }
}

#[test]
fn missing_primary_display_clears_stale_vrr_and_refresh() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(DisplayRefresh(DisplayTiming {
            refresh: hz(120),
            vrr: VrrStatus::Active,
        }))
        .init_resource::<PowerSource>()
        .add_systems(Update, track_display_refresh);
    app.update();
    assert_eq!(
        app.world().resource::<DisplayRefresh>().0,
        DisplayTiming::default()
    );
}

/// Native refresh demand follows the same state and cap as frame admission.
fn display_link_rate(app: &App, window: Entity) -> Option<FrameRate> {
    app.world()
        .resource::<FramePacingRuntime>()
        .display_link_rate(
            app.world().resource::<PresentModeRuntime>().intent(),
            app.world().resource::<DisplayRefresh>().0,
            {
                let state = app.world().get::<Window>(window).unwrap();
                state.focused && state.visible
            },
            pacing(app),
        )
}

#[test]
fn synchronized_native_refresh_requests_the_reported_rate_without_a_sixty_hz_ceiling() {
    let (mut app, window) = pacing_app((None, false), FrameRateLimit::Unlimited);
    for rate in [60, 120, 144, 240] {
        app.world_mut().resource_mut::<DisplayRefresh>().0.refresh = hz(rate);
        app.update();
        assert_eq!(display_link_rate(&app, window), hz(rate));
        assert_eq!(pacing(&app).rate, None, "FIFO owns display pacing");
    }
    app.world_mut().resource_mut::<DisplayRefresh>().0.refresh = None;
    app.update();
    assert_eq!(display_link_rate(&app, window), None);
}

#[test]
fn native_refresh_respects_saved_launch_and_required_limits() {
    for (launch, saved, expected) in [
        (None, fixed(60), 60),
        (Some(30), fixed(60), 30),
        (Some(240), fixed(60), 120),
    ] {
        let (mut app, window) = pacing_app((launch, false), saved);
        assert_eq!(display_link_rate(&app, window), hz(expected));
        app.world_mut()
            .resource_mut::<FramePacingRuntime>()
            .require_limit(hz(20).unwrap());
        app.update();
        assert_eq!(display_link_rate(&app, window), hz(20));
    }
}

#[test]
fn native_refresh_releases_background_demand_and_restores_it_on_focus() {
    let (mut app, window) = pacing_app((None, false), FrameRateLimit::Unlimited);
    assert_eq!(display_link_rate(&app, window), hz(120));
    set_focus(&mut app, window, false);
    assert_eq!(display_link_rate(&app, window), None);
    set_focus(&mut app, window, true);
    occlude(&mut app, window, true);
    assert_eq!(display_link_rate(&app, window), None);
    occlude(&mut app, window, false);
    assert_eq!(display_link_rate(&app, window), hz(120));
}

#[test]
fn native_refresh_does_not_drive_unpaced_or_fixed_clock_surfaces() {
    let (app, window) = pacing_app_with((None, false), FrameRateLimit::Unlimited, |video| {
        video.vsync = false.into();
    });
    assert_eq!(display_link_rate(&app, window), None);
    let (mut app, window) = pacing_app((None, false), FrameRateLimit::Unlimited);
    app.world_mut().get_mut::<Window>(window).unwrap().visible = false;
    app.update();
    assert_eq!(display_link_rate(&app, window), None);
    let (app, window) = pacing_app((None, true), FrameRateLimit::Unlimited);
    assert_eq!(display_link_rate(&app, window), None);
    let (mut app, window) = pacing_app((None, false), FrameRateLimit::Unlimited);
    app.world_mut()
        .resource_mut::<FramePacingRuntime>()
        .set_suspended(true);
    app.update();
    assert_eq!(display_link_rate(&app, window), None);
    app.world_mut()
        .resource_mut::<FramePacingRuntime>()
        .set_suspended(false);
    app.update();
    assert_eq!(display_link_rate(&app, window), hz(120));
}

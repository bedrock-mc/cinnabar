use bevy::prelude::{App, DetectChanges, MinimalPlugins};
use render_model::{PresentModeKind, SurfacePresentModes};

use super::*;

const METAL: SurfacePresentModes = SurfacePresentModes::FIFO_ONLY.with(PresentModeKind::Immediate);
const MAILBOX_ONLY: SurfacePresentModes =
    SurfacePresentModes::FIFO_ONLY.with(PresentModeKind::Mailbox);

#[test]
fn attributable_runs_and_explicit_flags_lock_the_requested_policy() {
    let automatic = PresentModeRuntime::from_startup(false, false, false, false);
    assert!(!automatic.locked());
    assert_eq!(automatic.policy.preference(), PresentModePreference::Auto);
    assert_eq!(automatic.window_present_mode(), PresentMode::Fifo);

    for (runtime, expected_preference, startup_mode) in [
        (
            PresentModeRuntime::from_startup(true, false, false, false),
            PresentModePreference::Vsync,
            PresentMode::Fifo,
        ),
        (
            PresentModeRuntime::from_startup(false, true, false, false),
            PresentModePreference::NoVsync,
            PresentMode::Fifo,
        ),
        (
            PresentModeRuntime::from_startup(false, false, true, false),
            PresentModePreference::Vsync,
            PresentMode::Fifo,
        ),
    ] {
        assert!(runtime.locked());
        assert_eq!(runtime.policy.preference(), expected_preference);
        assert_eq!(runtime.window_present_mode(), startup_mode);
    }
}

/// Startup and runtime share one decision: a probed surface only changes which mode is chosen.
#[test]
fn startup_flag_and_runtime_setting_select_the_same_mode_once_probed() {
    for supported in [METAL, MAILBOX_ONLY, SurfacePresentModes::FIFO_ONLY] {
        let flag = PresentModeRuntime::from_startup(false, true, false, false);
        flag.policy.publish_capabilities(Some(supported));

        let mut app = vsync_app(PresentModeRuntime::from_startup(false, false, false, false));
        publish_capabilities(&app, supported);
        publish_vsync(&mut app, false);
        assert_eq!(primary_present_mode(&mut app), flag.window_present_mode());
    }
}

fn vsync_app(runtime: PresentModeRuntime) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<RuntimeSettings>()
        .insert_resource(runtime)
        .add_systems(bevy::prelude::Update, apply_present_mode);
    spawn_primary_window(&mut app);
    app
}

fn spawn_primary_window(app: &mut App) {
    let window = Window {
        present_mode: PresentMode::Fifo,
        ..Window::default()
    };
    app.world_mut().spawn((window, PrimaryWindow));
}

fn publish_vsync(app: &mut App, vsync: bool) {
    publish_video(app, |video| video.vsync = vsync);
}

fn publish_video(app: &mut App, edit: impl FnOnce(&mut ui::VideoSettings)) {
    let mut settings = ui::UserSettings::default();
    edit(&mut settings.video);
    app.world_mut()
        .resource_mut::<RuntimeSettings>()
        .replace_user_settings(settings);
    app.update();
}

fn publish_capabilities(app: &App, supported: SurfacePresentModes) {
    app.world()
        .resource::<PresentModeRuntime>()
        .policy
        .publish_capabilities(Some(supported));
}

fn preference(app: &App) -> PresentModePreference {
    app.world()
        .resource::<PresentModeRuntime>()
        .policy
        .preference()
}

fn primary_present_mode(app: &mut App) -> PresentMode {
    let world = app.world_mut();
    let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
    windows.single(world).unwrap().present_mode
}

#[test]
fn toggling_the_user_setting_switches_the_present_mode_live() {
    let mut app = vsync_app(PresentModeRuntime::from_startup(false, false, false, false));

    publish_vsync(&mut app, false);
    assert_eq!(preference(&app), PresentModePreference::NoVsync);
    assert_eq!(primary_present_mode(&mut app), PresentMode::Fifo);

    publish_vsync(&mut app, true);
    assert_eq!(preference(&app), PresentModePreference::Auto);
    assert_eq!(primary_present_mode(&mut app), PresentMode::Fifo);
}

/// The pre-probe FIFO request is replaced by Mailbox once the surface offers it for a limit
/// above the display's refresh.
#[test]
fn a_completed_probe_moves_an_outpacing_limit_to_mailbox() {
    let mut app = vsync_app(PresentModeRuntime::from_startup(false, false, false, false));
    publish_video(&mut app, |video| {
        video.vsync = false;
        video.frame_rate_limit = render_api::FrameRateLimit::Unlimited;
    });
    assert_eq!(primary_present_mode(&mut app), PresentMode::Fifo);

    publish_capabilities(&app, MAILBOX_ONLY);
    app.update();
    assert_eq!(primary_present_mode(&mut app), PresentMode::Mailbox);
}

/// `--no-vsync` and the VSync setting mean the same tear-free path; nothing a player can set
/// requests Immediate.
#[test]
fn no_launch_flag_or_setting_requests_immediate() {
    for (no_vsync, vsync) in [(true, true), (false, false), (false, true)] {
        let mut app = vsync_app(PresentModeRuntime::from_startup(
            false, no_vsync, false, false,
        ));
        publish_capabilities(&app, METAL);
        for limit in [
            render_api::FrameRateLimit::Automatic,
            render_api::FrameRateLimit::Unlimited,
        ] {
            publish_video(&mut app, |video| {
                video.vsync = vsync;
                video.frame_rate_limit = limit;
            });
            assert_ne!(primary_present_mode(&mut app), PresentMode::Immediate);
        }
    }
}

/// An unrelated settings revision must not drop an applied driver remedy.
#[test]
fn vsync_on_keeps_the_automatic_driver_remedy() {
    let runtime = PresentModeRuntime::from_startup(false, false, false, false);
    let render_policy = runtime.policy();
    let mut app = vsync_app(runtime);
    render_policy.publish_remedy(PresentModeRemedy::UseImmediate);
    app.update();

    publish_vsync(&mut app, true);
    assert_eq!(preference(&app), PresentModePreference::Auto);
    assert_eq!(render_policy.remedy(), PresentModeRemedy::UseImmediate);
    assert_eq!(primary_present_mode(&mut app), PresentMode::Immediate);
}

#[test]
fn launch_flags_override_the_user_setting() {
    for (force_vsync, no_vsync, setting, effective) in
        [(true, false, false, true), (false, true, true, false)]
    {
        let runtime = PresentModeRuntime::from_startup(force_vsync, no_vsync, false, false);
        assert_eq!(runtime.vsync_override(), Some(effective));
        let startup_mode = runtime.window_present_mode();
        let startup_preference = runtime.policy.preference();
        let mut app = vsync_app(runtime);

        publish_vsync(&mut app, setting);
        assert_eq!(preference(&app), startup_preference);
        assert_eq!(primary_present_mode(&mut app), startup_mode);
    }
    assert_eq!(
        PresentModeRuntime::from_startup(false, false, false, false).vsync_override(),
        None
    );
}

#[test]
fn locked_acceptance_policy_ignores_runtime_setting_replacements() {
    let mut app = vsync_app(PresentModeRuntime::from_startup(false, false, true, false));
    publish_vsync(&mut app, false);
    assert_eq!(preference(&app), PresentModePreference::Vsync);
    assert_eq!(primary_present_mode(&mut app), PresentMode::Fifo);
}

/// A hidden surface never waits for a display, yet the menu keeps the saved VSync choice.
#[test]
fn a_hidden_surface_presents_without_display_pacing_and_leaves_the_setting_editable() {
    let runtime = PresentModeRuntime::from_startup(false, false, false, true);
    assert_eq!(runtime.vsync_override(), None);
    let mut app = vsync_app(runtime);
    publish_capabilities(&app, METAL);
    publish_vsync(&mut app, true);
    assert_eq!(preference(&app), PresentModePreference::Auto);
    assert_eq!(primary_present_mode(&mut app), PresentMode::Immediate);
}

#[test]
fn a_setting_update_retries_until_the_primary_window_exists() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<RuntimeSettings>()
        .insert_resource(PresentModeRuntime::from_startup(false, false, false, false))
        .add_systems(bevy::prelude::Update, apply_present_mode);
    publish_vsync(&mut app, false);

    let runtime = app.world().resource::<PresentModeRuntime>();
    assert_eq!(runtime.observed_settings_generation(), 0);
    assert_eq!(runtime.policy.preference(), PresentModePreference::Auto);

    spawn_primary_window(&mut app);
    app.update();

    let runtime = app.world().resource::<PresentModeRuntime>();
    assert_eq!(runtime.observed_settings_generation(), 1);
    assert_eq!(runtime.policy.preference(), PresentModePreference::NoVsync);
    assert_eq!(primary_present_mode(&mut app), PresentMode::Fifo);
}

#[test]
fn a_stable_mode_never_requests_another_surface_reconfigure() {
    let runtime = PresentModeRuntime::from_startup(false, false, false, false);
    let render_policy = runtime.policy();
    let mut app = vsync_app(runtime);
    render_policy.publish_remedy(PresentModeRemedy::UseImmediate);
    app.update();
    assert_eq!(primary_present_mode(&mut app), PresentMode::Immediate);

    app.world_mut().clear_trackers();
    app.update();
    let world = app.world_mut();
    let mut windows = world.query_filtered::<bevy::prelude::Ref<Window>, With<PrimaryWindow>>();
    let window = windows.single(world).unwrap();
    assert_eq!(window.present_mode, PresentMode::Immediate);
    assert!(
        !window.is_changed(),
        "a stable mode must not request another surface reconfigure"
    );
}

#[test]
fn present_mode_transition_reports_whether_it_changed_the_window() {
    let mut window = Window::default();
    assert!(set_present_mode_if_changed(
        &mut window,
        PresentMode::Immediate
    ));
    assert!(!set_present_mode_if_changed(
        &mut window,
        PresentMode::Immediate
    ));
}

/// Mirrors the render world's driver check for the window's current request.
fn publish_driver_recommendation(app: &mut App, supported: SurfacePresentModes) {
    let requested = primary_present_mode(app);
    let runtime = app.world().resource::<PresentModeRuntime>();
    runtime
        .policy
        .publish_remedy(render::resolve_dx12_present_mode_remedy(
            runtime.policy.preference(),
            wgpu::Backend::Dx12,
            "Radeon RX 570 Series",
            "31.0.21924.61",
            requested,
            supported,
        ));
}

/// The render world withdraws its recommendation once Immediate is requested; the adopted
/// remedy must hold instead of bouncing the surface between FIFO and Immediate.
#[test]
fn an_adopted_driver_remedy_stays_applied_through_the_render_feedback_loop() {
    let mut app = vsync_app(PresentModeRuntime::from_startup(false, false, false, false));
    publish_capabilities(&app, METAL);
    let mut changes = 0;
    let mut previous = primary_present_mode(&mut app);
    for _ in 0..120 {
        publish_driver_recommendation(&mut app, METAL);
        app.update();
        let current = primary_present_mode(&mut app);
        if current != previous {
            changes += 1;
            previous = current;
        }
    }
    assert_eq!(previous, PresentMode::Immediate);
    assert_eq!(changes, 1, "the surface reconfigured {changes} times");

    publish_vsync(&mut app, false);
    assert_eq!(
        app.world().resource::<PresentModeRuntime>().remedy_adopted,
        None,
        "a preference change drops the adopted remedy"
    );
}

/// Diagnostics wait for this selection, so it appears only once capabilities are known.
#[test]
fn the_capability_selected_mode_is_published_with_the_window_request() {
    let mut app = vsync_app(PresentModeRuntime::from_startup(false, true, false, false));
    publish_video(&mut app, |video| {
        video.frame_rate_limit = render_api::FrameRateLimit::Unlimited;
    });
    let policy = app.world().resource::<PresentModeRuntime>().policy();
    assert_eq!(policy.selection(), None);

    publish_capabilities(&app, MAILBOX_ONLY);
    app.update();
    assert_eq!(policy.selection(), Some(PresentModeKind::Mailbox));
    assert_eq!(primary_present_mode(&mut app), PresentMode::Mailbox);
}

/// VSync off stays tear-free on a surface without Mailbox, and Mailbox only serves a limit
/// that outpaces the display.
#[test]
fn vsync_off_never_requests_immediate() {
    let mut app = vsync_app(PresentModeRuntime::from_startup(false, false, false, false));
    publish_capabilities(&app, METAL);
    for limit in [
        render_api::FrameRateLimit::Automatic,
        render_api::FrameRateLimit::Unlimited,
    ] {
        publish_video(&mut app, |video| {
            video.vsync = false;
            video.frame_rate_limit = limit;
        });
        assert_eq!(
            primary_present_mode(&mut app),
            PresentMode::Fifo,
            "{limit:?}"
        );
    }
    publish_capabilities(&app, MAILBOX_ONLY);
    publish_video(&mut app, |video| {
        video.vsync = false;
        video.frame_rate_limit = render_api::FrameRateLimit::Unlimited;
    });
    assert_eq!(primary_present_mode(&mut app), PresentMode::Mailbox);
    publish_video(&mut app, |video| video.vsync = false);
    assert_eq!(primary_present_mode(&mut app), PresentMode::Fifo);
}

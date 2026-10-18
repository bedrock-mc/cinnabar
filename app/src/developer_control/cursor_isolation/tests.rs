//! CPU-only checks of the window state presented to the native output schedule.

use bevy::{
    input::{InputPlugin, mouse::AccumulatedMouseMotion},
    prelude::*,
    window::{
        CursorGrabMode, CursorMoved, CursorOptions, MonitorSelection, PrimaryWindow, WindowFocused,
        WindowMode,
    },
};
use client_presentation::local_player::LocalViewPose;
use client_state::DrivenInput;
use developer_control::protocol::InputCommand;

use crate::developer_control::input;

#[derive(Resource, Default)]
struct NativeFacing {
    cursor_updates: usize,
    cursors: Vec<CursorOptions>,
    windows: Vec<Window>,
}

/// Observes the components and change filters consumed by Bevy's native output systems.
fn observe_native_facing_state(
    windows: Query<&Window, Changed<Window>>,
    cursors: Query<&CursorOptions, Changed<CursorOptions>>,
    mut observed: ResMut<NativeFacing>,
) {
    observed.windows.extend(windows.iter().cloned());
    observed.cursor_updates += cursors.iter().count();
    observed.cursors.extend(cursors.iter().cloned());
}

/// Builds only ECS and input plugins; it has no native event loop or windows.
fn harness() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(InputPlugin)
        .add_message::<WindowFocused>()
        .add_message::<CursorMoved>()
        .init_resource::<LocalViewPose>()
        .init_resource::<NativeFacing>();
    input::configure(&mut app);
    super::configure(&mut app, false);
    app.add_systems(Last, observe_native_facing_state);
    let mut window = Window {
        focused: false,
        ..default()
    };
    window.set_cursor_position(Some(Vec2::new(31.0, 47.0)));
    let entity = app
        .world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow))
        .id();
    app.update();
    *app.world_mut().resource_mut::<NativeFacing>() = NativeFacing::default();
    (app, entity)
}

#[derive(Resource, Default)]
struct UiSample(Vec<Option<Vec2>>);

/// Exercises the production chat cursor transition and an unrelated window update.
fn close_chat_and_resize(
    mut windows: Query<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    mut motion: ResMut<AccumulatedMouseMotion>,
    mut sampled: ResMut<UiSample>,
) {
    let (mut window, mut cursor) = windows.single_mut().unwrap();
    sampled.0.push(window.cursor_position());
    crate::ui_runtime::interaction::input_capture::restore_gameplay_input_after_chat(
        &mut cursor,
        &mut keys,
        &mut buttons,
        &mut motion,
    );
    window.title = "resized while driven".into();
    window.resolution.set(1024.0, 768.0);
    window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
}

#[test]
fn synthetic_pointer_and_chat_changes_do_not_reach_native_cursor_output() {
    let (mut app, entity) = harness();
    app.init_resource::<UiSample>()
        .add_systems(Update, close_chat_and_resize)
        .add_systems(
            PostUpdate,
            |mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>| {
                client_shell::physical_input::release_cursor(&mut cursors.single_mut().unwrap());
            },
        );
    input::apply(
        app.world_mut(),
        &InputCommand {
            cursor: Some([123.0, 234.0]),
            ..default()
        },
    )
    .unwrap();
    app.update();
    assert_eq!(
        app.world().resource::<UiSample>().0,
        [Some(Vec2::new(123.0, 234.0))],
        "UI must receive the synthetic hit-test position"
    );
    let observed = app.world().resource::<NativeFacing>();
    assert_eq!(
        observed.cursor_updates, 0,
        "synthetic chat grab reached the native filter"
    );
    let window = observed
        .windows
        .last()
        .expect("resize must reach native output");
    assert_eq!(window.cursor_position(), Some(Vec2::new(31.0, 47.0)));
    assert!(!window.focused);
    assert_eq!(window.title, "resized while driven");
    assert_eq!(window.width(), 1024.0);
    assert_eq!(window.height(), 768.0);
    assert!(matches!(window.mode, WindowMode::BorderlessFullscreen(_)));
    assert_eq!(
        app.world().get::<Window>(entity).unwrap().cursor_position(),
        window.cursor_position()
    );
    app.update();
    assert_eq!(
        app.world().resource::<UiSample>().0[1],
        Some(Vec2::new(123.0, 234.0))
    );
    assert_eq!(app.world().resource::<NativeFacing>().cursor_updates, 0);
}

/// Requests capture through the same cursor component as gameplay and screen transitions.
fn request_capture(mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    let mut cursor = cursors.single_mut().unwrap();
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
}

#[test]
fn driven_control_releases_existing_native_capture_once() {
    for (grab_mode, visible) in [
        (CursorGrabMode::Locked, false),
        (CursorGrabMode::Confined, true),
        (CursorGrabMode::None, false),
    ] {
        let (mut app, entity) = harness();
        app.world_mut().get_mut::<Window>(entity).unwrap().focused = true;
        *app.world_mut().get_mut::<CursorOptions>(entity).unwrap() = CursorOptions {
            grab_mode,
            visible,
            hit_test: false,
        };
        app.update();
        let observed = app.world().resource::<NativeFacing>();
        assert_eq!(observed.cursor_updates, 1);
        assert_eq!(observed.cursors[0].grab_mode, grab_mode);
        assert_eq!(observed.cursors[0].visible, visible);
        *app.world_mut().resource_mut::<NativeFacing>() = NativeFacing::default();

        // A late UI writer must not undo the release at the native frame boundary.
        app.add_systems(PostUpdate, request_capture);
        input::apply(
            app.world_mut(),
            &InputCommand {
                cursor: Some([123.0, 234.0]),
                press: vec!["Escape".into()],
                ..default()
            },
        )
        .unwrap();
        app.update();
        let observed = app.world().resource::<NativeFacing>();
        assert_eq!(observed.cursor_updates, 1);
        let released = &observed.cursors[0];
        assert_eq!(released.grab_mode, CursorGrabMode::None);
        assert!(released.visible);
        assert!(!released.hit_test);
        let window = app.world().get::<Window>(entity).unwrap();
        assert!(window.focused);
        assert_eq!(window.cursor_position(), Some(Vec2::new(31.0, 47.0)));

        app.update();
        assert_eq!(app.world().resource::<NativeFacing>().cursor_updates, 1);
        let cursor = app.world().get::<CursorOptions>(entity).unwrap();
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
        assert!(cursor.visible);
        assert!(!cursor.hit_test);
    }
}

#[test]
fn hidden_cursor_output_stays_isolated_after_control_is_released() {
    let (mut app, entity) = harness();
    // Explicit configuration keeps this test independent of process environment.
    app.world_mut().insert_resource(super::Isolation::new(true));
    app.add_systems(Update, request_capture);
    input::apply(
        app.world_mut(),
        &InputCommand {
            cursor: Some([91.0, 73.0]),
            ..default()
        },
    )
    .unwrap();
    app.update();
    input::apply(
        app.world_mut(),
        &InputCommand {
            release_control: true,
            ..default()
        },
    )
    .unwrap();
    app.update();
    assert!(!app.world().contains_resource::<DrivenInput>());
    assert_eq!(app.world().resource::<NativeFacing>().cursor_updates, 0);
    let window = app.world().get::<Window>(entity).unwrap();
    assert_eq!(window.cursor_position(), Some(Vec2::new(31.0, 47.0)));
    assert!(!window.focused);
}

#[test]
fn desktop_cursor_requests_and_real_pointer_survive_control_handoff() {
    let (mut app, entity) = harness();
    app.world_mut()
        .insert_resource(super::Isolation::new(false));
    input::apply(
        app.world_mut(),
        &InputCommand {
            cursor: Some([91.0, 73.0]),
            hold: vec!["KeyW".into()],
            ..default()
        },
    )
    .unwrap();
    app.update();
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    {
        let mut window = app.world_mut().get_mut::<Window>(entity).unwrap();
        window
            .bypass_change_detection()
            .set_cursor_position(Some(Vec2::new(17.0, 19.0)));
    }
    input::apply(
        app.world_mut(),
        &InputCommand {
            release_control: true,
            ..default()
        },
    )
    .unwrap();
    app.add_systems(Update, request_capture);
    app.update();
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    assert_eq!(
        app.world().get::<Window>(entity).unwrap().cursor_position(),
        Some(Vec2::new(17.0, 19.0))
    );
    assert!(!app.world().get::<Window>(entity).unwrap().focused);
    let cursor = app.world().get::<CursorOptions>(entity).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
    assert!(!cursor.visible);
    assert_eq!(app.world().resource::<NativeFacing>().cursor_updates, 1);
}

#[test]
fn emote_open_and_close_keep_synthetic_input_inside_the_application() {
    use crate::ui_runtime::emotes::drive_emote_input;
    use client_shell::ui_resources::UiRuntimeResource;
    use client_state::EmoteInputConsumed;
    use client_state::PlayerRuntime;

    let (mut app, entity) = harness();
    let mut runtime = client_ui::ui_runtime::UiRuntime::new(1);
    let mut player = PlayerRuntime(player_state::PlayerState::new(1));
    runtime
        .publish_local_runtime_id(&mut player, 1, 42)
        .unwrap();
    app.init_resource::<Time<Real>>()
        .init_resource::<EmoteInputConsumed>()
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(player)
        .add_systems(Update, drive_emote_input);
    for (key, open) in [("KeyB", true), ("Escape", false)] {
        input::apply(
            app.world_mut(),
            &InputCommand {
                cursor: Some([123.0, 234.0]),
                press: vec![key.into()],
                ..default()
            },
        )
        .unwrap();
        app.update();
        assert_eq!(
            app.world()
                .resource::<UiRuntimeResource>()
                .emotes()
                .is_open(),
            open
        );
        assert!(app.world().resource::<EmoteInputConsumed>().0);
        assert_eq!(app.world().resource::<NativeFacing>().cursor_updates, 0);
        assert_eq!(
            app.world().get::<Window>(entity).unwrap().cursor_position(),
            Some(Vec2::new(31.0, 47.0))
        );
    }
}

#[test]
fn hidden_new_window_does_not_enter_the_native_cursor_change_filter() {
    let mut app = App::new();
    super::configure(&mut app, true);
    app.init_resource::<NativeFacing>()
        .add_systems(Last, observe_native_facing_state);
    let entity = app
        .world_mut()
        .spawn((
            Window {
                focused: false,
                visible: false,
                ..default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    app.update();
    assert_eq!(app.world().resource::<NativeFacing>().cursor_updates, 0);
    let cursor = app.world().get::<CursorOptions>(entity).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    assert!(cursor.visible);
}

#[test]
fn physical_cursor_precision_and_other_internal_requests_survive_virtual_input() {
    use bevy::math::{CompassOctant, DVec2};
    let (mut app, entity) = harness();
    let internal = {
        let mut window = app.world_mut().get_mut::<Window>(entity).unwrap();
        // This cursor is outside the window and is deliberately not representable as f32.
        window.set_physical_cursor_position(Some(DVec2::new(-0.0000000123456789, 9999.123456789)));
        window.internal
    };
    input::apply(
        app.world_mut(),
        &InputCommand {
            cursor: Some([123.0, 234.0]),
            ..default()
        },
    )
    .unwrap();
    app.add_systems(
        Update,
        |mut windows: Query<&mut Window, With<PrimaryWindow>>| {
            let mut window = windows.single_mut().unwrap();
            window.set_maximized(true);
            window.set_minimized(false);
            window.start_drag_move();
            window.start_drag_resize(CompassOctant::SouthEast);
        },
    );
    app.update();
    let mut restored = app.world().get::<Window>(entity).unwrap().internal;
    assert_eq!(restored.take_maximize_request(), Some(true));
    assert_eq!(restored.take_minimize_request(), Some(false));
    assert!(restored.take_move_request());
    assert_eq!(
        restored.take_resize_request(),
        Some(CompassOctant::SouthEast)
    );
    assert_eq!(
        restored, internal,
        "raw physical cursor state must be restored exactly"
    );
    assert_eq!(app.world().resource::<NativeFacing>().cursor_updates, 0);
}

#[test]
fn synthetic_pointer_click_activates_the_production_menu_hit() {
    use client_shell::{
        menu::{MenuClipboard, MenuRuntime, drive_menu_input},
        ui_resources::{UiPresentationResource, UiRuntimeResource},
    };
    use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
    use launcher::menu::{MenuAction, MenuField};

    let (mut app, entity) = harness();
    let player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let runtime = UiRuntime::new(1);
    let mut menu = MenuRuntime::new(true, 2, "test".into());
    menu.activate(MenuAction::PlayAddServer);
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation.set_menu_view(Some(menu.view()));
    presentation
        .build(
            &player,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let point = (0..720)
        .step_by(8)
        .flat_map(|y| {
            (0..1280)
                .step_by(8)
                .map(move |x| ui::UiPoint::new(x as f32, y as f32).unwrap())
        })
        .find(|point| presentation.hit_test_menu(*point) == Some(MenuAction::AddAddress))
        .unwrap();
    app.insert_resource(player)
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(UiPresentationResource(presentation))
        .insert_resource(menu)
        .insert_resource(MenuClipboard::with_access(|_| None, |_| {}))
        .add_systems(Update, drive_menu_input);
    input::apply(
        app.world_mut(),
        &InputCommand {
            cursor: Some([point.x(), point.y()]),
            press: vec!["MouseLeft".into()],
            ..default()
        },
    )
    .unwrap();
    app.update();
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().field,
        Some(MenuField::Address)
    );
    assert_eq!(app.world().resource::<NativeFacing>().cursor_updates, 0);
    assert_eq!(
        app.world().get::<Window>(entity).unwrap().cursor_position(),
        Some(Vec2::new(31.0, 47.0))
    );
}

#[test]
fn virtual_pointer_outside_the_viewport_survives_a_later_resize() {
    let (mut app, entity) = harness();
    app.init_resource::<UiSample>().add_systems(
        Update,
        |mut windows: Query<&mut Window, With<PrimaryWindow>>, mut sampled: ResMut<UiSample>| {
            let mut window = windows.single_mut().unwrap();
            if !sampled.0.is_empty() {
                window.resolution.set(1600.0, 900.0);
            }
            sampled.0.push(window.cursor_position());
        },
    );
    input::apply(
        app.world_mut(),
        &InputCommand {
            cursor: Some([1400.0, 100.0]),
            ..default()
        },
    )
    .unwrap();
    app.update();
    assert_eq!(app.world().resource::<UiSample>().0, [None]);
    app.update();
    assert_eq!(
        app.world().resource::<UiSample>().0[1],
        Some(Vec2::new(1400.0, 100.0)),
        "resizing must expose the retained virtual pointer to UI hit testing"
    );
    assert_eq!(
        app.world().get::<Window>(entity).unwrap().cursor_position(),
        Some(Vec2::new(31.0, 47.0))
    );
    assert_eq!(app.world().resource::<NativeFacing>().cursor_updates, 0);
}

//! Native input coverage for the settings-owned touch-control draft.
use super::*;
use bevy::{
    input::touch::{TouchInput, TouchPhase, touch_screen_input_system},
    prelude::*,
    window::WindowResolution,
};
use client_ui::test_support::pack_harness;
use launcher::menu::{MenuAction, MenuScreen};

const SIZE: [u32; 2] = [1280, 720];

/// Creates the real menu input system around an anonymous, carrier-backed editor.
fn fixture() -> Option<(App, Entity)> {
    let Some(presentation) = pack_harness::engine_presentation() else {
        eprintln!("skipping touch_editor input tests: missing installed UI carrier (make assets)");
        return None;
    };
    let mut menu = MenuRuntime::new(false, 2, "Player".into());
    menu.touch_available = true;
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::EditTouchControls);
    assert!(menu.touch_editor_active());
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .add_message::<TouchInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(menu)
        .insert_resource(UiPresentationResource(presentation))
        .add_systems(PreUpdate, touch_screen_input_system)
        .add_systems(Update, drive_menu_input);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                resolution: WindowResolution::new(SIZE[0], SIZE[1]),
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    app.update();
    paint(&mut app);
    Some((app, window))
}
/// Publishes the controller view and paints the production touch editor after each input batch.
fn paint(app: &mut App) {
    let view = app.world().resource::<MenuRuntime>().view();
    app.world_mut()
        .resource_scope(|world, mut p: Mut<UiPresentationResource>| {
            p.set_menu_view(Some(view));
            p.build(
                world.resource::<client_state::PlayerRuntime>(),
                &pack_harness::menu_runtime(),
                1000,
                SIZE,
                ui::DpiScale::new(1.).unwrap(),
            )
            .unwrap();
        });
}
/// Sends one finger through Bevy's touch event system before the menu observes it.
fn touch(app: &mut App, window: Entity, id: u64, phase: TouchPhase, at: [f32; 2]) {
    app.world_mut().write_message(TouchInput {
        window,
        phase,
        position: Vec2::from_array(at),
        force: None,
        id,
    });
    app.update();
    paint(app);
}
/// Reads a control's actual editor bounds rather than duplicating its authored coordinates.
fn bounds(app: &App, name: &str) -> [f32; 4] {
    app.world()
        .resource::<UiPresentationResource>()
        .touch_editor_bounds(name)
        .unwrap()
}
/// Returns the center of a painted rectangle.
fn center([x, y, w, h]: [f32; 4]) -> [f32; 2] {
    [x + w / 2., y + h / 2.]
}

#[test]
fn touch_editor_second_finger_cannot_steal_drag_and_cancellation_rolls_it_back() {
    let Some((mut app, window)) = fixture() else {
        return;
    };
    let original = bounds(&app, "jump");
    let at = center(original);
    touch(&mut app, window, 1, TouchPhase::Started, at);
    let other = center(bounds(&app, "joystick"));
    touch(&mut app, window, 2, TouchPhase::Started, other);
    touch(
        &mut app,
        window,
        1,
        TouchPhase::Moved,
        [at[0] - 160., at[1] - 80.],
    );
    assert!(bounds(&app, "jump")[0] < original[0] - 100.);
    touch(
        &mut app,
        window,
        1,
        TouchPhase::Canceled,
        [at[0] - 160., at[1] - 80.],
    );
    assert_eq!(bounds(&app, "jump"), original);
    touch(&mut app, window, 2, TouchPhase::Ended, other);
    assert!(app.world().resource::<MenuRuntime>().touch_editor_active());
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .touch_layout()
            .position("jump")
            .is_none()
    );
}
#[test]
fn touch_editor_save_button_commits_only_the_completed_draft() {
    let Some((mut app, window)) = fixture() else {
        return;
    };
    let at = center(bounds(&app, "chat"));
    touch(&mut app, window, 1, TouchPhase::Started, at);
    touch(
        &mut app,
        window,
        1,
        TouchPhase::Moved,
        [at[0] + 140., at[1] + 80.],
    );
    touch(
        &mut app,
        window,
        1,
        TouchPhase::Ended,
        [at[0] + 140., at[1] + 80.],
    );
    let save = center(
        app.world()
            .resource::<UiPresentationResource>()
            .touch_editor_action_bounds("hud.save")
            .unwrap(),
    );
    touch(&mut app, window, 3, TouchPhase::Started, save);
    assert!(app.world().resource::<MenuRuntime>().touch_editor_active());
    touch(&mut app, window, 3, TouchPhase::Ended, save);
    let menu = app.world().resource::<MenuRuntime>();
    assert!(!menu.touch_editor_active());
    assert!(
        menu.settings_options
            .touch_layout()
            .position("chat")
            .is_some()
    );
    assert!(menu.settings_dirty);
    assert_eq!(menu.screen(), MenuScreen::Settings);
}

#[test]
fn touch_editor_toolbar_swipe_does_not_save_or_close_the_draft() {
    let Some((mut app, window)) = fixture() else {
        return;
    };
    let save = center(
        app.world()
            .resource::<UiPresentationResource>()
            .touch_editor_action_bounds("hud.save")
            .unwrap(),
    );
    touch(&mut app, window, 1, TouchPhase::Started, save);
    touch(
        &mut app,
        window,
        1,
        TouchPhase::Moved,
        [save[0], save[1] - 60.],
    );
    touch(&mut app, window, 1, TouchPhase::Ended, save);
    assert!(app.world().resource::<MenuRuntime>().touch_editor_active());
    assert!(!app.world().resource::<MenuRuntime>().settings_dirty);
}

/// Physical mouse hold state survives the menu's per-frame button consumption.
#[test]
fn touch_editor_mouse_drag_survives_frames_without_repeated_button_events() {
    let Some((mut app, window)) = fixture() else {
        return;
    };
    let original = bounds(&app, "jump");
    let at = center(original);
    app.world_mut()
        .entity_mut(window)
        .get_mut::<Window>()
        .unwrap()
        .set_cursor_position(Some(Vec2::from_array(at)));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().write_message(MouseButtonInput {
        window,
        button: MouseButton::Left,
        state: ButtonState::Pressed,
    });
    app.update();
    paint(&mut app);
    let moved = [at[0] - 150., at[1] - 100.];
    app.world_mut()
        .entity_mut(window)
        .get_mut::<Window>()
        .unwrap()
        .set_cursor_position(Some(Vec2::from_array(moved)));
    app.update();
    paint(&mut app);
    assert!(bounds(&app, "jump")[0] < original[0] - 100.);
    app.world_mut().write_message(MouseButtonInput {
        window,
        button: MouseButton::Left,
        state: ButtonState::Released,
    });
    app.update();
    paint(&mut app);
    assert!(bounds(&app, "jump")[0] < original[0] - 100.);
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .touch_layout()
            .position("jump")
            .is_none()
    );
}

/// The final native release position, even without Moved, decides whether a toolbar tap commits.
#[test]
fn touch_editor_release_outside_toolbar_without_move_does_not_activate() {
    let Some((mut app, window)) = fixture() else {
        return;
    };
    let at = center(bounds(&app, "jump"));
    touch(&mut app, window, 1, TouchPhase::Started, at);
    touch(
        &mut app,
        window,
        1,
        TouchPhase::Ended,
        [at[0] - 140., at[1] - 80.],
    );
    let edited = bounds(&app, "jump");
    for action in ["hud.save", "hud.cancel", "hud.reset"] {
        let at = center(
            app.world()
                .resource::<UiPresentationResource>()
                .touch_editor_action_bounds(action)
                .unwrap(),
        );
        touch(&mut app, window, 2, TouchPhase::Started, at);
        touch(&mut app, window, 2, TouchPhase::Ended, [0., 0.]);
        assert!(
            app.world().resource::<MenuRuntime>().touch_editor_active(),
            "{action}"
        );
        assert_eq!(bounds(&app, "jump"), edited, "{action}");
        assert!(!app.world().resource::<MenuRuntime>().settings_dirty);
    }
}

/// A frame containing a complete gesture still delivers its release-only displacement.
#[test]
fn touch_editor_same_frame_gesture_uses_actual_release_coordinates() {
    let Some((mut app, window)) = fixture() else {
        return;
    };
    let original = bounds(&app, "jump");
    let at = center(original);
    for (phase, at) in [
        (TouchPhase::Started, at),
        (TouchPhase::Ended, [at[0] - 140., at[1] - 80.]),
    ] {
        app.world_mut().write_message(TouchInput {
            window,
            phase,
            position: Vec2::from_array(at),
            force: None,
            id: 1,
        });
    }
    app.update();
    paint(&mut app);
    assert!(bounds(&app, "jump")[0] < original[0] - 100.);
}

/// Touchscreen users can undo and redo one completed drag without a hardware keyboard.
#[test]
fn touch_editor_touch_undo_and_redo_restore_completed_gestures() {
    let Some((mut app, window)) = fixture() else {
        return;
    };
    let original = bounds(&app, "jump");
    let at = center(original);
    let undo = center(
        app.world()
            .resource::<UiPresentationResource>()
            .touch_editor_action_bounds("hud.undo")
            .unwrap(),
    );
    assert!(
        app.world()
            .resource::<UiPresentationResource>()
            .touch_editor_action_at(undo)
            .is_none()
    );
    touch(&mut app, window, 1, TouchPhase::Started, at);
    touch(
        &mut app,
        window,
        1,
        TouchPhase::Ended,
        [at[0] - 140., at[1] - 80.],
    );
    let edited = bounds(&app, "jump");
    assert!(edited[0] < original[0] - 100.);
    for (action, expected) in [("hud.undo", original), ("hud.redo", edited)] {
        let at = center(
            app.world()
                .resource::<UiPresentationResource>()
                .touch_editor_action_bounds(action)
                .unwrap(),
        );
        assert_eq!(
            app.world()
                .resource::<UiPresentationResource>()
                .touch_editor_action_at(at)
                .as_deref(),
            Some(action)
        );
        touch(&mut app, window, 2, TouchPhase::Started, at);
        touch(&mut app, window, 2, TouchPhase::Ended, at);
        assert_eq!(bounds(&app, "jump"), expected, "{action}");
    }
}

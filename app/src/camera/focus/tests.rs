use bevy::window::{CursorGrabMode, WindowEvent};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use client_ui::ui_runtime::UiRuntime;
use {super::*, client_presentation::camera::AutoFly};

use launcher::menu::settings_options::BindingId;
/// Builds the production focus/capture boundary without a windowing plugin or OS input.
pub(super) fn focus_app() -> (App, Entity) {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(AutoFly::new(false))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(UiRuntimeResource(UiRuntime::new(1)))
        .init_resource::<client_state::ScreenFocus>()
        .add_systems(
            Update,
            (
                client_shell::menu::screen_focus::publish_screen_focus,
                update_cursor_capture,
            )
                .chain(),
        );
    install(&mut app);
    let window = app
        .world_mut()
        .spawn((
            Window::default(),
            CursorOptions {
                grab_mode: CursorGrabMode::Locked,
                visible: false,
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    (app, window)
}

/// Checks both the OS request and the gameplay capture observation.
fn assert_released(app: &App, window: Entity) {
    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    assert!(cursor.visible);
    assert!(!client_presentation::camera::input_is_active(
        app.world().get::<Window>(window).unwrap(),
        cursor
    ));
}

#[test]
fn transient_overlay_focus_loss_beats_hud_capture_and_same_frame_click() {
    let (mut app, window) = focus_app();
    app.update();
    app.world_mut().write_message(WindowFocused {
        window,
        focused: false,
    });
    app.world_mut().write_message(WindowFocused {
        window,
        focused: true,
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::new(20.0, 10.0);
    app.update();
    assert_released(&app, window);
    assert_eq!(
        app.world().resource::<AccumulatedMouseMotion>().delta,
        Vec2::ZERO
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    app.update();
    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
    assert!(!cursor.visible);
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
    );
}

#[test]
fn occlusion_releases_until_unoccluded_and_ignores_other_windows() {
    let (mut app, window) = focus_app();
    let secondary = app.world_mut().spawn(Window::default()).id();
    app.world_mut().write_message(WindowFocused {
        window: secondary,
        focused: false,
    });
    app.world_mut().write_message(WindowOccluded {
        window: secondary,
        occluded: true,
    });
    app.update();
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );
    app.world_mut().write_message(WindowOccluded {
        window,
        occluded: true,
    });
    app.update();
    assert_released(&app, window);
    app.world_mut().write_message(WindowFocused {
        window,
        focused: true,
    });
    app.update();
    assert_released(&app, window);
    app.world_mut().write_message(WindowOccluded {
        window,
        occluded: false,
    });
    app.update();
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );
}

/// Simulates a screen adapter restoring a previously captured cursor.
fn restore_screen_capture(mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    for mut cursor in &mut cursors {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
}

#[test]
fn late_screen_capture_resumes_after_loss_but_not_during_driven_control() {
    let (mut app, window) = focus_app();
    app.add_systems(Update, restore_screen_capture.after(update_cursor_capture));
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    assert_released(&app, window);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );
    app.init_resource::<DrivenInput>();
    app.update();
    assert_released(&app, window);
}

#[test]
fn json_ui_form_release_restores_capture_after_overlay_and_response_delivery() {
    use bevy::input::{ButtonState, InputPlugin, mouse::MouseButtonInput};
    use client_ui::ui_runtime::flush_form_response;
    use {
        crate::ui_runtime::drive_server_form_input,
        client_ui::test_support::{mini_engine_presentation, pack_harness},
    };

    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let runtime = pack_harness::action_form(&mut player, "Menu", &["A", "B"]);
    let mut presentation = mini_engine_presentation();
    let (physical, dpi) = ([1280, 720], ui::DpiScale::new(1.0).unwrap());
    presentation
        .build(&player, &runtime, 0, physical, dpi)
        .unwrap();
    let identity = runtime.server_forms().active().unwrap().identity;
    let frame = presentation.form_engine_frame(identity).unwrap();
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.collection_index == Some(1))
        .unwrap();
    let centre = Vec2::new(
        frame.origin[0] + (hit.rect.x + hit.rect.w / 2.0) as f32 * frame.scale,
        frame.origin[1] + (hit.rect.y + hit.rect.h / 2.0) as f32 * frame.scale,
    );
    let mut app = App::new();
    app.add_plugins(InputPlugin)
        .init_resource::<client_state::ScreenFocus>()
        .insert_resource(AutoFly::new(false))
        .insert_resource(player)
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(UiPresentationResource(presentation))
        .add_systems(
            Update,
            (
                drive_server_form_input,
                client_shell::menu::screen_focus::publish_screen_focus,
                update_cursor_capture,
            )
                .chain(),
        );
    install(&mut app);
    let mut window = Window {
        focused: false,
        ..default()
    };
    window
        .resolution
        .set_physical_resolution(physical[0], physical[1]);
    window.set_cursor_position(Some(centre));
    let window = app
        .world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow))
        .id();
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    assert_released(&app, window);
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    app.update();
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .server_forms()
            .active()
            .is_some()
    );
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    });
    app.update();
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .server_forms()
            .active()
            .is_none()
    );
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .server_forms()
            .owns_input()
    );
    assert_released(&app, window);
    flush_form_response(
        &mut app.world_mut().resource_mut::<UiRuntimeResource>(),
        |_| false,
        |_| Ok(()),
    )
    .unwrap();
    app.update();
    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
    assert!(!cursor.visible);
}

#[test]
fn discarded_pointer_batch_cannot_authorize_a_later_programmatic_close() {
    use bevy::input::{ButtonState, mouse::MouseButtonInput};
    let (mut app, window) = focus_app();
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player);
    app.insert_resource(player)
        .insert_resource(UiRuntimeResource(runtime));
    app.add_systems(
        PreUpdate,
        (move |mut focused: MessageWriter<WindowFocused>,
               mut pointer: MessageWriter<MouseButtonInput>,
               mut emitted: Local<bool>| {
            if !*emitted {
                *emitted = true;
                for focused_value in [false, true] {
                    focused.write(WindowFocused {
                        window,
                        focused: focused_value,
                    });
                }
                for state in [ButtonState::Pressed, ButtonState::Released] {
                    pointer.write(MouseButtonInput {
                        button: MouseButton::Left,
                        state,
                        window,
                    });
                }
            }
        })
        .before(track_focus),
    );
    app.update();
    assert_released(&app, window);
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .close_chat();
    app.update();
    assert_released(&app, window);
}

#[test]
fn credits_keyboard_skip_retains_return_until_completion_delivery() {
    use crate::ui_runtime::interaction::drive_chat_keyboard_input;
    use bevy::{input::keyboard::KeyboardInput, time::Real};
    let (mut app, window) = focus_app();
    app.init_resource::<Time<Real>>()
        .add_message::<KeyboardInput>()
        .add_systems(
            Update,
            drive_chat_keyboard_input
                .before(client_shell::menu::screen_focus::publish_screen_focus),
        );
    {
        let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
        assert!(runtime.credits_mut().open(7, 1, 0));
        runtime.credits_mut().select(0, false);
    }
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .credits()
            .active()
            .is_none()
    );
    assert_released(&app, window);
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .credits_mut()
        .flush(Some(7), |_| Ok(()))
        .unwrap();
    app.update();
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );
}

#[test]
fn leave_bed_retains_return_until_server_wakes_player() {
    use crate::ui_runtime::interaction::drive_chat_ui_actions;
    use bevy::time::Real;
    use client_ui::{test_support::fixture_hud, ui_runtime::presentation::UiPresentationRuntime};
    use ui_engine::test_support::fixture_font;
    let (mut app, window) = focus_app();
    let player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = UiRuntime::new(1);
    runtime.set_local_sleeping(true);
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation.hud_frame_mut().sleep.observe(true, 1_000);
    presentation
        .build(
            &player,
            &runtime,
            3_000,
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
        .find(|point| presentation.hit_test_bed(*point) == Some(oreui::BedHit::LeaveBed))
        .unwrap();
    let centre = Vec2::new(point.x(), point.y());
    app.insert_resource(player)
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(UiPresentationResource(presentation))
        .init_resource::<Time<Real>>()
        .init_resource::<Touches>()
        .init_resource::<client_presentation::local_player::LocalPlayerFrameCarrier>()
        .init_resource::<client_presentation::local_player::InteractionOriginSnapshot>()
        .init_resource::<client_state::SemanticInputSnapshot>()
        .init_resource::<crate::runtime::world::ClientWorld>()
        .add_systems(
            Update,
            drive_chat_ui_actions.before(client_shell::menu::screen_focus::publish_screen_focus),
        );
    {
        let mut win = app.world_mut().get_mut::<Window>(window).unwrap();
        win.resolution.set_physical_resolution(1280, 720);
        win.set_cursor_position(Some(centre));
        win.focused = false;
    }
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert_released(&app, window);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .reset_all();
    assert!(
        app.world_mut()
            .resource_mut::<UiRuntimeResource>()
            .flush_wake_request(Some(1), |_| Ok::<_, ()>(()))
    );
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .set_local_sleeping(false);
    app.update();
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );
}

#[test]
fn touch_resume_returns_capture_after_focus_loss() {
    use bevy::input::{
        InputPlugin,
        touch::{TouchInput, TouchPhase},
    };
    use client_ui::ui_runtime::presentation::UiPresentationRuntime;
    use ui_engine::test_support::fixture_font;
    use {
        client_shell::menu::{MenuClipboard, MenuRuntime, drive_menu_input},
        launcher::menu::MenuAction,
    };
    let (mut app, window) = focus_app();
    app.add_plugins(InputPlugin);
    let player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let runtime = UiRuntime::new(1);
    let mut menu = MenuRuntime::new(false, 2, "test".into());
    menu.open_pause();
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
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
        .find(|point| presentation.hit_test_menu(*point) == Some(MenuAction::PauseResume))
        .unwrap();
    app.insert_resource(player)
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(menu)
        .insert_resource(UiPresentationResource(presentation))
        .insert_resource(MenuClipboard::with_access(|_| None, |_| {}))
        .add_systems(
            Update,
            drive_menu_input.before(client_shell::menu::screen_focus::publish_screen_focus),
        );
    {
        let mut win = app.world_mut().get_mut::<Window>(window).unwrap();
        win.resolution.set_physical_resolution(1280, 720);
        win.focused = false;
    }
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    // Menu controls activate on a completed tap, not on touch-down.
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        app.world_mut().write_message(TouchInput {
            phase,
            position: Vec2::new(point.x(), point.y()),
            window,
            force: None,
            id: 1,
        });
        app.update();
    }
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );
}

/// Sends a virtual Escape edge through the keyboard adapter without OS input.
fn press_escape(app: &mut App, window: Entity) {
    use bevy::input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
    };
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Escape,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    app.update();
}

#[test]
fn keyboard_wake_retains_return_until_server_acknowledgment() {
    use crate::ui_runtime::interaction::drive_chat_keyboard_input;
    use bevy::{input::keyboard::KeyboardInput, time::Real};
    let (mut app, window) = focus_app();
    app.init_resource::<Time<Real>>()
        .add_message::<KeyboardInput>()
        .add_systems(
            Update,
            drive_chat_keyboard_input
                .before(client_shell::menu::screen_focus::publish_screen_focus),
        );
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .set_local_sleeping(true);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    press_escape(&mut app, window);
    assert_released(&app, window);
    assert!(
        app.world_mut()
            .resource_mut::<UiRuntimeResource>()
            .flush_wake_request(Some(1), |_| Ok::<_, ()>(()))
    );
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .set_local_sleeping(false);
    app.update();
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );
}

#[test]
fn sign_finish_retains_return_until_transport_accepts_edit() {
    use crate::ui_runtime::drive_sign_editor;
    use bevy::input::keyboard::KeyboardInput;
    use client_ui::ui_runtime::{presentation::UiPresentationRuntime, sign_editor::SignEdit};
    use ui_engine::test_support::fixture_font;
    let (mut app, window) = focus_app();
    app.add_message::<KeyboardInput>()
        .init_resource::<crate::runtime::world::ClientWorld>()
        .insert_resource(UiPresentationResource(
            UiPresentationRuntime::new(fixture_font()).unwrap(),
        ))
        .add_systems(
            Update,
            drive_sign_editor.before(client_shell::menu::screen_focus::publish_screen_focus),
        );
    let mut edit = SignEdit::new([0, 0, 0], true, Default::default());
    assert!(edit.insert('a', |_| true));
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .sign_editor_mut()
        .open(edit);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    press_escape(&mut app, window);
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .sign_editor()
            .is_open()
    );
    assert_released(&app, window);
    assert!(
        app.world_mut()
            .resource_mut::<UiRuntimeResource>()
            .sign_editor_mut()
            .finish(|_| Ok(()))
    );
    app.update();
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );
}

#[test]
fn remapped_side_button_inventory_dismissal_returns_capture() {
    use bevy::{input::keyboard::KeyboardInput, time::Real};
    use client_ui::ui_runtime::presentation::UiPresentationRuntime;
    use ui_engine::test_support::fixture_font;
    use {
        crate::ui_runtime::interaction::drive_chat_keyboard_input,
        client_shell::menu::{MenuClipboard, MenuRuntime, drive_menu_input},
        launcher::menu::{
            MenuAction,
            settings_options::{EXTRA_KEYS, KEY_BINDINGS},
        },
    };
    let (mut app, window) = focus_app();
    let mut menu = MenuRuntime::new(false, 2, "test".into());
    menu.set_visible(true);
    let row = KEY_BINDINGS.len()
        + EXTRA_KEYS
            .iter()
            .position(|(name, _)| *name == "key.inventory")
            .unwrap();
    menu.activate(MenuAction::SettingsKey(row as u16));
    app.init_resource::<Time<Real>>()
        .init_resource::<Touches>()
        .add_message::<KeyboardInput>()
        .insert_resource(menu)
        .insert_resource(MenuClipboard::with_access(|_| None, |_| {}))
        .insert_resource(UiPresentationResource(
            UiPresentationRuntime::new(fixture_font()).unwrap(),
        ))
        .add_systems(Update, drive_chat_keyboard_input.before(drive_menu_input))
        .add_systems(
            Update,
            drive_menu_input.before(client_shell::menu::screen_focus::publish_screen_focus),
        );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Back);
    app.update();
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        assert_eq!(
            menu.settings_snapshot()
                .0
                .named_key_control(BindingId::Inventory),
            Some(semantic_input::PhysicalControl::MouseButton(
                client_shell::physical_input::mouse_button_code(MouseButton::Back).unwrap()
            ))
        );
        menu.set_visible(false);
    }
    app.world_mut()
        .resource_scope(|world, mut player: Mut<client_state::PlayerRuntime>| {
            let mut runtime = world.resource_mut::<UiRuntimeResource>();
            runtime.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
            runtime
                .publish_local_runtime_id(&mut player, 1, 42)
                .unwrap();
            runtime.toggle_inventory(&mut player, None);
        });
    assert!(app.world().resource::<UiRuntimeResource>().inventory_open());
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Back);
    app.update();
    assert!(!app.world().resource::<UiRuntimeResource>().inventory_open());
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );
}

#[test]
fn explicit_join_keeps_return_authorization_across_loading() {
    use bevy::input::keyboard::KeyboardInput;
    use client_ui::ui_runtime::presentation::UiPresentationRuntime;
    use ui_engine::test_support::fixture_font;
    use {
        client_shell::menu::{MenuClipboard, MenuRuntime, drive_menu_input},
        launcher::menu::{MenuAction, MenuScreen},
    };
    for local in [false, true] {
        let (mut app, window) = focus_app();
        let mut menu = MenuRuntime::new(true, 2, "test".into());
        let action = if local {
            menu.set_local_worlds(vec![Default::default()]);
            menu.activate(MenuAction::Navigate(MenuScreen::Play));
            menu.activate(MenuAction::PlayLocalWorld(0));
            assert_eq!(menu.take_local_world_request(), Some(0));
            MenuAction::PlayLocalWorld(0)
        } else {
            launcher_host::servers::save_servers(
                &menu.layout().server_file(),
                &[launcher::menu::view::SavedServer {
                    name: "local fixture".into(),
                    address: "127.0.0.1".into(),
                    favorite: false,
                    last_joined_unix: 0,
                }],
            )
            .unwrap();
            menu = MenuRuntime::new_with_layout(
                true,
                Some(2),
                "test".into(),
                menu.layout().clone(),
                menu.player_skin().clone(),
            );
            menu.activate(MenuAction::Navigate(MenuScreen::Servers));
            menu.activate(MenuAction::SelectServerTab(
                launcher::menu::MenuServerTab::Saved,
            ));
            MenuAction::PlaySaved(0)
        };
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        presentation.set_menu_view(Some(menu.view()));
        presentation
            .build(
                app.world().resource::<client_state::PlayerRuntime>(),
                app.world().resource::<UiRuntimeResource>(),
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let point = if local {
            ui::UiPoint::new(0.0, 0.0).unwrap()
        } else {
            (0..720)
                .step_by(4)
                .flat_map(|y| {
                    (0..1280)
                        .step_by(4)
                        .map(move |x| ui::UiPoint::new(x as f32, y as f32).unwrap())
                })
                .find(|point| presentation.hit_test_menu(*point) == Some(action))
                .unwrap()
        };
        app.init_resource::<Touches>()
            .add_message::<KeyboardInput>()
            .insert_resource(menu)
            .insert_resource(UiPresentationResource(presentation))
            .insert_resource(MenuClipboard::with_access(|_| None, |_| {}))
            .add_systems(
                Update,
                drive_menu_input.before(client_shell::menu::screen_focus::publish_screen_focus),
            );
        {
            let mut win = app.world_mut().get_mut::<Window>(window).unwrap();
            win.resolution.set_physical_resolution(1280, 720);
            win.set_cursor_position(Some(Vec2::new(point.x(), point.y())));
            win.focused = false;
        }
        app.update();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert_released(&app, window);
        if local {
            use bevy::input::{
                ButtonState,
                keyboard::{Key, NativeKey},
            };
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Enter);
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
        } else {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
        }
        app.update();
        assert_released(&app, window);
        {
            let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
            assert!(menu.is_visible());
            if local {
                assert_eq!(menu.take_local_world_request(), Some(0));
            } else {
                assert!(menu.take_join_intent().is_some());
            }
        }
        // Loading still owns input after the original pointer edge is consumed.
        app.update();
        assert_released(&app, window);
        app.world_mut().resource_mut::<MenuRuntime>().show_world();
        app.update();
        assert_eq!(
            app.world().get::<CursorOptions>(window).unwrap().grab_mode,
            CursorGrabMode::Locked,
            "join capture for local={local}"
        );
    }
}

/// Without an OS grab, gameplay holds input exactly where desktop would lock the cursor, so a
/// hardware keyboard drives gameplay and screens still own their input.
#[test]
fn grab_free_capture_follows_the_desktop_capture_rules() {
    let gameplay = client_presentation::observations::CursorPolicy {
        capture_allowed: true,
        consent: false,
        absorbs_input: false,
        steals_mouse: Some(true),
        driven: false,
    };
    assert!(super::grab_free_capture(&gameplay, true));
    assert!(!super::grab_free_capture(&gameplay, false), "unfocused");
    for blocked in [
        client_presentation::observations::CursorPolicy {
            capture_allowed: false,
            ..gameplay
        },
        client_presentation::observations::CursorPolicy {
            consent: true,
            ..gameplay
        },
        client_presentation::observations::CursorPolicy {
            absorbs_input: true,
            ..gameplay
        },
        client_presentation::observations::CursorPolicy {
            steals_mouse: Some(false),
            ..gameplay
        },
        client_presentation::observations::CursorPolicy {
            driven: true,
            ..gameplay
        },
    ] {
        assert!(!super::grab_free_capture(&blocked, true));
    }
}

/// Builds a focus app driving a shown data-driven custom form (instance 7, form 4) through a
/// painted presentation.
fn data_driven_app() -> (App, Entity) {
    use crate::ui_runtime::data_driven::drive_data_driven_input;
    use bevy::input::{keyboard::KeyboardInput, mouse::MouseWheel};
    use client_ui::{
        test_support::fixture_hud,
        ui_runtime::{SequencedUiEvent, presentation::UiPresentationRuntime},
    };
    use protocol::{ddui::ScriptedScreen, test_support::ddui};
    use ui_engine::test_support::fixture_font;
    let (mut app, window) = focus_app();
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut player = player_state::PlayerState::new(1);
    let events = ddui::show_events(
        ScriptedScreen::CustomForm,
        4,
        7,
        ddui::custom_form_property(),
    );
    {
        let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
        for (sequence, event) in (1..).zip(events) {
            runtime
                .apply(
                    &mut player,
                    SequencedUiEvent {
                        session_id: 1,
                        fifo_sequence: sequence,
                        local_millis: sequence * 10,
                        server_tick: None,
                        event,
                    },
                )
                .unwrap();
        }
        presentation
            .build(
                &player,
                &runtime,
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    app.add_message::<KeyboardInput>()
        .add_message::<MouseWheel>()
        .add_message::<WindowEvent>()
        .insert_resource(UiPresentationResource(presentation))
        .add_systems(
            Update,
            (
                crate::ui_runtime::data_driven::publish_host_screens,
                drive_data_driven_input,
            )
                .chain()
                .before(client_shell::menu::screen_focus::publish_screen_focus),
        );
    (app, window)
}

/// A key press as the ordered window stream carries it.
fn window_key(window: Entity, key_code: KeyCode, text: Option<&str>) -> WindowEvent {
    use bevy::input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
    };
    WindowEvent::KeyboardInput(KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state: ButtonState::Pressed,
        text: text.map(Into::into),
        repeat: false,
        window,
    })
}

#[test]
fn escaping_a_data_driven_form_returns_capture_after_focus_loss() {
    let (mut app, window) = data_driven_app();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    assert_released(&app, window);
    // The input plugin mirrors the ordered key into the pressed keys, which activate focus.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.world_mut()
        .write_message(window_key(window, KeyCode::Escape, None));
    app.update();
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .data_driven_ui()
            .active()
            .is_none()
    );
    app.update();
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );
}

#[test]
fn text_typed_after_a_click_in_one_frame_reaches_the_clicked_field() {
    use bevy::input::{ButtonState, mouse::MouseButtonInput};
    use oreui::data_driven_screen::FormHit;
    use protocol::ddui::{
        DataPrimitive, DataStoreUpdate, ScriptedScreen, data_store_update_packet,
    };
    let (mut app, window) = data_driven_app();
    // Layout entry 6 is the fixture's text field.
    let field = app
        .world()
        .resource::<UiPresentationResource>()
        .data_driven_target(FormHit::Item(6))
        .unwrap();
    let centre = Vec2::new(
        (field.min().x() + field.max().x()) * 0.5,
        (field.min().y() + field.max().y()) * 0.5,
    );
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(centre));
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut()
            .write_message(WindowEvent::MouseButtonInput(MouseButtonInput {
                button: MouseButton::Left,
                state,
                window,
            }));
    }
    app.world_mut()
        .write_message(window_key(window, KeyCode::KeyX, Some("x")));
    app.update();
    let sent = app
        .world_mut()
        .resource_mut::<UiRuntimeResource>()
        .take_client_packet()
        .map(|packet| protocol::encode(&packet).unwrap());
    let expected = data_store_update_packet(&DataStoreUpdate {
        store: protocol::ddui::SCRIPT_STORE.into(),
        property: ScriptedScreen::CustomForm.property(Some(7)).into(),
        path: "layout[6].text".into(),
        value: DataPrimitive::String("hellox".into()),
        property_update_count: 1,
        path_update_count: 1,
    });
    assert_eq!(sent, Some(protocol::encode(&expected).unwrap()));
}

#[test]
fn ordered_events_from_a_consent_frame_are_not_replayed_to_the_form() {
    let (mut app, window) = data_driven_app();
    app.update();
    app.insert_resource(client_state::ConsentInput(true));
    app.world_mut()
        .write_message(window_key(window, KeyCode::Escape, None));
    app.update();
    app.insert_resource(client_state::ConsentInput(false));
    app.update();
    assert!(
        app.world()
            .resource::<UiRuntimeResource>()
            .data_driven_ui()
            .active()
            .is_some()
    );
}

#[test]
fn a_press_and_release_resolve_where_the_cursor_was_for_each() {
    use bevy::{
        input::{ButtonState, mouse::MouseButtonInput},
        window::CursorMoved,
    };
    use oreui::data_driven_screen::FormHit;
    let (mut app, window) = data_driven_app();
    // Layout entry 9 is the fixture's "Click me" button.
    let button = app
        .world()
        .resource::<UiPresentationResource>()
        .data_driven_target(FormHit::Item(9))
        .unwrap();
    let on_button = Vec2::new(
        (button.min().x() + button.max().x()) * 0.5,
        (button.min().y() + button.max().y()) * 0.5,
    );
    let outside = Vec2::new(20.0, 20.0);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(outside));
    app.update();
    let edge = |state| {
        WindowEvent::MouseButtonInput(MouseButtonInput {
            button: MouseButton::Left,
            state,
            window,
        })
    };
    app.world_mut().write_message(edge(ButtonState::Pressed));
    app.world_mut()
        .write_message(WindowEvent::CursorMoved(CursorMoved {
            window,
            position: on_button,
            delta: None,
        }));
    app.world_mut().write_message(edge(ButtonState::Released));
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(on_button));
    app.update();
    // The press began off the form, so releasing over the button does not press it.
    assert!(
        app.world_mut()
            .resource_mut::<UiRuntimeResource>()
            .take_client_packet()
            .is_none()
    );
}

/// The window-logical centre of the form target `hit` from the app's last paint.
fn target_centre(app: &App, hit: oreui::data_driven_screen::FormHit) -> Vec2 {
    let bounds = app
        .world()
        .resource::<UiPresentationResource>()
        .data_driven_target(hit)
        .unwrap();
    Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    )
}

/// A left-button edge as the ordered window stream carries it.
fn window_edge(window: Entity, state: bevy::input::ButtonState) -> WindowEvent {
    WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
        button: MouseButton::Left,
        state,
        window,
    })
}

#[test]
fn a_click_after_unfocused_cursor_moves_lands_where_the_cursor_is() {
    use bevy::{input::ButtonState, window::CursorMoved};
    use oreui::data_driven_screen::FormHit;
    let (mut app, window) = data_driven_app();
    // Layout entry 9 is the fixture's "Click me" button.
    let on_button = target_centre(&app, FormHit::Item(9));
    let set_cursor = |app: &mut App, at: Vec2| {
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(at));
    };
    set_cursor(&mut app, Vec2::new(20.0, 20.0));
    app.update();
    // The cursor moves onto the button while the window is unfocused.
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.world_mut()
        .write_message(WindowEvent::CursorMoved(CursorMoved {
            window,
            position: on_button,
            delta: None,
        }));
    set_cursor(&mut app, on_button);
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.world_mut()
        .write_message(window_edge(window, ButtonState::Pressed));
    app.world_mut()
        .write_message(window_edge(window, ButtonState::Released));
    app.update();
    assert!(
        app.world_mut()
            .resource_mut::<UiRuntimeResource>()
            .take_client_packet()
            .is_some(),
        "the click pressed the button under the cursor"
    );
}

#[test]
fn a_slider_drag_within_one_frame_writes_its_release_position() {
    use bevy::{input::ButtonState, window::CursorMoved};
    use oreui::data_driven_screen::FormHit;
    use protocol::ddui::{
        DataPrimitive, DataStoreUpdate, ScriptedScreen, data_store_update_packet,
    };
    let (mut app, window) = data_driven_app();
    // Layout entry 4 is the fixture's 0..10 slider, stepping by 2.
    let track = app
        .world()
        .resource::<UiPresentationResource>()
        .data_driven_target(FormHit::Item(4))
        .unwrap();
    let y = (track.min().y() + track.max().y()) * 0.5;
    let start = Vec2::new(track.min().x() + 1.0, y);
    let end = Vec2::new(track.max().x() - 1.0, y);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(start));
    app.update();
    app.world_mut()
        .write_message(window_edge(window, ButtonState::Pressed));
    app.world_mut()
        .write_message(WindowEvent::CursorMoved(CursorMoved {
            window,
            position: end,
            delta: None,
        }));
    app.world_mut()
        .write_message(window_edge(window, ButtonState::Released));
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(end));
    app.update();
    let sent: Vec<_> = std::iter::from_fn(|| {
        app.world_mut()
            .resource_mut::<UiRuntimeResource>()
            .take_client_packet()
    })
    .map(|packet| protocol::encode(&packet).unwrap())
    .collect();
    let value = |value, count| {
        protocol::encode(&data_store_update_packet(&DataStoreUpdate {
            store: protocol::ddui::SCRIPT_STORE.into(),
            property: ScriptedScreen::CustomForm.property(Some(7)).into(),
            path: "layout[4].value".into(),
            value: DataPrimitive::Number(value),
            property_update_count: 1,
            path_update_count: count,
        }))
        .unwrap()
    };
    assert_eq!(sent, [value(0.0, 1), value(10.0, 2)]);
}

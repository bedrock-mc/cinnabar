use super::*;
use bevy::window::CursorGrabMode;
use client_ui::ui_runtime::UiRuntime;

/// Builds the production focus/capture boundary without a windowing plugin or OS input.
fn focus_app() -> (App, Entity) {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(AutoFly::new(false))
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(UiRuntime::new(1))
        .add_systems(Update, update_cursor_capture);
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
    assert!(!super::super::input_is_active(
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
    assert_released(&app, window);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
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
fn occlusion_releases_until_unoccluded_and_clicked_and_ignores_other_windows() {
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
    assert_released(&app, window);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
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
fn late_screen_capture_cannot_override_loss_or_driven_control() {
    let (mut app, window) = focus_app();
    app.add_systems(Update, restore_screen_capture.after(update_cursor_capture));
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    assert_released(&app, window);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    assert_released(&app, window);
    app.init_resource::<DrivenInput>();
    app.update();
    assert_released(&app, window);
}

#[test]
fn json_ui_form_release_restores_capture_after_overlay_and_response_delivery() {
    use crate::ui_runtime::{
        drive_server_form_input,
        presentation::forms::{pack_harness, tests::mini_engine_presentation},
    };
    use bevy::input::{ButtonState, InputPlugin, mouse::MouseButtonInput};
    use client_ui::ui_runtime::flush_form_response;

    let mut player = crate::player_runtime::PlayerRuntime::new(1);
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
        .insert_resource(AutoFly::new(false))
        .insert_resource(player)
        .insert_resource(runtime)
        .insert_resource(presentation)
        .add_systems(
            Update,
            (drive_server_form_input, update_cursor_capture).chain(),
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
            .resource::<UiRuntime>()
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
            .resource::<UiRuntime>()
            .server_forms()
            .active()
            .is_none()
    );
    assert!(
        app.world()
            .resource::<UiRuntime>()
            .server_forms()
            .owns_input()
    );
    assert_released(&app, window);
    flush_form_response(&mut app.world_mut().resource_mut::<UiRuntime>(), |_| Ok(())).unwrap();
    app.update();
    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
    assert!(!cursor.visible);
}

#[test]
fn discarded_pointer_batch_cannot_authorize_a_later_programmatic_close() {
    use bevy::input::{ButtonState, mouse::MouseButtonInput};
    let (mut app, window) = focus_app();
    let mut player = crate::player_runtime::PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player);
    app.insert_resource(player).insert_resource(runtime);
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
    app.world_mut().resource_mut::<UiRuntime>().close_chat();
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
            drive_chat_keyboard_input.before(update_cursor_capture),
        );
    {
        let mut runtime = app.world_mut().resource_mut::<UiRuntime>();
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
            .resource::<UiRuntime>()
            .credits()
            .active()
            .is_none()
    );
    assert_released(&app, window);
    app.world_mut()
        .resource_mut::<UiRuntime>()
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
    use client_ui::{
        test_support::{fixture_font, fixture_hud},
        ui_runtime::presentation::UiPresentationRuntime,
    };
    let (mut app, window) = focus_app();
    let player = crate::player_runtime::PlayerRuntime::new(1);
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
        .find(|point| {
            presentation.hit_test_bed(*point)
                == Some(client_ui::ui_runtime::presentation::BedHit::LeaveBed)
        })
        .unwrap();
    let centre = Vec2::new(point.x(), point.y());
    app.insert_resource(player)
        .insert_resource(runtime)
        .insert_resource(presentation)
        .init_resource::<Time<Real>>()
        .init_resource::<Touches>()
        .init_resource::<crate::local_player::LocalPlayerFrameCarrier>()
        .init_resource::<crate::local_player::InteractionOriginSnapshot>()
        .init_resource::<crate::semantic_controls::SemanticInputSnapshot>()
        .init_resource::<crate::runtime::world::ClientWorld>()
        .add_systems(Update, drive_chat_ui_actions.before(update_cursor_capture));
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
            .resource_mut::<UiRuntime>()
            .flush_wake_request(Some(1), |_| Ok::<_, ()>(()))
    );
    app.world_mut()
        .resource_mut::<UiRuntime>()
        .set_local_sleeping(false);
    app.update();
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );
}

#[test]
fn touch_resume_returns_capture_after_focus_loss() {
    use crate::menu::{MenuAction, MenuClipboard, MenuRuntime, drive_menu_input};
    use bevy::input::{
        InputPlugin,
        touch::{TouchInput, TouchPhase},
    };
    use client_ui::{test_support::fixture_font, ui_runtime::presentation::UiPresentationRuntime};
    let (mut app, window) = focus_app();
    app.add_plugins(InputPlugin);
    let player = crate::player_runtime::PlayerRuntime::new(1);
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
        .insert_resource(runtime)
        .insert_resource(menu)
        .insert_resource(presentation)
        .insert_resource(MenuClipboard::with_access(|_| None, |_| {}))
        .add_systems(Update, drive_menu_input.before(update_cursor_capture));
    {
        let mut win = app.world_mut().get_mut::<Window>(window).unwrap();
        win.resolution.set_physical_resolution(1280, 720);
        win.focused = false;
    }
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    app.world_mut().write_message(TouchInput {
        phase: TouchPhase::Started,
        position: Vec2::new(point.x(), point.y()),
        window,
        force: None,
        id: 1,
    });
    app.update();
    assert!(app.world().resource::<MenuRuntime>().is_visible());
    app.world_mut().write_message(TouchInput {
        phase: TouchPhase::Ended,
        position: Vec2::new(point.x(), point.y()),
        window,
        force: None,
        id: 1,
    });
    app.update();
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
            drive_chat_keyboard_input.before(update_cursor_capture),
        );
    app.world_mut()
        .resource_mut::<UiRuntime>()
        .set_local_sleeping(true);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    press_escape(&mut app, window);
    assert_released(&app, window);
    assert!(
        app.world_mut()
            .resource_mut::<UiRuntime>()
            .flush_wake_request(Some(1), |_| Ok::<_, ()>(()))
    );
    app.world_mut()
        .resource_mut::<UiRuntime>()
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
    use client_ui::{
        test_support::fixture_font,
        ui_runtime::{presentation::UiPresentationRuntime, sign_editor::SignEdit},
    };
    let (mut app, window) = focus_app();
    app.add_message::<KeyboardInput>()
        .init_resource::<crate::runtime::world::ClientWorld>()
        .insert_resource(UiPresentationRuntime::new(fixture_font()).unwrap())
        .add_systems(Update, drive_sign_editor.before(update_cursor_capture));
    let mut edit = SignEdit::new([0, 0, 0], true, Default::default());
    assert!(edit.insert('a', |_| true));
    app.world_mut()
        .resource_mut::<UiRuntime>()
        .sign_editor_mut()
        .open(edit);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    press_escape(&mut app, window);
    assert!(app.world().resource::<UiRuntime>().sign_editor().is_open());
    assert_released(&app, window);
    assert!(
        app.world_mut()
            .resource_mut::<UiRuntime>()
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
    use crate::{
        menu::{
            MenuAction, MenuClipboard, MenuRuntime, drive_menu_input,
            settings_options::{EXTRA_KEYS, KEY_BINDINGS},
        },
        ui_runtime::interaction::drive_chat_keyboard_input,
    };
    use bevy::{input::keyboard::KeyboardInput, time::Real};
    use client_ui::{test_support::fixture_font, ui_runtime::presentation::UiPresentationRuntime};
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
        .insert_resource(UiPresentationRuntime::new(fixture_font()).unwrap())
        .add_systems(Update, drive_chat_keyboard_input.before(drive_menu_input))
        .add_systems(Update, drive_menu_input.before(update_cursor_capture));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Back);
    app.update();
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        assert_eq!(
            menu.settings_snapshot()
                .0
                .named_key_control("key.inventory"),
            Some(semantic_input::PhysicalControl::MouseButton(
                crate::semantic_controls::physical::mouse_button_code(MouseButton::Back).unwrap()
            ))
        );
        menu.set_visible(false);
    }
    app.world_mut().resource_scope(
        |world, mut player: Mut<crate::player_runtime::PlayerRuntime>| {
            let mut runtime = world.resource_mut::<UiRuntime>();
            runtime.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
            runtime
                .publish_local_runtime_id(&mut player, 1, 42)
                .unwrap();
            runtime.toggle_inventory(&mut player);
        },
    );
    assert!(app.world().resource::<UiRuntime>().inventory_open());
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Back);
    app.update();
    assert!(!app.world().resource::<UiRuntime>().inventory_open());
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );
}

#[test]
fn explicit_join_keeps_return_authorization_across_loading() {
    use crate::menu::{MenuAction, MenuClipboard, MenuRuntime, MenuScreen, drive_menu_input};
    use bevy::input::keyboard::KeyboardInput;
    use client_ui::{test_support::fixture_font, ui_runtime::presentation::UiPresentationRuntime};
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
            crate::menu::servers::save_servers(
                &menu.layout().server_file(),
                &[crate::menu::SavedServer {
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
                crate::menu::MenuServerTab::Saved,
            ));
            MenuAction::PlaySaved(0)
        };
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        presentation.set_menu_view(Some(menu.view()));
        presentation
            .build(
                app.world()
                    .resource::<crate::player_runtime::PlayerRuntime>(),
                app.world().resource::<UiRuntime>(),
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
            .insert_resource(presentation)
            .insert_resource(MenuClipboard::with_access(|_| None, |_| {}))
            .add_systems(Update, drive_menu_input.before(update_cursor_capture));
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

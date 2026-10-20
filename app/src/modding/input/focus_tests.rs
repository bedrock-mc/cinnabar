use super::*;
use bevy::input::keyboard::Key;
use client_ui::test_support::mini_engine_presentation;
use mod_ui::PanelUiOwner;

mod closing_result;

fn fixture() -> String {
    fixture_with_capture(false)
}

/// Builds the same controls component with optional guest-owned keyboard capture.
fn fixture_with_capture(capture_key: bool) -> String {
    let panel = format!(
        r#"{{"title":"Local controls","toggle_key":"ShiftRight","dark":true,"capture_key":{capture_key},"controls":[{{"kind":"slider","id":"strength","label":"Strength","value":35,"min":0,"max":100,"step":1}}]}}"#
    );
    fixture_with_panel(&panel)
}

/// Builds a granted controls component around a supplied native panel.
fn fixture_with_panel(panel: &str) -> String {
    serde_json::from_str::<ui::mod_panel::Panel>(panel)
        .expect("valid native fixture panel")
        .validate()
        .unwrap();
    let package = include_str!("../../../../crates/mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    include_str!("../../../../crates/mod-host/src/tests/guest.wat")
        .replace("(component", &format!("(component (import \"{name}/panel@{version}\" (instance $panel (export \"set-content\" (func (param \"json\" string) (result (result (error string))))))) (alias export $panel \"set-content\" (func $set-panel))"))
        .replace("$HUD", &format!("{name}/hud@{version}"))
        .replace("$ENVIRONMENT", &format!("{name}/environment@{version}"))
        .replace("$INPUT", &format!("{name}/input@{version}"))
        .replace("$TEXT", "Fixture")
        .replace("$LENGTH", "7")
        .replace("$FRAME", "")
        .replace("(core func $lower-label", "(core func $lower-panel (canon lower (func $set-panel) (memory $memory) (realloc $realloc))) (core func $lower-label")
        .replace("(import \"host\" \"time\"", "(import \"host\" \"panel\" (func $panel (param i32 i32 i32))) (import \"host\" \"time\"")
        .replace("(export \"time\" (func $lower-time))", "(export \"panel\" (func $lower-panel)) (export \"time\" (func $lower-time))")
        .replace("(data (i32.const 0)", &format!("(data (i32.const 1024) \"{}\") (data (i32.const 0)", panel.replace('"', "\\22")))
        .replacen("(func (export \"init\")", &format!("(func (export \"init\") i32.const 1024 i32.const {} i32.const 512 call $panel", panel.len()), 1)
}

fn render(
    presentation: &mut UiPresentationRuntime,
    player: &client_state::PlayerRuntime,
    ui: &UiRuntime,
) {
    presentation
        .build(player, ui, 0, [1280, 720], ui::DpiScale::new(1.0).unwrap())
        .unwrap();
}

#[test]
fn unfocused_stop_and_toggle_keys_preserve_editor_and_do_not_replay_on_regain() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("focus.component.wat");
    let grants = mod_host::ModGrants {
        controls: true,
        ..Default::default()
    };
    let mut host =
        mod_host::ModHost::load_snapshot_with_grants(&path, fixture().as_bytes(), grants).unwrap();
    host.set_panel_open(true);
    let player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let ui = UiRuntime::new(1);
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), host.panel())
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    render(&mut presentation, &player, &ui);
    // Locate the rendered numeric editor through its public pointer routing,
    // rather than coupling this app regression to private layout hit regions.
    'search: for y in (0..720).step_by(8) {
        for x in (0..1280).step_by(8) {
            if !presentation.mod_ui().mod_panel_open() {
                presentation.mod_ui_mut().set_mod_panel_open(true);
                render(&mut presentation, &player, &ui);
            }
            presentation
                .mod_ui_mut()
                .mod_panel_events([x as f32, y as f32], true, true);
            if presentation.mod_ui().mod_panel_editing() {
                break 'search;
            }
        }
    }
    assert!(
        presentation.mod_ui().mod_panel_editing(),
        "rendered numeric input was not reachable"
    );
    presentation
        .mod_ui_mut()
        .mod_panel_key("Digit4", Some("42"));
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(player)
        .insert_resource(UiRuntimeResource(ui))
        .insert_resource(UiPresentationResource(presentation))
        .insert_resource(ModRuntime::new(
            host,
            super::super::registration::HostMetadata::environment(0),
        ))
        .add_systems(Update, prepare_mod_input);
    let entity = app
        .world_mut()
        .spawn((
            Window {
                focused: false,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    for (key_code, logical_key) in [(KeyCode::ShiftRight, Key::Shift), (KeyCode::F10, Key::F10)] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: entity,
        });
    }
    app.update();
    assert!(
        app.world()
            .resource::<ModRuntime>()
            .hosts
            .host(0)
            .panel_open()
    );
    assert!(
        app.world()
            .resource::<UiPresentationResource>()
            .mod_ui()
            .mod_panel_editing()
    );
    assert!(
        app.world()
            .resource::<ModRuntime>()
            .controls
            .keys_pressed
            .is_empty()
    );
    app.world_mut().get_mut::<Window>(entity).unwrap().focused = true;
    app.update();
    assert!(
        app.world()
            .resource::<ModRuntime>()
            .hosts
            .host(0)
            .panel_open()
    );
    assert!(
        app.world()
            .resource::<ModRuntime>()
            .controls
            .keys_pressed
            .is_empty()
    );
    assert_eq!(
        app.world_mut()
            .resource_mut::<UiPresentationResource>()
            .mod_ui_mut()
            .mod_panel_key("Enter", None),
        vec![ui::mod_panel::Event {
            id: "strength".into(),
            value: 42.0
        }]
    );
}

fn hud_editor_app() -> (App, Entity) {
    personal_panel_app(true, false)
}

/// Text-only keyboard messages edit native fields without becoming held gameplay controls.
#[test]
fn text_without_a_physical_key_reaches_the_native_editor() {
    let (mut app, window) = personal_panel_app(false, false);
    let directory = tempfile::tempdir().unwrap();
    let other = mod_host::ModHost::load_snapshot_with_grants(
        &directory.path().join("other.wat"),
        fixture().replace("ShiftRight", "TextInput").as_bytes(),
        mod_host::ModGrants {
            controls: true,
            hud: true,
            ..Default::default()
        },
    )
    .unwrap();
    {
        let mut runtime = app.world_mut().resource_mut::<ModRuntime>();
        runtime.push_host(
            other,
            super::super::registration::HostMetadata::environment(1),
        );
    }
    open_rendered_field(&mut app);
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
        logical_key: Key::Character("42".into()),
        state: ButtonState::Pressed,
        text: Some("42".into()),
        repeat: false,
        window,
    });
    app.update();
    assert_eq!(
        app.world().resource::<ModRuntime>().hosts.panel_owner(),
        0,
        "text input cannot act as another component's physical toggle key"
    );
    assert!(
        app.world()
            .resource::<ModRuntime>()
            .controls
            .keys_held
            .is_empty()
    );
    assert!(
        app.world()
            .resource::<ModRuntime>()
            .controls
            .keys_pressed
            .is_empty()
    );
    // The panel clears shared key state each frame; the physical modifier must survive that.
    for (key_code, text) in [
        (KeyCode::SuperLeft, None),
        (KeyCode::KeyA, None),
        (
            KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
            Some("57"),
        ),
    ] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
            state: ButtonState::Pressed,
            text: text.map(Into::into),
            repeat: false,
            window,
        });
        app.update();
    }
    for (key_code, state, text) in [
        (KeyCode::KeyA, ButtonState::Pressed, None),
        (KeyCode::SuperLeft, ButtonState::Released, None),
        (
            KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
            ButtonState::Pressed,
            Some("73"),
        ),
    ] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
            state,
            text: text.map(Into::into),
            repeat: false,
            window,
        });
    }
    app.update();
    assert_eq!(
        app.world_mut()
            .resource_mut::<UiPresentationResource>()
            .mod_ui_mut()
            .mod_panel_key("Enter", None),
        vec![ui::mod_panel::Event {
            id: "strength".into(),
            value: 73.
        }]
    );
}

/// Focuses the first native editor through rendered pointer routing.
fn open_rendered_field(app: &mut App) {
    let mut presentation = app.world_mut().resource_mut::<UiPresentationResource>();
    let frame = presentation
        .mod_ui()
        .mod_panel_state()
        .unwrap()
        .frame
        .expect("native panel rendered");
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("mod.edit:0"))
        .or_else(|| {
            frame
                .hits
                .iter()
                .find(|hit| hit.pressed.as_deref() == Some("mod.control:0"))
        })
        .expect("native field has a rendered action");
    let point = [
        (hit.rect.x + hit.rect.w * 0.5) as f32 * frame.scale + frame.origin[0],
        (hit.rect.y + hit.rect.h * 0.5) as f32 * frame.scale + frame.origin[1],
    ];
    presentation
        .mod_ui_mut()
        .mod_panel_events(point, true, true);
    assert!(presentation.mod_ui().mod_panel_editing());
}

#[test]
fn native_text_clipboard_shortcuts_do_not_insert_command_letters() {
    use std::sync::{Arc, Mutex};
    for modifier in [KeyCode::SuperLeft, KeyCode::ControlLeft] {
        let component = fixture_with_panel(
            r#"{"title":"Text controls","toggle_key":"ShiftRight","dark":true,"controls":[{"kind":"text","id":"name","label":"Name","value":"Main","min_chars":0,"max_chars":24}]}"#,
        );
        let (mut app, window) = personal_panel_app_with_fixture(false, &component);
        let clipboard = Arc::new(Mutex::new("#7F00FF00".to_owned()));
        let reader = clipboard.clone();
        let writer = clipboard.clone();
        app.insert_resource(client_shell::menu::MenuClipboard::with_access(
            move |_| Some(reader.lock().unwrap().clone()),
            move |text| *writer.lock().unwrap() = text,
        ));
        open_rendered_field(&mut app);
        for (key_code, text) in [
            (modifier, None),
            (KeyCode::KeyC, Some("c")),
            (KeyCode::KeyX, Some("x")),
            (KeyCode::KeyV, Some("v")),
            (KeyCode::KeyB, Some("b")),
            (KeyCode::Digit1, Some("1")),
        ] {
            if key_code == KeyCode::KeyV {
                *clipboard.lock().unwrap() = "#7F00FF00".to_owned();
            }
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
                state: ButtonState::Pressed,
                text: text.map(Into::into),
                repeat: false,
                window,
            });
            app.update();
            if key_code == KeyCode::KeyC || key_code == KeyCode::KeyX {
                assert_eq!(*clipboard.lock().unwrap(), "Main");
            }
        }
        let mut presentation = app.world_mut().resource_mut::<UiPresentationResource>();
        assert!(
            presentation
                .mod_ui_mut()
                .mod_panel_key("Enter", None)
                .is_empty()
        );
        assert_eq!(
            presentation
                .mod_ui_mut()
                .take_mod_panel_text_events(PanelUiOwner::Environment(0)),
            vec![ui::mod_panel::TextEvent {
                id: "name".into(),
                value: "#7F00FF00".into()
            }]
        );
    }
}

/// Opens either a native HUD editor or a guest keyboard-capturing personal panel.
fn personal_panel_app(hud_editor: bool, capture_key: bool) -> (App, Entity) {
    personal_panel_app_with_fixture(hud_editor, &fixture_with_capture(capture_key))
}

/// Creates an app whose personal panel comes from the supplied component fixture.
fn personal_panel_app_with_fixture(hud_editor: bool, component: &str) -> (App, Entity) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hud-editor.component.wat");
    let mut host = mod_host::ModHost::load_snapshot_with_grants(
        &path,
        component.as_bytes(),
        mod_host::ModGrants {
            hud: true,
            controls: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(host.panel().is_some(), "fixture panel was not admitted");
    host.set_panel_open(true);
    let player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let ui = UiRuntime::new(1);
    let mut presentation = mini_engine_presentation();
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), host.panel())
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    if hud_editor {
        let preview:ui::mod_hud::Hud=serde_json::from_str(r#"{"cards":[{"id":"equipment","position":[0.5,0.5],"rows":[{"label":"Helmet","value":"85%"}]}]}"#).unwrap();
        presentation
            .mod_ui_mut()
            .open_mod_hud_editor(&preview)
            .unwrap();
    }
    render(&mut presentation, &player, &ui);
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(player)
        .insert_resource(UiRuntimeResource(ui))
        .insert_resource(UiPresentationResource(presentation))
        .insert_resource(ModRuntime {
            hud_editor_owner: hud_editor.then_some(super::super::hud_editor::Owner {
                host: 0,
                session: 1,
            }),
            ..ModRuntime::new(
                host,
                super::super::registration::HostMetadata::environment(0),
            )
        })
        .add_systems(Update, prepare_mod_input);
    let mut window = Window {
        focused: true,
        ..Default::default()
    };
    window.set_cursor_position(Some(Vec2::new(640., 360.)));
    let entity = app
        .world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow))
        .id();
    (app, entity)
}

/// A modifier binding and the native fallback both open and close the selected panel without repeat toggles.
#[test]
fn personal_panel_opens_with_right_shift_or_f10_without_text_input() {
    for (key_code, logical_key) in [(KeyCode::ShiftRight, Key::Shift), (KeyCode::F10, Key::F10)] {
        let (mut app, window) = personal_panel_app(false, false);
        app.world_mut()
            .resource_mut::<ModRuntime>()
            .hosts
            .host_mut(0)
            .set_panel_open(false);
        app.world_mut()
            .resource_mut::<UiPresentationResource>()
            .mod_ui_mut()
            .set_mod_panel_open(false);
        let event = KeyboardInput {
            key_code,
            logical_key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        };
        app.world_mut().write_message(event.clone());
        app.update();
        assert!(
            app.world()
                .resource::<ModRuntime>()
                .hosts
                .host(0)
                .panel_open(),
            "{key_code:?}"
        );
        app.world_mut().write_message(KeyboardInput {
            repeat: true,
            ..event.clone()
        });
        app.update();
        assert!(
            app.world()
                .resource::<ModRuntime>()
                .hosts
                .host(0)
                .panel_open()
        );
        app.world_mut().write_message(KeyboardInput {
            state: ButtonState::Released,
            ..event.clone()
        });
        app.update();
        app.world_mut().write_message(event);
        app.update();
        assert!(
            !app.world()
                .resource::<ModRuntime>()
                .hosts
                .host(0)
                .panel_open()
        );
    }
}

#[test]
fn personal_panel_keyboard_capture_and_native_editor_do_not_open_or_replay_world_ui() {
    for hud_editor in [false, true] {
        let (mut app, entity) = personal_panel_app(hud_editor, !hud_editor);
        app.init_resource::<Time<bevy::time::Real>>().add_systems(
            Update,
            crate::ui_runtime::drive_chat_keyboard_input.after(prepare_mod_input),
        );
        for (key_code, logical_key) in [
            (KeyCode::KeyT, Key::Character("t".into())),
            (KeyCode::KeyE, Key::Character("e".into())),
            (KeyCode::Enter, Key::Enter),
        ] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key_code);
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: entity,
            });
            app.update();
            let runtime = app.world().resource::<ModRuntime>();
            assert!(runtime.hosts.host(0).panel_open());
            if hud_editor {
                assert!(runtime.controls.keys_pressed.is_empty());
            } else {
                assert_eq!(runtime.controls.keys_pressed, [format!("{key_code:?}")]);
            }
            assert!(
                app.world()
                    .resource::<UiPresentationResource>()
                    .mod_ui()
                    .mod_panel_open()
            );
            let ui = app.world().resource::<UiRuntimeResource>();
            assert!(!ui.chat_focused(), "personal-panel key opened chat");
            assert!(!ui.inventory_open(), "personal-panel key opened inventory");
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(key_code);
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
                state: ButtonState::Released,
                text: None,
                repeat: false,
                window: entity,
            });
            app.update();
        }
        assert!(
            !app.world()
                .resource::<UiPresentationResource>()
                .mod_ui()
                .mod_hud_editor_open(),
            "Enter still saves and closes the native HUD editor"
        );
        app.world_mut()
            .resource_mut::<UiPresentationResource>()
            .mod_ui_mut()
            .set_mod_panel_open(false);
        app.world_mut()
            .resource_mut::<ModRuntime>()
            .hosts
            .host_mut(0)
            .set_panel_open(false);
        app.update();
        assert!(!app.world().resource::<UiRuntimeResource>().chat_focused());
        assert!(!app.world().resource::<UiRuntimeResource>().inventory_open());

        // Fresh world input works after ownership ends; captured messages stay consumed.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: entity,
        });
        app.update();
        assert!(app.world().resource::<UiRuntimeResource>().chat_focused());
    }
}

#[test]
fn hud_drag_released_outside_window_does_not_resume_on_pointer_reentry() {
    let (mut app, entity) = hud_editor_app();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window: entity,
    });
    app.update();
    app.world_mut()
        .get_mut::<Window>(entity)
        .unwrap()
        .set_cursor_position(None);
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window: entity,
    });
    app.update();
    app.world_mut()
        .get_mut::<Window>(entity)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(1100., 640.)));
    app.update();
    let mut presentation = app.world_mut().resource_mut::<UiPresentationResource>();
    presentation.mod_ui_mut().mod_panel_key("Enter", None);
    let result = presentation
        .mod_ui_mut()
        .take_mod_hud_editor_result()
        .unwrap();
    assert!(result.saved);
    assert_eq!(result.placements[0].position, Some([0.5, 0.5]));
}

#[test]
fn hud_editor_focus_and_session_loss_cancel_draft_and_release_world_input_ownership() {
    for focus_loss in [false, true] {
        let (mut app, entity) = hud_editor_app();
        app.update();
        if focus_loss {
            app.world_mut().get_mut::<Window>(entity).unwrap().focused = false;
        } else {
            app.world_mut()
                .resource_mut::<UiRuntimeResource>()
                .begin_session(2);
        }
        app.update();
        let p = app.world().resource::<UiPresentationResource>();
        assert!(!p.mod_ui().mod_hud_editor_open());
        assert!(!p.mod_ui().mod_panel_open());
        let runtime = app.world().resource::<ModRuntime>();
        assert!(runtime.hud_editor_owner.is_none());
        assert!(!runtime.hosts.host(0).panel_open());
        assert!(!runtime.controls.panel_open);
    }
}

#[test]
fn hud_editor_save_and_cancel_return_to_panel_without_replaying_gameplay_keys() {
    for (key_code, logical_key) in [(KeyCode::Enter, Key::Enter), (KeyCode::Escape, Key::Escape)] {
        let (mut app, entity) = hud_editor_app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key_code);
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: entity,
        });
        app.update();
        let p = app.world().resource::<UiPresentationResource>();
        assert!(!p.mod_ui().mod_hud_editor_open());
        assert!(p.mod_ui().mod_panel_open());
        let runtime = app.world().resource::<ModRuntime>();
        assert!(runtime.hud_editor_owner.is_none());
        assert!(runtime.hosts.host(0).panel_open());
        assert!(runtime.controls.panel_open);
        assert!(runtime.controls.keys_pressed.is_empty());
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(key_code)
        );
    }
}

#[test]
fn hud_editor_owner_switch_cancels_old_draft_without_keeping_input_capture() {
    let (mut app, _) = hud_editor_app();
    let directory = tempfile::tempdir().unwrap();
    let empty = br#"(component (core module $m (func (export "init")) (func (export "frame")))
        (core instance $i (instantiate $m))
        (func (export "init") (canon lift (core func $i "init")))
        (func (export "frame") (canon lift (core func $i "frame"))))"#;
    let empty_host = mod_host::ModHost::load_snapshot_with_grants(
        &directory.path().join("empty.wat"),
        empty,
        mod_host::ModGrants::default(),
    )
    .unwrap();
    {
        let mut runtime = app.world_mut().resource_mut::<ModRuntime>();
        runtime.push_host(
            empty_host,
            super::super::registration::HostMetadata::environment(1),
        );
        runtime.swap_hosts(0, 1);
        assert_eq!(runtime.hosts.panel_owner(), 1);
    }
    app.update();
    assert!(
        app.world()
            .resource::<UiPresentationResource>()
            .mod_ui()
            .mod_hud_editor_open()
    );
    let mut new_host = mod_host::ModHost::load_snapshot_with_grants(
        &directory.path().join("new.wat"),
        fixture().as_bytes(),
        mod_host::ModGrants {
            hud: true,
            controls: true,
            ..Default::default()
        },
    )
    .unwrap();
    new_host.set_panel_open(false);
    {
        let mut runtime = app.world_mut().resource_mut::<ModRuntime>();
        *runtime.hosts.host_mut(0) = new_host;
        runtime.hosts.select_panel(&["ShiftRight".into()]);
    }
    app.update();
    assert!(
        !app.world()
            .resource::<UiPresentationResource>()
            .mod_ui()
            .mod_hud_editor_open()
    );
    assert!(
        !app.world()
            .resource::<UiPresentationResource>()
            .mod_ui()
            .mod_panel_open()
    );
    let runtime = app.world().resource::<ModRuntime>();
    assert!(runtime.hud_editor_owner.is_none());
    assert_eq!(runtime.hosts.panel_owner(), 0);
    assert!(runtime.hosts.host(0).is_active() && runtime.hosts.host(1).is_active());
    assert!(
        !runtime.hosts.host(1).panel_open(),
        "old owner cannot reopen stale input ownership"
    );
    assert!(!runtime.controls.panel_open);
}

#[test]
fn incremental_hud_save_retains_component_owner_until_editor_finishes() {
    let (mut app, _) = hud_editor_app();
    let preview: ui::mod_hud::Hud = serde_json::from_str(
        r#"{"autosave":true,"cards":[{"id":"equipment","position":[0.5,0.5],"rows":[{"label":"Helmet","value":"85%"}]}]}"#,
    ).unwrap();
    app.world_mut()
        .resource_scope(|world, mut presentation: Mut<UiPresentationResource>| {
            presentation
                .mod_ui_mut()
                .open_mod_hud_editor(&preview)
                .unwrap();
            render(
                &mut presentation,
                world.resource::<client_state::PlayerRuntime>(),
                world.resource::<UiRuntimeResource>(),
            );
            presentation
                .mod_ui_mut()
                .mod_panel_events([640., 360.], true, false);
            presentation.mod_ui_mut().mod_panel_key("ArrowRight", None);
            let first = presentation
                .mod_ui_mut()
                .take_mod_hud_editor_result()
                .expect("rendered card selection must produce a saved nudge");
            assert!(first.saved);
            presentation.mod_ui_mut().mod_panel_key("ArrowRight", None);
            let mut runtime = world.resource_mut::<ModRuntime>();
            super::super::hud_editor::collect(&mut runtime, &mut presentation);
            assert!(
                runtime.hud_editor_owner.is_some(),
                "a saved checkpoint keeps its component owner"
            );
            assert!(presentation.mod_ui().mod_hud_editor_open());
            assert!(
                presentation
                    .mod_ui_mut()
                    .take_mod_hud_editor_result()
                    .is_none(),
                "checkpoint delivered once"
            );
            presentation.mod_ui_mut().cancel_mod_hud_editor();
            super::super::hud_editor::collect(&mut runtime, &mut presentation);
            assert!(runtime.hud_editor_owner.is_none());
        });
}

#[test]
fn autosave_editor_escape_closes_entire_panel_without_reopening_it() {
    let (mut app, entity) = hud_editor_app();
    let preview: ui::mod_hud::Hud = serde_json::from_str(
        r#"{"autosave":true,"cards":[{"id":"equipment","position":[0.5,0.5],"rows":[{"label":"Helmet","value":"85%"}]}]}"#,
    ).unwrap();
    app.world_mut()
        .resource_mut::<UiPresentationResource>()
        .mod_ui_mut()
        .open_mod_hud_editor(&preview)
        .unwrap();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Escape,
        logical_key: Key::Escape,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: entity,
    });
    app.update();
    let p = app.world().resource::<UiPresentationResource>();
    assert!(!p.mod_ui().mod_hud_editor_open());
    assert!(
        !p.mod_ui().mod_panel_open(),
        "keyboard dismissal survives pointer-routing setup"
    );
    let runtime = app.world().resource::<ModRuntime>();
    assert!(!runtime.hosts.host(0).panel_open() && !runtime.controls.panel_open);
    assert!(runtime.hud_editor_owner.is_none());
    assert!(
        runtime.controls.keys_pressed.is_empty(),
        "Escape does not leak to gameplay"
    );
}

/// Closing chrome keeps its original live-session fence until the animation completes.
#[test]
fn hud_editor_closing_surface_is_revoked_on_focus_or_session_loss() {
    for focus_loss in [false, true] {
        let (mut app, window) = hud_editor_app();
        let preview: ui::mod_hud::Hud = serde_json::from_value(serde_json::json!({
            "autosave":true,"cards":[],
            "surface":{
                "screen":"fixture.root",
                "document":serde_json::json!({"namespace":"fixture","root":{
                    "type":"screen","size":["100%","100%"],"controls":[]
                }}).to_string(),
                "bindings":{"#alpha":1},
                "transitions":{"#alpha":{"duration":0.2,"exit":0}}
            }
        }))
        .unwrap();
        {
            let mut p = app.world_mut().resource_mut::<UiPresentationResource>();
            p.mod_ui_mut().open_mod_hud_editor(&preview).unwrap();
            render(
                &mut p,
                &client_state::PlayerRuntime(player_state::PlayerState::new(1)),
                &UiRuntime::new(1),
            );
        }
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::ShiftRight,
            logical_key: Key::Shift,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        {
            let p = app.world().resource::<UiPresentationResource>();
            assert!(!p.mod_ui().mod_hud_editor_open() && !p.mod_ui().mod_panel_open());
            assert!(p.mod_ui().mod_hud_editor_painting());
            assert!(
                app.world()
                    .resource::<ModRuntime>()
                    .hud_editor_owner
                    .is_some()
            );
        }
        app.update();
        assert!(
            app.world()
                .resource::<UiPresentationResource>()
                .mod_ui()
                .mod_hud_editor_painting(),
            "an ordinary closed-panel frame must continue the exit"
        );
        if focus_loss {
            app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        } else {
            app.world_mut()
                .resource_mut::<UiRuntimeResource>()
                .begin_session(2);
        }
        app.update();
        assert!(
            !app.world()
                .resource::<UiPresentationResource>()
                .mod_ui()
                .mod_hud_editor_painting()
        );
        assert!(
            app.world()
                .resource::<ModRuntime>()
                .hud_editor_owner
                .is_none()
        );
    }
}

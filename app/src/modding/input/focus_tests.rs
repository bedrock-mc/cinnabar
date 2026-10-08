use super::*;
use crate::ui_runtime::presentation::forms::tests::mini_engine_presentation;
use bevy::input::keyboard::Key;

fn fixture() -> String {
    let package = include_str!("../../../../crates/mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let panel = r#"{"title":"Local controls","toggle_key":"ShiftRight","dark":true,"controls":[{"kind":"slider","id":"strength","label":"Strength","value":35,"min":0,"max":100,"step":1}]}"#;
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
    player: &crate::player_runtime::PlayerRuntime,
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
    let player = crate::player_runtime::PlayerRuntime::new(1);
    let ui = UiRuntime::new(1);
    let mut presentation = mini_engine_presentation();
    presentation.set_mod_panel(host.panel()).unwrap();
    presentation.set_mod_panel_open(true);
    render(&mut presentation, &player, &ui);
    // Locate the rendered numeric editor through its public pointer routing,
    // rather than coupling this app regression to private layout hit regions.
    'search: for y in (0..720).step_by(8) {
        for x in (0..1280).step_by(8) {
            if !presentation.mod_panel_open() {
                presentation.set_mod_panel_open(true);
                render(&mut presentation, &player, &ui);
            }
            presentation.mod_panel_events([x as f32, y as f32], true, true);
            if presentation.mod_panel_editing() {
                break 'search;
            }
        }
    }
    assert!(
        presentation.mod_panel_editing(),
        "rendered numeric input was not reachable"
    );
    presentation.mod_panel_key("Digit4", Some("42"));
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(player)
        .insert_resource(ui)
        .insert_resource(presentation)
        .insert_resource(ModRuntime {
            host,
            companions: Vec::new(),
            label: None,
            label_inputs: Vec::new(),
            label_rebuilds: 0,
            render_sources: Vec::new(),
            render_merge: Default::default(),
            last_reload: std::time::Instant::now(),
            controls: mod_host::empty_controls(),
            reload_on_main: false,
            registration_identity: None,
            registration_request: None,
            suspended: false,
            hud_editor_owner: None,
        })
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
    assert!(app.world().resource::<ModRuntime>().host.panel_open());
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
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
    assert!(app.world().resource::<ModRuntime>().host.panel_open());
    assert!(
        app.world()
            .resource::<ModRuntime>()
            .controls
            .keys_pressed
            .is_empty()
    );
    assert_eq!(
        app.world_mut()
            .resource_mut::<UiPresentationRuntime>()
            .mod_panel_key("Enter", None),
        vec![ui::mod_panel::Event {
            id: "strength".into(),
            value: 42.0
        }]
    );
}

fn hud_editor_app() -> (App, Entity) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hud-editor.component.wat");
    let mut host = mod_host::ModHost::load_snapshot_with_grants(
        &path,
        fixture().as_bytes(),
        mod_host::ModGrants {
            hud: true,
            controls: true,
            ..Default::default()
        },
    )
    .unwrap();
    host.set_panel_open(true);
    let player = crate::player_runtime::PlayerRuntime::new(1);
    let ui = UiRuntime::new(1);
    let mut presentation = mini_engine_presentation();
    presentation.set_mod_panel(host.panel()).unwrap();
    presentation.set_mod_panel_open(true);
    let preview:ui::mod_hud::Hud=serde_json::from_str(r#"{"cards":[{"id":"equipment","position":[0.5,0.5],"rows":[{"label":"Helmet","value":"85%"}]}]}"#).unwrap();
    presentation.open_mod_hud_editor(&preview).unwrap();
    render(&mut presentation, &player, &ui);
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(player)
        .insert_resource(ui)
        .insert_resource(presentation)
        .insert_resource(ModRuntime {
            host,
            companions: Vec::new(),
            label: None,
            label_inputs: Vec::new(),
            label_rebuilds: 0,
            render_sources: Vec::new(),
            render_merge: Default::default(),
            last_reload: std::time::Instant::now(),
            controls: mod_host::empty_controls(),
            reload_on_main: false,
            registration_identity: None,
            registration_request: None,
            suspended: false,
            hud_editor_owner: Some(super::super::hud_editor::Owner {
                host: 0,
                session: 1,
            }),
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
    let mut presentation = app.world_mut().resource_mut::<UiPresentationRuntime>();
    presentation.mod_panel_key("Enter", None);
    let result = presentation.take_mod_hud_editor_result().unwrap();
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
            app.world_mut().resource_mut::<UiRuntime>().begin_session(2);
        }
        app.update();
        let p = app.world().resource::<UiPresentationRuntime>();
        assert!(!p.mod_hud_editor_open());
        assert!(!p.mod_panel_open());
        let runtime = app.world().resource::<ModRuntime>();
        assert!(runtime.hud_editor_owner.is_none());
        assert!(!runtime.host.panel_open());
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
        let p = app.world().resource::<UiPresentationRuntime>();
        assert!(!p.mod_hud_editor_open());
        assert!(p.mod_panel_open());
        let runtime = app.world().resource::<ModRuntime>();
        assert!(runtime.hud_editor_owner.is_none());
        assert!(runtime.host.panel_open());
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
        let old = std::mem::replace(&mut runtime.host, empty_host);
        runtime
            .companions
            .push(super::super::multi::Companion { host: old });
        runtime.hud_editor_owner.as_mut().unwrap().host = 1;
        assert_eq!(runtime.panel_owner(), 1);
    }
    app.update();
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
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
    app.world_mut().resource_mut::<ModRuntime>().host = new_host;
    app.update();
    assert!(
        !app.world()
            .resource::<UiPresentationRuntime>()
            .mod_hud_editor_open()
    );
    assert!(
        !app.world()
            .resource::<UiPresentationRuntime>()
            .mod_panel_open()
    );
    let runtime = app.world().resource::<ModRuntime>();
    assert!(runtime.hud_editor_owner.is_none());
    assert_eq!(runtime.panel_owner(), 0);
    assert!(runtime.host.is_active() && runtime.companions[0].host.is_active());
    assert!(
        !runtime.companions[0].host.panel_open(),
        "old owner cannot reopen stale input ownership"
    );
    assert!(!runtime.controls.panel_open);
}

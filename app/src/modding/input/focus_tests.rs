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

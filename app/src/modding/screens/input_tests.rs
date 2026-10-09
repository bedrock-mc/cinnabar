//! Input regressions across the real inventory suppression and mod-screen paths.
use super::*;
use crate::ui_runtime::drive_chat_keyboard_input;
use bevy::window::WindowEvent;
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, NativeKey},
        mouse::AccumulatedMouseMotion,
    },
    time::Real,
    window::{CursorOptions, PrimaryWindow},
};
use client_ui::test_support::{fixture_font, inventory_session};

const MOD_ID: &str = "input_test";
const VIEW: &str = "ui/view.json";
const ACTION: &str = "input_test.show";
const EMPTY_COMPONENT: &str = r#"(component (core module $m (func (export "init")) (func (export "frame"))) (core instance $i (instantiate $m)) (func (export "init") (canon lift (core func $i "init"))) (func (export "frame") (canon lift (core func $i "frame"))))"#;

#[derive(Resource, Default)]
struct Observed {
    modifiers: KeyModifiers,
    keys: Vec<ModEvent>,
    pointer: Option<[f32; 2]>,
    declarations: Option<Vec<KeyDecl>>,
}
#[derive(Resource)]
struct Host(ModHost);

/// Creates a focused inventory with the production keyboard suppression system.
fn input_app() -> (App, Entity) {
    let mut player = crate::player_runtime::PlayerRuntime::new(1);
    let mut ui = inventory_session(&mut player);
    ui.toggle_inventory(&mut player);
    assert!(ui.inventory_open());
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .add_message::<MouseWheel>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Observed>()
        .insert_resource(player)
        .insert_resource(ui)
        .insert_resource(UiPresentationRuntime::new(fixture_font()).unwrap());
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    (app, window)
}

/// Sends one physical keyboard edge and lets the input systems process it.
fn send(app: &mut App, window: Entity, key_code: KeyCode, state: ButtonState, text: Option<&str>) {
    queue_key(app, window, key_code, state, text);
    app.update();
}

/// Queues a physical key in both streams when ordered window input is available.
fn queue_key(
    app: &mut App,
    window: Entity,
    key_code: KeyCode,
    state: ButtonState,
    text: Option<&str>,
) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    if state.is_pressed() {
        keys.press(key_code);
    } else {
        keys.release(key_code);
    }
    let event = KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: text.map(Into::into),
        repeat: false,
        window,
    };
    if app
        .world()
        .contains_resource::<bevy::ecs::message::Messages<WindowEvent>>()
    {
        app.world_mut()
            .write_message(WindowEvent::KeyboardInput(event.clone()));
    }
    app.world_mut().write_message(event);
}

/// Observes the modifiers that the mod adapter reads after inventory suppression.
fn observe_modifiers(
    mut input: ScreenInput,
    windows: Query<&Window>,
    mut observed: ResMut<Observed>,
) {
    observed.modifiers = input.modifiers(windows.single().unwrap().focused);
}

#[test]
fn mod_modifiers_survive_inventory_suppression_and_release_independently() {
    let (mut app, window) = input_app();
    app.add_systems(
        Update,
        (drive_chat_keyboard_input, observe_modifiers).chain(),
    );
    for key in [
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::ShiftLeft,
        KeyCode::AltRight,
    ] {
        send(&mut app, window, key, ButtonState::Pressed, None);
    }
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .get_pressed()
            .next()
            .is_none()
    );
    assert_eq!(
        app.world().resource::<Observed>().modifiers,
        KeyModifiers {
            ctrl: true,
            shift: true,
            alt: true
        }
    );
    send(
        &mut app,
        window,
        KeyCode::ControlLeft,
        ButtonState::Released,
        None,
    );
    app.update();
    assert!(app.world().resource::<Observed>().modifiers.ctrl);
    for key in [KeyCode::ControlRight, KeyCode::ShiftLeft, KeyCode::AltRight] {
        send(&mut app, window, key, ButtonState::Released, None);
    }
    assert_eq!(
        app.world().resource::<Observed>().modifiers,
        KeyModifiers::default()
    );
    send(
        &mut app,
        window,
        KeyCode::ControlLeft,
        ButtonState::Pressed,
        None,
    );
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    assert_eq!(
        app.world().resource::<Observed>().modifiers,
        KeyModifiers::default()
    );
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    assert_eq!(
        app.world().resource::<Observed>().modifiers,
        KeyModifiers::default()
    );
}

/// Draws an original mod view over the inventory using the installed UI fixture.
fn draw_view(app: &mut App) -> bool {
    let files = Arc::new(server_experience::screen::Files {
        namespace: MOD_ID.into(),
        templates: [(VIEW.into(), serde_json::to_vec(&serde_json::json!({"namespace":MOD_ID,"view":{"type":"panel","size":[100,80],"controls":[{"title":{"type":"label","text":"View"}}]}})).unwrap())].into(),
        textures: Vec::new(),
    });
    if !draw_package_view(app, MOD_ID, &files) {
        return false;
    }
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .mod_view_shown()
    );
    true
}

/// Draws a package snapshot over the inventory using the installed UI fixture.
fn draw_package_view(
    app: &mut App,
    id: &str,
    files: &Arc<server_experience::screen::Files>,
) -> bool {
    let Some(mut presentation) = client_ui::test_support::engine_presentation() else {
        eprintln!(
            "skipping {}: missing installed UI carrier (make assets)",
            std::thread::current()
                .name()
                .unwrap_or("mod screen input test")
        );
        return false;
    };
    let view = VIEW.to_owned();
    presentation.set_mod_screens(Some(ModScreensInput {
        id,
        files,
        overlay: None,
        view: Some(&view),
        focus: None,
        data: &server_experience::screen::Modal::default(),
    }));
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
    app.insert_resource(presentation);
    true
}

/// Delivers declared mod keys through the production adapter after vanilla input.
fn observe_keys(
    mut input: ScreenInput,
    player: Res<crate::player_runtime::PlayerRuntime>,
    ui: Res<UiRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut observed: ResMut<Observed>,
) {
    let modifiers = input.modifiers(true);
    let cursor = observed.pointer;
    let pressed = pointer_events(
        &mut presentation,
        cursor,
        modifiers,
        &mut input,
        &mut observed.keys,
    );
    let keys = observed.declarations.clone().unwrap_or_else(|| {
        vec![KeyDecl {
            id: ACTION.into(),
            key: "r".into(),
            modifiers: Vec::new(),
            label: "Show".into(),
        }]
    });
    key_events(
        &player,
        &ui,
        &mut presentation,
        KeyFrame {
            keys: &keys,
            menu: None,
            cursor,
            pressed,
        },
        &mut input,
        &mut observed.keys,
    );
}

#[test]
fn hidden_vanilla_text_fields_do_not_receive_typing_under_a_mod_view() {
    for anvil in [false, true] {
        let (mut app, window) = input_app();
        if !draw_view(&mut app) {
            return;
        }
        let mut ui = app.world_mut().resource_mut::<UiRuntime>();
        ui.screen_state_mut().search = "Search".into();
        ui.screen_state_mut().anvil_name = "Name".into();
        ui.screen_state_mut().search_focused = !anvil;
        ui.screen_state_mut().anvil_focused = anvil;
        app.add_systems(Update, drive_chat_keyboard_input);
        send(
            &mut app,
            window,
            KeyCode::KeyR,
            ButtonState::Pressed,
            Some("r"),
        );
        let ui = app.world().resource::<UiRuntime>();
        assert_eq!(ui.screen_state().search, "Search");
        assert_eq!(ui.screen_state().anvil_name, "Name");
        assert!(ui.inventory_open());
    }
}

#[test]
fn a_shown_mod_view_releases_declared_keys_from_hidden_vanilla_text_focus() {
    let (mut app, window) = input_app();
    if !draw_view(&mut app) {
        return;
    }
    app.world_mut()
        .resource_mut::<UiRuntime>()
        .screen_state_mut()
        .search_focused = true;
    let dir = tempfile::tempdir().unwrap();
    let component = dir.path().join("empty.wat");
    std::fs::write(&component, EMPTY_COMPONENT).unwrap();
    app.insert_resource(Host(ModHost::load(&component).unwrap()));
    app.add_systems(Update, (drive_chat_keyboard_input, observe_keys).chain());
    send(&mut app, window, KeyCode::KeyR, ButtonState::Pressed, None);
    assert_eq!(
        app.world().resource::<Observed>().keys,
        [ModEvent::Key {
            id: ACTION.into(),
            hovered: None,
            row: None
        }]
    );
    app.world_mut().resource_mut::<Observed>().keys.clear();
    app.world_mut()
        .resource_mut::<UiPresentationRuntime>()
        .set_mod_screens(None);
    send(
        &mut app,
        window,
        KeyCode::KeyR,
        ButtonState::Pressed,
        Some("r"),
    );
    assert!(app.world().resource::<Observed>().keys.is_empty());
    assert_eq!(
        app.world().resource::<UiRuntime>().screen_state().search,
        "r"
    );
}

#[derive(Resource, Default)]
struct OwnerState(ScreenState);

/// Runs the real owner's adapter with no focused pointer or keyboard input.
fn exercise_owner(
    mut input: ScreenInput,
    mut host: ResMut<Host>,
    mut state: ResMut<OwnerState>,
    player: Res<crate::player_runtime::PlayerRuntime>,
    ui: Res<UiRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
) {
    drive_owner(
        &mut host.0,
        &mut state.0,
        &player,
        &ui,
        &mut presentation,
        None,
        None,
        false,
        &mut input,
    );
}

/// Writes a hash-checked empty component with an original valid or unresolvable template.
fn write_screen_package(dir: &std::path::Path, id: &str, rejected: bool) {
    use sha2::{Digest, Sha256};
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    let root = if rejected {
        "view@absent_fixture.panel"
    } else {
        "view"
    };
    let template = serde_json::to_vec(
        &serde_json::json!({"namespace":id, root:{"type":"panel","size":[100,80]}}),
    )
    .unwrap();
    let files = [
        ("mod.wasm", EMPTY_COMPONENT.as_bytes()),
        (VIEW, template.as_slice()),
    ];
    let mut hashes = String::new();
    for (path, bytes) in files {
        std::fs::write(dir.join(path), bytes).unwrap();
        let hash: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        hashes.push_str(&format!("\"{path}\" = \"{hash}\"\n"));
    }
    let api = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/mods/screen-probe/mod.toml"
    ))
    .lines()
    .find(|line| line.starts_with("api ="))
    .unwrap();
    std::fs::write(dir.join(mod_host::package::MANIFEST), format!("id = \"{id}\"\nversion = \"0.1.0\"\n{api}\npermissions = [\"screen\"]\ntemplates = [\"{VIEW}\"]\n[files]\n{hashes}")).unwrap();
}

#[test]
fn a_replacements_owner_does_not_inherit_a_previous_packages_presentation_failure() {
    for replacement in ["reload", "owner", "same"] {
        let (mut app, _) = input_app();
        let dir = tempfile::tempdir().unwrap();
        write_screen_package(dir.path(), MOD_ID, true);
        let mut host = ModHost::load_package(dir.path(), mod_host::ModGrants::default()).unwrap();
        let files = Arc::clone(&host.package().unwrap().files);
        if !draw_package_view(&mut app, MOD_ID, &files) {
            return;
        }
        let presentation = app.world().resource::<UiPresentationRuntime>();
        assert!(presentation.mod_screens_failure().is_some());
        let layout = presentation.mod_screen_layout().cloned();
        assert!(layout.is_some());
        if replacement == "reload" {
            write_screen_package(dir.path(), MOD_ID, false);
            assert!(host.reload_if_changed().unwrap());
        } else if replacement == "owner" {
            let next = dir.path().join("replacement");
            write_screen_package(&next, "replacement", false);
            host = ModHost::load_package(&next, mod_host::ModGrants::default()).unwrap();
        }
        app.insert_resource(Host(host));
        app.insert_resource(OwnerState(ScreenState {
            layout,
            ..Default::default()
        }));
        app.add_systems(Update, exercise_owner);
        app.update();
        assert_eq!(
            app.world().resource::<Host>().0.is_active(),
            replacement != "same",
            "{replacement}"
        );
        if replacement != "same" {
            assert!(app.world().resource::<OwnerState>().0.layout.is_some());
            assert!(
                app.world()
                    .resource::<UiPresentationRuntime>()
                    .mod_screens_failure()
                    .is_none()
            );
        }
    }
}

/// Builds and encodes the existing screen probe once in this worktree's test target.
fn transition_probe_component() -> &'static [u8] {
    static COMPONENT: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    COMPONENT.get_or_init(|| {
        let target = std::env::current_exe()
            .unwrap()
            .ancestors()
            .nth(3)
            .unwrap()
            .join("screen-transition-probe");
        let output =
            std::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
                .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
                .args([
                    "build",
                    "--locked",
                    "--target",
                    "wasm32-unknown-unknown",
                    "-p",
                    "screen-probe-mod",
                    "--target-dir",
                ])
                .arg(&target)
                .output()
                .unwrap();
        assert!(
            output.status.success(),
            "screen probe build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let module =
            std::fs::read(target.join("wasm32-unknown-unknown/debug/screen_probe_mod.wasm"))
                .unwrap();
        wit_component::ComponentEncoder::default()
            .module(&module)
            .unwrap()
            .validate(true)
            .encode()
            .unwrap()
    })
}

/// Writes the existing probe package with hashes and a chosen version for reload tests.
fn write_transition_probe(dir: &std::path::Path, version: &str) {
    use sha2::{Digest, Sha256};
    let source = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../examples/mods/screen-probe"
    ));
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    let mut hashes = String::new();
    for (path, bytes) in [
        ("mod.wasm", transition_probe_component().to_vec()),
        (
            "ui/overlay.json",
            std::fs::read(source.join("ui/overlay.json")).unwrap(),
        ),
        (VIEW, std::fs::read(source.join(VIEW)).unwrap()),
    ] {
        std::fs::write(dir.join(path), &bytes).unwrap();
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        hashes.push_str(&format!("\"{path}\" = \"{hash}\"\n"));
    }
    let manifest = std::fs::read_to_string(source.join(mod_host::package::MANIFEST))
        .unwrap()
        .lines()
        .map(|line| {
            if line.starts_with("version =") {
                format!("version = \"{version}\"")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(
        dir.join(mod_host::package::MANIFEST),
        format!("{manifest}\n{hashes}"),
    )
    .unwrap();
}

#[test]
fn a_package_reload_keeps_the_open_container_and_restored_view() {
    let (mut app, _) = input_app();
    let dir = tempfile::tempdir().unwrap();
    write_transition_probe(dir.path(), "0.1.0");
    let mut host = ModHost::load_package(dir.path(), mod_host::ModGrants::default()).unwrap();
    let package = host.package().unwrap();
    if !draw_package_view(&mut app, &package.id, &package.files) {
        return;
    }
    let layout = app
        .world()
        .resource::<UiPresentationRuntime>()
        .mod_screen_layout()
        .cloned();
    assert!(layout.is_some());
    host.dispatch(vec![
        ModEvent::ScreenChanged(layout.clone()),
        ModEvent::Action {
            id: "probe.view".into(),
            index: None,
        },
    ])
    .unwrap();
    assert!(host.screens().view.is_some());
    write_transition_probe(dir.path(), "0.1.1");
    assert!(host.reload_if_changed().unwrap());
    host.dispatch(vec![ModEvent::Action {
        id: "probe.view".into(),
        index: None,
    }])
    .unwrap();
    app.insert_resource(Host(host));
    app.insert_resource(OwnerState(ScreenState {
        layout,
        ..Default::default()
    }));
    app.add_systems(Update, exercise_owner);
    app.update();
    let host = &app.world().resource::<Host>().0;
    assert!(
        host.screens().view.is_some(),
        "reload reported a container closure"
    );
    assert_eq!(
        host.screens().data.values.get("#layout_open"),
        Some(&server_experience::screen::Value::Bool(true))
    );
}

/// Exercises ownership transitions through the production multi-mod adapter.
fn exercise_runtime(
    mut input: ScreenInput,
    mut runtime: ResMut<ModRuntime>,
    player: Res<crate::player_runtime::PlayerRuntime>,
    ui: Res<UiRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
) {
    drive(
        &mut runtime,
        &player,
        &ui,
        &mut presentation,
        None,
        None,
        false,
        &mut input,
    );
}

#[test]
fn a_departing_screen_owner_closes_its_view_and_forgets_the_container() {
    let (mut app, _) = input_app();
    let dir = tempfile::tempdir().unwrap();
    write_transition_probe(dir.path(), "0.1.0");
    let first = ModHost::load_package(dir.path(), mod_host::ModGrants::default()).unwrap();
    let mut previous = ModHost::load_package(dir.path(), mod_host::ModGrants::default()).unwrap();
    let package = previous.package().unwrap();
    if !draw_package_view(&mut app, &package.id, &package.files) {
        return;
    }
    let layout = app
        .world()
        .resource::<UiPresentationRuntime>()
        .mod_screen_layout()
        .cloned();
    previous
        .dispatch(vec![
            ModEvent::ScreenChanged(layout.clone()),
            ModEvent::Action {
                id: "probe.view".into(),
                index: None,
            },
        ])
        .unwrap();
    assert!(previous.screens().view.is_some());
    let mut configured = App::new();
    super::super::install(&mut configured, vec![first, previous]);
    let mut runtime = configured
        .world_mut()
        .remove_resource::<ModRuntime>()
        .unwrap();
    runtime.screens = ScreenState {
        owner: Some(1),
        layout,
        ..Default::default()
    };
    app.insert_resource(runtime);
    app.add_systems(Update, exercise_runtime);
    app.update();
    let runtime = app.world().resource::<ModRuntime>();
    let previous = runtime.host(1);
    assert!(
        previous.screens().view.is_none(),
        "departing owner retained its view"
    );
    assert_eq!(
        previous.screens().data.values.get("#layout_open"),
        Some(&server_experience::screen::Value::Bool(false))
    );
    assert_eq!(
        previous.screens().data.values.get("#view_closed"),
        Some(&server_experience::screen::Value::Integer(1))
    );
}

#[test]
fn data_callbacks_observe_simultaneous_container_opening_and_closing() {
    for open in [true, false] {
        let (mut app, _) = input_app();
        let dir = tempfile::tempdir().unwrap();
        write_transition_probe(dir.path(), "0.1.0");
        let mut host = ModHost::load_package(dir.path(), mod_host::ModGrants::default()).unwrap();
        let files = Arc::clone(&host.package().unwrap().files);
        if !draw_package_view(&mut app, "probe", &files) {
            return;
        }
        let layout = app
            .world()
            .resource::<UiPresentationRuntime>()
            .mod_screen_layout()
            .cloned();
        assert!(layout.is_some());
        if !open {
            host.dispatch(vec![ModEvent::ScreenChanged(layout.clone())])
                .unwrap();
            app.world_mut().resource_scope(
                |world, mut player: Mut<crate::player_runtime::PlayerRuntime>| {
                    world
                        .resource_mut::<UiRuntime>()
                        .toggle_inventory(&mut player);
                },
            );
            app.world_mut().resource_scope(
                |world, mut presentation: Mut<UiPresentationRuntime>| {
                    presentation
                        .build(
                            world.resource::<crate::player_runtime::PlayerRuntime>(),
                            world.resource::<UiRuntime>(),
                            0,
                            [1280, 720],
                            ui::DpiScale::new(1.0).unwrap(),
                        )
                        .unwrap();
                },
            );
        }
        let mut configured = App::new();
        super::super::install(&mut configured, vec![host]);
        let mut runtime = configured
            .world_mut()
            .remove_resource::<ModRuntime>()
            .unwrap();
        runtime.screens.owner = Some(0);
        runtime.screens.layout = if open { None } else { layout };
        app.insert_resource(runtime);
        app.add_systems(Update, exercise_runtime);
        app.update();
        let host = app.world().resource::<ModRuntime>().host(0);
        assert_eq!(
            host.screens().data.values.get("#data_layout_open"),
            Some(&server_experience::screen::Value::Bool(open)),
            "data callback used the previous layout while open={open}"
        );
        assert_eq!(
            host.screens().data.values.get("#layout_open"),
            Some(&server_experience::screen::Value::Bool(open))
        );
    }
}

/// Draws an original edit box and returns a window-logical point inside it.
fn draw_edit_box(app: &mut App) -> Option<[f32; 2]> {
    let template = serde_json::json!({"namespace":MOD_ID,"view":{
        "type":"panel","size":["100%","100%"],"controls":[{"search":{
            "type":"edit_box","size":[80,20],"offset":[20,20],
            "anchor_from":"top_left","anchor_to":"top_left",
            "text_box_name":"input_test.search","max_length":32,"text_control":"display",
            "button_mappings":[
                {"from_button_id":"button.menu_select","to_button_id":"button.text_edit_box_selected",
                    "handle_select":true,"handle_deselect":false,"mapping_type":"pressed"},
                {"from_button_id":"button.menu_select","to_button_id":"button.text_edit_box_selected",
                    "handle_select":false,"handle_deselect":true,"mapping_type":"global","consume_event":false},
                {"from_button_id":"button.menu_cancel","to_button_id":"button.text_edit_box_deselected",
                    "handle_select":false,"handle_deselect":true,"mapping_type":"global"}],
            "controls":[{"display":{"type":"label","text":"","size":[80,20]}}]}}]}});
    let files = Arc::new(server_experience::screen::Files {
        namespace: MOD_ID.into(),
        templates: [(VIEW.into(), serde_json::to_vec(&template).unwrap())].into(),
        textures: Vec::new(),
    });
    if !draw_package_view(app, MOD_ID, &files) {
        return None;
    }
    let presentation = app.world().resource::<UiPresentationRuntime>();
    assert!(!presentation.mod_text_focused());
    let scale = presentation.mod_screen_layout().unwrap().size.scale as f32;
    Some([30.0 * scale, 25.0 * scale])
}

#[test]
fn clicking_a_mod_edit_box_routes_same_update_typing_without_a_shortcut() {
    let (mut app, window) = input_app();
    let Some(point) = draw_edit_box(&mut app) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let component = dir.path().join("empty.wat");
    std::fs::write(&component, EMPTY_COMPONENT).unwrap();
    app.insert_resource(Host(ModHost::load(&component).unwrap()));
    app.world_mut().resource_mut::<Observed>().pointer = Some(point);
    app.add_systems(Update, observe_keys);
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    send(
        &mut app,
        window,
        KeyCode::KeyR,
        ButtonState::Pressed,
        Some("r"),
    );
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .mod_text_focused(),
        "fixture click did not select its edit box"
    );
    assert_eq!(
        app.world().resource::<Observed>().keys,
        [ModEvent::TextChanged {
            control: "input_test.search".into(),
            text: "r".into(),
        }]
    );
}

#[test]
fn click_focus_precedes_vanilla_inventory_shortcuts() {
    for ordered in [false, true] {
        let (mut app, window) = input_app();
        let Some(point) = draw_edit_box(&mut app) else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let component = dir.path().join("empty.wat");
        std::fs::write(&component, EMPTY_COMPONENT).unwrap();
        app.insert_resource(Host(ModHost::load(&component).unwrap()));
        app.world_mut().resource_mut::<Observed>().pointer = Some(point);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(Vec2::from_array(point)));
        if ordered {
            app.add_message::<WindowEvent>();
        }
        app.add_systems(Update, (drive_chat_keyboard_input, observe_keys).chain());
        let click = MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window,
        };
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut().write_message(click);
        if ordered {
            app.world_mut()
                .write_message(WindowEvent::MouseButtonInput(click));
        }
        send(
            &mut app,
            window,
            KeyCode::KeyE,
            ButtonState::Pressed,
            Some("e"),
        );
        assert!(
            app.world().resource::<UiRuntime>().inventory_open(),
            "click-plus-typing closed the inventory with ordered={ordered}"
        );
        assert_eq!(
            app.world().resource::<Observed>().keys,
            [ModEvent::TextChanged {
                control: "input_test.search".into(),
                text: "e".into(),
            }]
        );
    }
}

#[test]
fn declared_keys_use_modifiers_at_each_press() {
    for (edges, expected) in [
        (
            vec![
                (KeyCode::ControlLeft, ButtonState::Pressed, None),
                (KeyCode::KeyR, ButtonState::Pressed, Some("r")),
                (KeyCode::ControlLeft, ButtonState::Released, None),
            ],
            "control",
        ),
        (
            vec![
                (KeyCode::KeyR, ButtonState::Pressed, Some("r")),
                (KeyCode::ControlLeft, ButtonState::Pressed, None),
            ],
            "plain",
        ),
    ] {
        let (mut app, window) = input_app();
        if !draw_view(&mut app) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let component = dir.path().join("empty.wat");
        std::fs::write(&component, EMPTY_COMPONENT).unwrap();
        app.insert_resource(Host(ModHost::load(&component).unwrap()));
        app.world_mut().resource_mut::<Observed>().declarations = Some(vec![
            KeyDecl {
                id: "control".into(),
                key: "r".into(),
                modifiers: vec![Modifier::Ctrl],
                label: "Control".into(),
            },
            KeyDecl {
                id: "plain".into(),
                key: "r".into(),
                modifiers: Vec::new(),
                label: "Plain".into(),
            },
        ]);
        app.add_systems(Update, observe_keys);
        for (key, state, text) in edges {
            queue_key(&mut app, window, key, state, text);
        }
        app.update();
        assert_eq!(
            app.world().resource::<Observed>().keys,
            [ModEvent::Key {
                id: expected.into(),
                hovered: None,
                row: None,
            }]
        );
    }
}

#[test]
fn production_mod_driver_delivers_declared_keys_and_view_escape() {
    let (mut app, window) = input_app();
    let dir = tempfile::tempdir().unwrap();
    write_transition_probe(dir.path(), "0.1.0");
    let mut host = ModHost::load_package(dir.path(), mod_host::ModGrants::default()).unwrap();
    let package = host.package().unwrap();
    if !draw_package_view(&mut app, &package.id, &package.files) {
        return;
    }
    let layout = app
        .world()
        .resource::<UiPresentationRuntime>()
        .mod_screen_layout()
        .cloned();
    assert!(layout.is_some());
    host.dispatch(vec![
        ModEvent::ScreenChanged(layout),
        ModEvent::Action {
            id: "probe.view".into(),
            index: None,
        },
    ])
    .unwrap();
    assert!(host.screens().view.is_some());
    let mut configured = App::new();
    super::super::install(&mut configured, vec![host]);
    app.insert_resource(
        configured
            .world_mut()
            .remove_resource::<ModRuntime>()
            .unwrap(),
    )
    .insert_resource(crate::environment::VisualTimeOverride(None))
    .init_resource::<super::super::interaction::ModInteraction>()
    .add_systems(
        Update,
        (drive_chat_keyboard_input, super::super::drive_mod).chain(),
    );
    send(&mut app, window, KeyCode::KeyR, ButtonState::Pressed, None);
    assert!(matches!(
        app.world().resource::<ModRuntime>().host.screens().data.values.get("#key"),
        Some(server_experience::screen::Value::Text(key)) if key.starts_with("probe.show:")
    ));
    send(
        &mut app,
        window,
        KeyCode::Escape,
        ButtonState::Pressed,
        None,
    );
    let runtime = app.world().resource::<ModRuntime>();
    assert!(runtime.host.screens().view.is_none());
    assert_eq!(
        runtime.host.screens().data.values.get("#view_closed"),
        Some(&server_experience::screen::Value::Integer(1))
    );
}

#[test]
fn escape_closes_a_view_after_same_frame_pointer_actions() {
    use sha2::{Digest, Sha256};
    let (mut app, window) = input_app();
    let dir = tempfile::tempdir().unwrap();
    write_transition_probe(dir.path(), "0.1.0");
    let template = serde_json::to_vec(&serde_json::json!({"namespace":"probe","view":{
        "type":"panel","size":["100%","100%"],"controls":[{"open":{
            "type":"button","size":[80,20],"offset":[20,20],
            "anchor_from":"top_left","anchor_to":"top_left",
            "button_mappings":[{"from_button_id":"button.menu_select",
                "to_button_id":"probe.view","mapping_type":"pressed"}]}}]}}))
    .unwrap();
    let old = std::fs::read(dir.path().join(VIEW)).unwrap();
    std::fs::write(dir.path().join(VIEW), &template).unwrap();
    let manifest_path = dir.path().join(mod_host::package::MANIFEST);
    let manifest = std::fs::read_to_string(&manifest_path).unwrap().replace(
        &format!("{:x}", Sha256::digest(old)),
        &format!("{:x}", Sha256::digest(template)),
    );
    std::fs::write(manifest_path, manifest).unwrap();
    let mut host = ModHost::load_package(dir.path(), mod_host::ModGrants::default()).unwrap();
    let package = host.package().unwrap();
    if !draw_package_view(&mut app, &package.id, &package.files) {
        return;
    }
    let layout = app
        .world()
        .resource::<UiPresentationRuntime>()
        .mod_screen_layout()
        .cloned();
    let scale = layout.as_ref().unwrap().size.scale as f32;
    host.dispatch(vec![
        ModEvent::ScreenChanged(layout),
        ModEvent::Action {
            id: "probe.view".into(),
            index: None,
        },
        ModEvent::Action {
            id: "probe.read_all".into(),
            index: None,
        },
    ])
    .unwrap();
    assert!(host.screens().view.is_some());
    let mut configured = App::new();
    super::super::install(&mut configured, vec![host]);
    app.insert_resource(
        configured
            .world_mut()
            .remove_resource::<ModRuntime>()
            .unwrap(),
    )
    .insert_resource(crate::environment::VisualTimeOverride(None))
    .init_resource::<super::super::interaction::ModInteraction>()
    .add_systems(
        Update,
        (drive_chat_keyboard_input, super::super::drive_mod).chain(),
    );
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(30.0 * scale, 25.0 * scale)));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window,
    });
    send(
        &mut app,
        window,
        KeyCode::Escape,
        ButtonState::Pressed,
        None,
    );
    let runtime = app.world().resource::<ModRuntime>();
    assert!(matches!(runtime.host.screens().data.values.get("#action"),
        Some(server_experience::screen::Value::Text(action)) if action == "probe.view:None:Ok(())"));
    assert!(
        runtime.host.screens().view.is_none(),
        "a queued pointer action reopened the escaped view"
    );
    assert_eq!(
        runtime.host.screens().data.values.get("#view_closed"),
        Some(&server_experience::screen::Value::Integer(1))
    );
}

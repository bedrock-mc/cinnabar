//! Real carrier, Bevy pointer/keyboard input, rendered fields and persisted endpoints.

use std::sync::{Arc, Mutex};

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    window::{CursorOptions, PrimaryWindow, WindowResolution},
};
use ui::{DpiScale, UiPoint, UiVisual};

use client_ui::ui_runtime::{
    UiRuntime,
    presentation::{UiPresentationRuntime, forms::snapshot},
};
use {
    super::*,
    launcher::menu::{MenuAction, MenuField, MenuScreen},
};

#[derive(Resource)]
struct Frame(render_model::UiRenderInput);

#[derive(Resource, Default)]
struct FrameTime(u64);

#[test]
fn idle_menu_does_not_repeat_cursor_os_notifications() {
    let Some(mut h) = Harness::new(1.0) else {
        return;
    };
    #[derive(Resource, Default)]
    struct CursorWrites(usize);
    h.app.init_resource::<CursorWrites>().add_systems(
        PostUpdate,
        |cursors: Query<(), Changed<CursorOptions>>, mut writes: ResMut<CursorWrites>| {
            writes.0 += cursors.iter().count();
        },
    );
    {
        let mut cursor = h
            .app
            .world_mut()
            .get_mut::<CursorOptions>(h.window)
            .unwrap();
        cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
        cursor.visible = false;
        cursor.hit_test = false;
    }
    h.app.update();
    assert_eq!(h.app.world().resource::<CursorWrites>().0, 1);
    for focused in [true, false, true] {
        h.app
            .world_mut()
            .get_mut::<Window>(h.window)
            .unwrap()
            .focused = focused;
        h.app.update();
        assert_eq!(h.app.world().resource::<CursorWrites>().0, 1);
        let cursor = h.app.world().get::<CursorOptions>(h.window).unwrap();
        assert_eq!(cursor.grab_mode, bevy::window::CursorGrabMode::None);
        assert!(cursor.visible && !cursor.hit_test);
    }
}

/// Publishes through the same presentation builder after the production input system.
fn publish(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    menu: Res<MenuRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut frame: ResMut<Frame>,
    runtime: Res<UiRuntime>,
    mut time: ResMut<FrameTime>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    time.0 += 16;
    presentation.set_menu_view(Some(menu.view()));
    let window = windows.single().unwrap();
    frame.0 = presentation
        .build(
            &player_runtime,
            &runtime,
            time.0,
            [window.physical_width(), window.physical_height()],
            DpiScale::new(window.scale_factor()).unwrap(),
        )
        .unwrap();
}

struct Harness {
    app: App,
    window: Entity,
    clipboard: Arc<Mutex<String>>,
    root: PathBuf,
}

impl Harness {
    /// Uses a temporary user directory; never touches the owner's saved servers.
    fn new(dpi: f32) -> Option<Self> {
        let player_runtime = crate::player_runtime::PlayerRuntime::new(1);
        let mut presentation = client_ui::test_support::pack_harness::engine_presentation()?;
        let frame = presentation
            .build(
                &player_runtime,
                &UiRuntime::new(1),
                0,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let root = client_ui::test_support::pack_harness::scratch_dir("server-input");
        let mut layout = launcher::test_support::checkout();
        layout.user_config_root = root.clone();
        let menu = MenuRuntime::new_with_layout(
            true,
            Some(2),
            "Test".into(),
            layout,
            crate::player_skin::LocalPlayerSkin::generated_default("Test"),
        );
        let clipboard = Arc::new(Mutex::new(String::new()));
        let read = clipboard.clone();
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Touches>()
            .insert_resource(player_runtime)
            .insert_resource(presentation)
            .insert_resource(menu)
            .insert_resource(Frame(frame))
            .insert_resource(client_ui::test_support::pack_harness::menu_runtime())
            .init_resource::<FrameTime>()
            .insert_resource(MenuClipboard::with_access(
                move |limit| {
                    let text = read.lock().unwrap().clone();
                    (text.len() <= limit).then_some(text)
                },
                |_| {},
            ))
            .add_systems(Update, (drive_menu_input, publish).chain());
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    resolution: WindowResolution::new((1280.0 * dpi) as u32, (720.0 * dpi) as u32)
                        .with_scale_factor_override(dpi),
                    ..Default::default()
                },
                CursorOptions::default(),
                PrimaryWindow,
            ))
            .id();
        Some(Self {
            app,
            window,
            clipboard,
            root,
        })
    }

    /// Sends an OS-style key event through the production Bevy reader and publishes its frame.
    fn key(&mut self, key_code: KeyCode, text: Option<&str>, state: ButtonState) {
        self.app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
            state,
            text: text.map(Into::into),
            repeat: false,
            window: self.window,
        });
        self.app.update();
    }

    /// Presses one key; modifier release is sent explicitly by `select_all`.
    fn press(&mut self, key: KeyCode, text: Option<&str>) {
        self.key(key, text, ButtonState::Pressed);
    }

    /// Finds a real hit region, moves the window cursor there and sends a left click.
    fn click(&mut self, action: MenuAction) {
        let presentation = self.app.world().resource::<UiPresentationRuntime>();
        let point = (0..720)
            .step_by(2)
            .flat_map(|y| {
                (0..1280)
                    .step_by(2)
                    .map(move |x| UiPoint::new(x as f32, y as f32).unwrap())
            })
            .find(|point| presentation.hit_test_menu(*point) == Some(action))
            .expect("visible hit target");
        self.app
            .world_mut()
            .get_mut::<Window>(self.window)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(point.x(), point.y())));
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        self.app.update();
        self.app.update();
    }

    /// Selects the focused field through the platform shortcut.
    fn select_all(&mut self) {
        self.press(KeyCode::SuperLeft, None);
        self.press(KeyCode::KeyA, Some("a"));
        self.key(KeyCode::SuperLeft, None, ButtonState::Released);
    }

    /// Pastes through the clipboard resource and real shortcut handling.
    fn paste(&mut self, text: &str) {
        *self.clipboard.lock().unwrap() = text.to_owned();
        self.press(KeyCode::SuperLeft, None);
        self.press(KeyCode::KeyV, Some("v"));
        self.key(KeyCode::SuperLeft, None, ButtonState::Released);
    }

    /// Checks each rendered field by its own hit rectangle, so swapped/stale text fails.
    fn fields(&self, expected: [&str; 3], focused: MenuField) {
        let presentation = self.app.world().resource::<UiPresentationRuntime>();
        let nodes = client_ui::test_support::pack_harness::menu_nodes(presentation);
        let caret = self.app.world().resource::<MenuRuntime>().view().caret;
        let selected = caret.selection.is_some();
        let (mut rendered_carets, mut literal_carets) = (0, 0);
        for (action, value) in [
            MenuAction::AddName,
            MenuAction::AddAddress,
            MenuAction::AddPort,
        ]
        .into_iter()
        .zip(expected)
        {
            let texts: Vec<_> = nodes
                .iter()
                .filter_map(|node| {
                    let UiVisual::Text { layout, .. } = node.visual() else {
                        return None;
                    };
                    let parent = nodes
                        .iter()
                        .find(|parent| Some(parent.id()) == node.parent())?;
                    let x = parent.bounds().min().x() + node.bounds().min().x() + 1.0;
                    let y = parent.bounds().min().y() + node.bounds().min().y() + 1.0;
                    (presentation.hit_test_menu(UiPoint::new(x, y).unwrap()) == Some(action)).then(
                        || {
                            layout
                                .glyphs()
                                .iter()
                                .map(|g| g.codepoint)
                                .collect::<String>()
                        },
                    )
                })
                .collect();
            let focused_action = match focused {
                MenuField::Name => MenuAction::AddName,
                MenuField::Address => MenuAction::AddAddress,
                MenuField::Port => MenuAction::AddPort,
                _ => unreachable!(),
            };
            let placeholder = match action {
                MenuAction::AddName => "addExternalServerScreen.namePlaceholder",
                MenuAction::AddAddress => "addExternalServerScreen.ipPlaceholder",
                _ => "",
            };
            let placeholder = client_ui::test_support::pack_harness::menu_translation(
                self.app.world().resource::<UiRuntime>(),
                placeholder,
            );
            let wanted = if value.is_empty() && action != focused_action {
                placeholder.as_deref().unwrap_or("")
            } else {
                value
            };
            literal_carets += wanted
                .chars()
                .filter(|&ch| ch == json_ui::CARET_GLYPH)
                .count();
            rendered_carets += texts
                .iter()
                .flat_map(|text| text.chars())
                .filter(|&ch| ch == json_ui::CARET_GLYPH)
                .count();
            let mut wanted = wanted.to_owned();
            if action == focused_action && !selected {
                wanted.insert(caret.byte, json_ui::CARET_GLYPH);
            }
            let actual: Vec<_> = texts
                .iter()
                .map(String::as_str)
                .filter(|text| !text.is_empty())
                .collect();
            let wanted: Vec<_> = (!wanted.is_empty())
                .then_some(wanted.as_str())
                .into_iter()
                .collect();
            if actual != wanted {
                eprintln!(
                    "field={:?} values={:?}",
                    self.app.world().resource::<MenuRuntime>().view().field,
                    [
                        &self.app.world().resource::<MenuRuntime>().view().name,
                        &self.app.world().resource::<MenuRuntime>().view().address
                    ]
                );
                client_ui::test_support::pack_harness::dump(nodes);
                snapshot::write(&self.app.world().resource::<Frame>().0, "input-failure");
            }
            assert_eq!(actual, wanted, "rendered value in {action:?}");
        }
        assert_eq!(
            self.app.world().resource::<MenuRuntime>().view().field,
            Some(focused)
        );
        let selections: Vec<_> = nodes
            .iter()
            .filter(|node| matches!(node.visual(), UiVisual::InvertedSprite { .. }))
            .collect();
        assert_eq!(
            rendered_carets,
            literal_carets + usize::from(!selected),
            "caret belongs only to the focused field"
        );
        assert_eq!(
            selections.len(),
            usize::from(selected),
            "select-all must be visible"
        );
        if let Some(selection) = selections.first() {
            let parent = nodes
                .iter()
                .find(|parent| Some(parent.id()) == selection.parent())
                .unwrap();
            let point = UiPoint::new(
                parent.bounds().min().x() + selection.bounds().min().x() + 1.0,
                parent.bounds().min().y() + selection.bounds().min().y() + 1.0,
            )
            .unwrap();
            let action = match focused {
                MenuField::Name => MenuAction::AddName,
                MenuField::Address => MenuAction::AddAddress,
                MenuField::Port => MenuAction::AddPort,
                _ => unreachable!(),
            };
            assert_eq!(presentation.hit_test_menu(point), Some(action));
        }
    }
}

impl Drop for Harness {
    /// Flushes queued writes before removing the isolated config directory.
    fn drop(&mut self) {
        self.app.world().resource::<MenuRuntime>().saves.flush();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn real_carrier_server_fields_click_type_select_paste_save_and_play() {
    for dpi in [1.0, 2.0] {
        exercise_server_fields(dpi);
    }
}

/// Replays the full input sequence at the requested physical-to-logical scale.
fn exercise_server_fields(dpi: f32) {
    let Some(mut h) = Harness::new(dpi) else {
        // The shared carrier loader names the missing fixture.
        return;
    };
    h.app
        .world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::Navigate(MenuScreen::Servers));
    h.app.update();
    h.click(MenuAction::PlayAddServer);
    for editing in [false, true] {
        let mut values = if editing {
            ["Server_Name_", "127.0.0.1", "19133"]
        } else {
            ["", "", DEFAULT_PORT]
        }
        .map(str::to_owned);
        for (index, action, field, pasted) in [
            (0, MenuAction::AddName, MenuField::Name, "Server_Name_"),
            (1, MenuAction::AddAddress, MenuField::Address, "127.0.0.1"),
            (2, MenuAction::AddPort, MenuField::Port, "19133"),
        ] {
            h.click(action);
            // Clear first so the number field has room for a typed digit.
            h.select_all();
            h.press(KeyCode::Backspace, None);
            values[index].clear();
            h.fields(values.each_ref().map(String::as_str), field);
            h.press(KeyCode::Digit4, Some("4"));
            values[index].push('4');
            h.fields(values.each_ref().map(String::as_str), field);
            h.press(KeyCode::Backspace, None);
            values[index].clear();
            h.fields(values.each_ref().map(String::as_str), field);
            h.press(KeyCode::Digit7, Some("7"));
            values[index].push('7');
            h.fields(values.each_ref().map(String::as_str), field);
            h.select_all();
            assert!(
                h.app
                    .world()
                    .resource::<MenuRuntime>()
                    .view()
                    .caret
                    .selection
                    .is_some()
            );
            h.fields(values.each_ref().map(String::as_str), field);
            h.paste(pasted);
            values[index] = pasted.into();
            h.fields(values.each_ref().map(String::as_str), field);
        }
        h.click(MenuAction::AddName);
        h.press(KeyCode::Tab, None);
        h.fields(values.each_ref().map(String::as_str), MenuField::Address);
        h.press(KeyCode::ShiftLeft, None);
        h.press(KeyCode::Tab, None);
        h.key(KeyCode::ShiftLeft, None, ButtonState::Released);
        h.fields(values.each_ref().map(String::as_str), MenuField::Name);
        h.click(MenuAction::AddPort);
        h.click(MenuAction::AddAddress);
        h.fields(values.each_ref().map(String::as_str), MenuField::Address);
        snapshot::write(
            &h.app.world().resource::<Frame>().0,
            if editing {
                "edit-server-input"
            } else {
                "add-server-input"
            },
        );
        h.click(MenuAction::AddSave);
        let menu = h.app.world().resource::<MenuRuntime>();
        menu.saves.flush();
        let saved = load_servers(&menu.layout.server_file()).servers;
        assert_eq!(saved.len(), 1);
        assert_eq!(
            (&*saved[0].name, &*saved[0].address),
            ("Server_Name_", "127.0.0.1:19133")
        );
        h.app
            .world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(MenuAction::EditSaved(0));
        h.app.update();
    }
    h.press(KeyCode::Escape, None);
    assert_eq!(
        h.app.world().resource::<MenuRuntime>().screen(),
        MenuScreen::AddServer
    );
    assert!(h.app.world().resource::<MenuRuntime>().field.is_none());
    h.press(KeyCode::Escape, None);
    assert_ne!(
        h.app.world().resource::<MenuRuntime>().screen(),
        MenuScreen::AddServer
    );
    h.app
        .world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::PlayAddServer);
    h.app.update();
    for (action, value) in [
        (MenuAction::AddName, "Play"),
        (MenuAction::AddAddress, "localhost"),
        (MenuAction::AddPort, "19134"),
    ] {
        h.click(action);
        h.select_all();
        h.paste(value);
    }
    h.click(MenuAction::AddSaveConnect);
    let pending = h
        .app
        .world_mut()
        .resource_mut::<MenuRuntime>()
        .take_join_intent()
        .expect("Play queues connection");
    assert_eq!(pending.address, "localhost:19134");
    let menu = h.app.world().resource::<MenuRuntime>();
    menu.saves.flush();
    assert!(
        load_servers(&menu.layout.server_file())
            .servers
            .iter()
            .any(|server| { server.name == "Play" && server.address == pending.address })
    );
}

#[test]
fn failed_server_loads_do_not_authorize_replacing_the_original_list() {
    for failed_quarantine in [false, true] {
        let root = std::env::temp_dir().join(format!(
            "cinnabar-server-protection-{}-{:?}-{failed_quarantine}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut layout = MenuRuntime::new(true, 2, "Steve".into()).layout.clone();
        layout.user_config_root = root.clone();
        let path = layout.server_file();
        if failed_quarantine {
            std::fs::write(&path, b"invalid").unwrap();
            let mut target = path.as_os_str().to_os_string();
            target.push(".invalid");
            let target = std::path::PathBuf::from(target);
            std::fs::create_dir_all(&target).unwrap();
            std::fs::write(target.join("keep"), b"keep").unwrap();
        } else {
            std::fs::create_dir_all(&path).unwrap();
        }
        let menu = MenuRuntime::new_with_layout(
            true,
            Some(2),
            "Steve".into(),
            layout,
            crate::player_skin::LocalPlayerSkin::generated_default("Steve"),
        );
        assert!(menu.message.is_some());
        if !failed_quarantine {
            std::fs::remove_dir(&path).unwrap();
        }
        // A transient failure can recover; the menu still has no authority for this snapshot.
        let original = br#"[{"name":"Keep","address":"keep.example"}]"#;
        std::fs::write(&path, original).unwrap();
        assert!(menu.saves.save(&[]).is_err());
        menu.saves.flush();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        drop(menu);
        std::fs::remove_dir_all(root).unwrap();
    }
}

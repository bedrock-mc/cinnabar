use super::*;
use bevy::{input::keyboard::Key, prelude::*};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct Harness {
    app: App,
    window: Entity,
    clipboard: Arc<Mutex<String>>,
    reads: Arc<AtomicUsize>,
}

impl Harness {
    fn new() -> Self {
        let clipboard = Arc::new(Mutex::new("pasted 🦀".to_owned()));
        let reads = Arc::new(AtomicUsize::new(0));
        let (text, count) = (clipboard.clone(), reads.clone());
        let copied = clipboard.clone();
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .init_resource::<Time<Real>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .insert_resource(UiRuntime::new(1))
            .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
            .insert_resource(crate::menu::MenuClipboard::with_access(
                move |_| {
                    count.fetch_add(1, Ordering::Relaxed);
                    Some(text.lock().unwrap().clone())
                },
                move |text| *copied.lock().unwrap() = text,
            ))
            .add_systems(Update, drive_chat_keyboard_input);
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
        Self {
            app,
            window,
            clipboard,
            reads,
        }
    }

    fn key(&mut self, key_code: KeyCode, state: ButtonState, text: Option<&str>) {
        let mut keys = self.app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        match state {
            ButtonState::Pressed => keys.press(key_code),
            ButtonState::Released => keys.release(key_code),
        }
        self.app.world_mut().write_message(KeyboardInput {
            key_code,
            state,
            logical_key: Key::Character(text.unwrap_or("").into()),
            text: text.map(Into::into),
            repeat: false,
            window: self.window,
        });
        self.app.update();
    }

    fn press(&mut self, key: KeyCode, text: Option<&str>) {
        self.key(key, ButtonState::Pressed, text);
    }

    fn release(&mut self, key: KeyCode) {
        self.key(key, ButtonState::Released, None);
    }

    fn text(&self) -> &str {
        self.app
            .world()
            .resource::<UiRuntime>()
            .chat_editor()
            .as_str()
    }
}

#[test]
fn held_control_or_command_pastes_across_suppressed_frames_even_before_chat_opens() {
    for modifier in [
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ] {
        let mut h = Harness::new();
        h.press(modifier, None);
        h.press(KeyCode::KeyT, Some("t"));
        h.app.update();
        assert!(
            h.app
                .world()
                .resource::<ButtonInput<KeyCode>>()
                .get_pressed()
                .next()
                .is_none()
        );
        h.press(KeyCode::KeyV, Some("v"));
        assert_eq!(h.text(), "pasted 🦀");
        assert_eq!(h.reads.load(Ordering::Relaxed), 1);
        assert!(
            h.app
                .world()
                .resource::<UiRuntime>()
                .pending_chat_sends()
                .is_empty()
        );
    }
}

#[test]
fn modifier_sides_release_independently_and_alt_excludes_paste() {
    let mut h = Harness::new();
    h.press(KeyCode::KeyT, Some("t"));
    h.press(KeyCode::ControlLeft, None);
    h.press(KeyCode::ControlRight, None);
    h.release(KeyCode::ControlLeft);
    h.press(KeyCode::AltRight, None);
    h.press(KeyCode::KeyV, Some("v"));
    assert_eq!(h.text(), "");
    assert_eq!(h.reads.load(Ordering::Relaxed), 0);
    h.release(KeyCode::AltRight);
    h.press(KeyCode::KeyV, Some("v"));
    assert_eq!(h.text(), "pasted 🦀");
    h.release(KeyCode::ControlRight);
    h.press(KeyCode::KeyV, Some("v"));
    assert_eq!(h.text(), "pasted 🦀v");
    assert_eq!(h.reads.load(Ordering::Relaxed), 1);
}

#[test]
fn held_shift_selects_across_frames_and_clipboard_adapter_enforces_chat_bound() {
    let mut h = Harness::new();
    h.press(KeyCode::KeyT, Some("t"));
    h.press(KeyCode::KeyA, Some("abc"));
    h.press(KeyCode::ShiftLeft, None);
    h.press(KeyCode::ArrowLeft, None);
    h.press(KeyCode::KeyX, Some("x"));
    assert_eq!(h.text(), "abx");
    h.release(KeyCode::ShiftLeft);
    *h.clipboard.lock().unwrap() = "x".repeat(ui::MAX_CHAT_INPUT_BYTES + 1);
    h.press(KeyCode::ControlLeft, None);
    h.press(KeyCode::KeyV, Some("v"));
    assert_eq!(
        h.text(),
        "abx",
        "oversized clipboard payload cannot escape the adapter bound"
    );
    assert_eq!(h.reads.load(Ordering::Relaxed), 1);
}

#[test]
fn focus_loss_drops_held_modifiers_and_discards_unfocused_events() {
    let mut h = Harness::new();
    h.press(KeyCode::KeyT, Some("t"));
    h.press(KeyCode::ControlLeft, None);
    h.app
        .world_mut()
        .get_mut::<Window>(h.window)
        .unwrap()
        .focused = false;
    h.release(KeyCode::ControlLeft);
    h.app
        .world_mut()
        .get_mut::<Window>(h.window)
        .unwrap()
        .focused = true;
    h.press(KeyCode::KeyT, Some("t"));
    h.press(KeyCode::KeyV, Some("v"));
    assert_eq!(h.text(), "v");
    assert_eq!(h.reads.load(Ordering::Relaxed), 0);
}

#[test]
fn menu_and_server_form_ownership_drop_chat_modifiers() {
    for menu_owns in [true, false] {
        let mut h = Harness::new();
        h.press(KeyCode::KeyT, Some("t"));
        h.press(KeyCode::ControlLeft, None);
        if menu_owns {
            h.app
                .insert_resource(crate::menu::MenuRuntime::new_with_layout(
                    true,
                    Some(2),
                    "Fixture".into(),
                    crate::install_layout::scratch("chat-modifier-ownership"),
                    crate::player_skin::LocalPlayerSkin::generated_default("Fixture"),
                ));
        } else {
            // An open chat correctly makes incoming forms busy. Close it through
            // the real keyboard path before admitting the new screen's authority.
            h.press(KeyCode::Escape, None);
            let mut runtime = h.app.world_mut().remove_resource::<UiRuntime>().unwrap();
            runtime
                .apply(
                    &mut h
                        .app
                        .world_mut()
                        .resource_mut::<crate::player_runtime::PlayerRuntime>(),
                    client_ui::ui_runtime::SequencedUiEvent {
                        session_id: 1,
                        fifo_sequence: 1,
                        local_millis: 1,
                        server_tick: None,
                        event: protocol::UiEvent::Form(protocol::FormRequestEvent {
                            form_id: 1,
                            kind: protocol::FormKind::Menu,
                            title: None,
                            json: "{}".into(),
                            model: protocol::ServerFormModel::TextMenu(protocol::TextMenuForm {
                                title: "Fixture".into(),
                                content: "Choose".into(),
                                buttons: vec![Arc::from("OK")].into(),
                                button_images: [].into(),
                                omitted_images: 0,
                            }),
                        }),
                    },
                )
                .unwrap();
            assert!(runtime.server_forms().owns_input());
            h.app.insert_resource(runtime);
        }
        // The raw release belongs to the other screen and is deliberately discarded by chat.
        h.release(KeyCode::ControlLeft);
        h.app
            .world_mut()
            .remove_resource::<crate::menu::MenuRuntime>();
        h.app.insert_resource(UiRuntime::new(1));
        h.press(KeyCode::KeyT, Some("t"));
        h.press(KeyCode::KeyV, Some("v"));
        assert_eq!(h.text(), "v");
        assert_eq!(h.reads.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn held_control_or_command_selects_all_then_copies_without_typing_or_sending() {
    for modifier in [
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ] {
        let mut h = Harness::new();
        h.press(KeyCode::KeyT, Some("t"));
        h.press(KeyCode::KeyX, Some("Zeno 世界 🦀"));
        h.press(modifier, None);
        h.app.update();
        h.press(KeyCode::KeyA, Some("a"));
        assert_eq!(
            h.app
                .world()
                .resource::<UiRuntime>()
                .chat_editor()
                .selection(),
            Some(0..h.text().len())
        );
        h.app.update();
        h.press(KeyCode::KeyC, Some("c"));
        assert_eq!(*h.clipboard.lock().unwrap(), "Zeno 世界 🦀");
        assert_eq!(h.text(), "Zeno 世界 🦀");
        assert!(
            h.app
                .world()
                .resource::<UiRuntime>()
                .pending_chat_sends()
                .is_empty()
        );
        h.release(modifier);
        h.press(KeyCode::KeyX, Some("x"));
        assert_eq!(
            h.text(),
            "x",
            "select-all replacement covers the whole Unicode text"
        );
    }
}

#[test]
fn copy_uses_only_unicode_selection_and_no_selection_leaves_clipboard_unchanged() {
    let mut h = Harness::new();
    h.press(KeyCode::KeyT, Some("t"));
    h.press(KeyCode::KeyX, Some("A世界🦀"));
    h.press(KeyCode::ControlLeft, None);
    h.press(KeyCode::KeyC, Some("c"));
    assert_eq!(*h.clipboard.lock().unwrap(), "pasted 🦀");
    h.release(KeyCode::ControlLeft);
    h.press(KeyCode::ShiftRight, None);
    h.press(KeyCode::ArrowLeft, None);
    h.press(KeyCode::ArrowLeft, None);
    h.release(KeyCode::ShiftRight);
    h.press(KeyCode::ControlRight, None);
    h.app.update();
    h.press(KeyCode::KeyC, Some("c"));
    assert_eq!(*h.clipboard.lock().unwrap(), "界🦀");
    assert_eq!(h.text(), "A世界🦀");
    assert!(
        h.app
            .world()
            .resource::<UiRuntime>()
            .pending_chat_sends()
            .is_empty()
    );
}

#[test]
fn alt_excludes_select_all_and_copy_shortcuts() {
    let mut h = Harness::new();
    h.press(KeyCode::KeyT, Some("t"));
    h.press(KeyCode::KeyX, Some("draft"));
    h.press(KeyCode::ControlLeft, None);
    h.press(KeyCode::AltLeft, None);
    h.press(KeyCode::KeyA, Some("a"));
    h.press(KeyCode::KeyC, Some("c"));
    assert!(
        h.app
            .world()
            .resource::<UiRuntime>()
            .chat_editor()
            .selection()
            .is_none()
    );
    assert_eq!(*h.clipboard.lock().unwrap(), "pasted 🦀");
    assert_eq!(h.text(), "draft");
}

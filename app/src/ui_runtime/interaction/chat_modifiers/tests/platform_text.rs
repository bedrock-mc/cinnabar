//! Platform composition and the platform Back key keep their place among the frame's keys.

use super::*;
use bevy::window::{Ime, WindowEvent};

impl Harness {
    /// Delivers one frame of window events, mirroring keys into the keyboard message stream.
    fn frame(&mut self, events: Vec<WindowEvent>) {
        for event in events {
            if let WindowEvent::KeyboardInput(input) = &event {
                self.app.world_mut().write_message(input.clone());
            }
            self.app.world_mut().write_message(event);
        }
        self.app.update();
    }

    fn window_key(&self, key_code: KeyCode, logical_key: Key, text: Option<&str>) -> WindowEvent {
        WindowEvent::KeyboardInput(KeyboardInput {
            key_code,
            logical_key,
            state: ButtonState::Pressed,
            text: text.map(Into::into),
            repeat: false,
            window: self.window,
        })
    }

    fn commit(&self, value: &str) -> WindowEvent {
        WindowEvent::Ime(Ime::Commit {
            window: self.window,
            value: value.into(),
        })
    }

    fn platform_back(&self) -> WindowEvent {
        self.window_key(
            KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Android(4)),
            Key::BrowserBack,
            None,
        )
    }
}

/// The chat keyboard harness with the chat's pointer system running first, as in production.
fn harness() -> Harness {
    let mut h = Harness::new();
    h.app
        .add_message::<WindowEvent>()
        .init_resource::<bevy::input::touch::Touches>()
        .init_resource::<crate::runtime::world::ClientWorld>()
        .init_resource::<client_presentation::local_player::InteractionOriginSnapshot>()
        .init_resource::<client_presentation::local_player::LocalPlayerFrameCarrier>()
        .init_resource::<client_state::SemanticInputSnapshot>()
        .insert_resource(UiPresentationResource(
            UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap(),
        ))
        .add_systems(
            Update,
            crate::ui_runtime::drive_chat_ui_actions.before(drive_chat_keyboard_input),
        );
    h
}

/// Text committed before Enter in the same frame is part of the message Enter sends.
#[test]
fn composition_committed_before_enter_is_sent() {
    let mut h = harness();
    h.press(KeyCode::KeyT, None);
    h.press(KeyCode::KeyA, Some("a"));
    let enter = h.window_key(KeyCode::Enter, Key::Enter, None);
    let backspace = h.window_key(KeyCode::Backspace, Key::Backspace, None);
    let (first, second) = (h.commit("日本"), h.commit("語"));
    h.frame(vec![first, backspace, second, enter]);
    let runtime = h.app.world().resource::<UiRuntimeResource>();
    assert!(!runtime.chat_focused());
    let sent: Vec<_> = runtime
        .pending_chat_sends()
        .iter()
        .map(|request| request.message.to_string())
        .collect();
    assert_eq!(sent, ["a日語"]);
}

/// A chat popup owns input, so composition never edits the draft beneath it.
#[test]
fn chat_popup_blocks_composition() {
    let mut h = harness();
    h.press(KeyCode::KeyT, None);
    h.app
        .world_mut()
        .resource_mut::<UiPresentationResource>()
        .set_chat_settings_open(true);
    let commit = h.commit("hidden");
    h.frame(vec![commit]);
    assert_eq!(h.text(), "");
    h.app
        .world_mut()
        .resource_mut::<UiPresentationResource>()
        .set_chat_settings_open(false);
    let commit = h.commit("shown");
    h.frame(vec![commit]);
    assert_eq!(h.text(), "shown");
}

/// Back after the chat key in the same frame closes the chat that key opened, and consumes the
/// mouse Back press the platform key stands for.
#[test]
fn platform_back_after_chat_key_closes_chat() {
    let mut h = harness();
    h.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Back);
    let open = h.window_key(KeyCode::KeyT, Key::Character("t".into()), Some("t"));
    let back = h.platform_back();
    h.frame(vec![open, back]);
    assert!(!h.app.world().resource::<UiRuntimeResource>().chat_focused());
    assert!(
        !h.app
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .just_pressed(MouseButton::Back)
    );
}

/// Scripted text arrives as a commit plus a typed mirror; chat inserts each text once.
#[test]
fn scripted_text_with_typed_mirror_is_inserted_once() {
    let mut h = harness();
    h.press(KeyCode::KeyT, None);
    let mut events = Vec::new();
    for value in ["hello", " 日本"] {
        events.push(h.commit(value));
        events.push(h.window_key(
            KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
            Key::Character(value.into()),
            Some(value),
        ));
    }
    let release = WindowEvent::KeyboardInput(KeyboardInput {
        key_code: KeyCode::KeyT,
        logical_key: Key::Character("t".into()),
        state: ButtonState::Released,
        text: None,
        repeat: false,
        window: h.window,
    });
    events.insert(0, release);
    h.frame(events);
    assert_eq!(h.text(), "hello 日本");
}

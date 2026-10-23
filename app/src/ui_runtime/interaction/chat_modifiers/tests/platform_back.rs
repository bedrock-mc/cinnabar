//! The platform Back key keeps its place among the frame's keys.

use super::*;
use bevy::window::WindowEvent;

impl Harness {
    /// Delivers one frame of keys through the ordered window stream and the keyboard messages.
    fn frame(&mut self, keys: Vec<KeyboardInput>) {
        for key in keys {
            self.app.world_mut().write_message(key.clone());
            self.app
                .world_mut()
                .write_message(WindowEvent::KeyboardInput(key));
        }
        self.app.update();
    }

    fn pressed(&self, key_code: KeyCode, logical_key: Key, text: Option<&str>) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key,
            state: ButtonState::Pressed,
            text: text.map(Into::into),
            repeat: false,
            window: self.window,
        }
    }

    /// Android's Back key, which its Back gesture also sends.
    fn platform_back(&self) -> KeyboardInput {
        self.pressed(
            KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Android(4)),
            Key::BrowserBack,
            None,
        )
    }
}

fn harness() -> Harness {
    let mut h = Harness::new();
    h.app
        .add_message::<WindowEvent>()
        .insert_resource(UiPresentationResource(
            UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap(),
        ));
    h
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
    let open = h.pressed(KeyCode::KeyT, Key::Character("t".into()), Some("t"));
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

/// Back closes a chat popup before the chat, one press at a time, and a held key never cancels.
#[test]
fn platform_back_closes_the_chat_popup_then_chat() {
    let mut h = harness();
    h.press(KeyCode::KeyT, None);
    h.app
        .world_mut()
        .resource_mut::<UiPresentationResource>()
        .set_chat_settings_open(true);
    let back = h.platform_back();
    h.frame(vec![back]);
    assert!(h.app.world().resource::<UiRuntimeResource>().chat_focused());
    assert!(
        !h.app
            .world()
            .resource::<UiPresentationResource>()
            .chat_settings_open()
    );
    let held = KeyboardInput {
        repeat: true,
        ..h.platform_back()
    };
    h.frame(vec![held]);
    assert!(h.app.world().resource::<UiRuntimeResource>().chat_focused());
    let back = h.platform_back();
    h.frame(vec![back]);
    assert!(!h.app.world().resource::<UiRuntimeResource>().chat_focused());
}

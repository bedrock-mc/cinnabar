//! The command shortcut opens the editor with its prefix and consumes the opening key.

use super::*;

#[test]
fn slash_opens_chat_with_one_prefix_and_typing_continues_after_it() {
    for sleeping in [false, true] {
        let mut h = Harness::new();
        h.app
            .world_mut()
            .resource_mut::<UiRuntimeResource>()
            .set_local_sleeping(sleeping);
        h.press(KeyCode::Slash, Some("/"));

        assert!(h.app.world().resource::<UiRuntimeResource>().chat_focused());
        assert_eq!(h.text(), "/");
        assert!(
            h.app
                .world()
                .resource::<UiRuntimeResource>()
                .pending_chat_sends()
                .is_empty()
        );
        let cursor = h.app.world().get::<CursorOptions>(h.window).unwrap();
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
        assert!(cursor.visible);
        assert!(
            h.app
                .world()
                .resource::<ButtonInput<KeyCode>>()
                .get_pressed()
                .next()
                .is_none()
        );

        h.release(KeyCode::Slash);
        h.press(KeyCode::KeyH, Some("help"));
        assert_eq!(h.text(), "/help");
        h.press(KeyCode::Escape, None);
        assert!(!h.app.world().resource::<UiRuntimeResource>().chat_focused());
        assert_eq!(h.text(), "");
        h.press(KeyCode::KeyT, Some("t"));
        assert!(h.app.world().resource::<UiRuntimeResource>().chat_focused());
        assert_eq!(h.text(), "");
    }
}

#[test]
fn slash_in_open_chat_is_inserted_at_the_caret() {
    let mut h = Harness::new();
    h.press(KeyCode::KeyT, Some("t"));
    h.press(KeyCode::KeyA, Some("ab"));
    h.press(KeyCode::ArrowLeft, None);
    h.press(KeyCode::Slash, Some("/"));
    assert_eq!(h.text(), "a/b");
}

#[test]
fn unfocused_slash_does_not_open_chat_or_replay_after_focus_returns() {
    let mut h = Harness::new();
    h.set_focused(false);
    h.press(KeyCode::Slash, Some("/"));
    h.set_focused(true);
    h.app.update();
    assert!(!h.app.world().resource::<UiRuntimeResource>().chat_focused());
    assert_eq!(h.text(), "");
}

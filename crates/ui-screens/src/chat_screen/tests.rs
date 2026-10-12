//! The chat settings popup and a pending link confirmation own wheel input.

use super::*;

#[test]
fn review_chat_settings_own_wheel_input() {
    let mut chat = ChatScreen {
        scroll: Some((
            "history".to_owned(),
            ScrollMetrics {
                content: 400.0,
                viewport: 100.0,
                ..ScrollMetrics::default()
            },
        )),
        ..ChatScreen::default()
    };
    chat.settings.set_open(true);
    chat.scroll(40.0, true);
    assert_eq!(chat.from_bottom, 0.0);
    chat.settings.set_open(false);
    chat.scroll(40.0, true);
    assert!(chat.from_bottom > 0.0);
}

#[test]
fn chat_link_confirmation_owns_wheel_input() {
    let mut chat = ChatScreen {
        scroll: Some((
            "history".to_owned(),
            ScrollMetrics {
                content: 400.0,
                viewport: 100.0,
                ..Default::default()
            },
        )),
        scale: 1.0,
        ..ChatScreen::default()
    };
    chat.links.push("https://example.com".into());
    chat.request_link(0);
    chat.scroll(40.0, true);
    assert_eq!(chat.from_bottom, 0.0);
    chat.cancel_link();
    chat.scroll(40.0, true);
    assert!(chat.from_bottom > 0.0);
}

#[test]
fn pending_target_survives_frame_replacement_and_cancel_blocks_background_scroll() {
    let mut chat = ChatScreen::default();
    chat.links.push("https://example.com/a".into());
    chat.request_link(0);
    chat.links.clear();
    assert!(chat.link_confirmation_open());
    assert_eq!(
        chat.take_confirmed_link().as_deref(),
        Some("https://example.com/a")
    );
    assert!(!chat.link_confirmation_open());
    chat.links.push("https://example.com/b".into());
    chat.request_link(0);
    chat.cancel_link();
    assert_eq!(chat.take_confirmed_link(), None);
    chat.request_link(99);
    assert!(!chat.link_confirmation_open());
}

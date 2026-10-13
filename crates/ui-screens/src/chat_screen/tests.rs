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

#[test]
fn mention_rows_show_the_name_and_highlight_the_typed_match() {
    assert_eq!(
        suggestion_row_text("hello @Al", "@\"Alice One\"", &|_| None),
        "§7\"§fAl§7ice One\"§r"
    );
    assert_eq!(suggestion_row_text("@", "@Alex", &|_| None), "§7Alex§r");
}

#[test]
fn command_rows_highlight_the_name_match_after_the_slash() {
    for input in ["/iv", "/IV"] {
        assert_eq!(
            suggestion_row_text(input, "/give", &|_| None),
            "§7/g§fiv§7e§r"
        );
    }
    assert_eq!(
        suggestion_row_text("/g", "/give", &|_| None),
        "§7/§fg§7ive§r"
    );
    assert_eq!(suggestion_row_text("/", "/give", &|_| None), "§7/give§r");
}

#[test]
fn here_and_selector_rows_use_their_translated_descriptions() {
    let translate = |key: &str| match key {
        "chat.mentions.autocomplete.allPlayers" => Some(Arc::from("All players")),
        "commands.autocomplete.n" => Some(Arc::from("Nearest entity")),
        _ => None,
    };
    assert_eq!(
        suggestion_row_text("@h", "@here", &translate),
        "§7§f@h§7ere§7 - §fAll players§r"
    );
    assert_eq!(
        suggestion_row_text("/give @", "@n", &translate),
        "§7§f@§7n§7 - §fNearest entity§r"
    );
}

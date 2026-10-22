//! The chat settings popup and a pending link confirmation own wheel input.

use super::*;

#[test]
fn only_chat_history_background_follows_opacity_on_rebind() {
    use json_ui::{BindState, Catalog, EmptyLibrary, bind_stateful};
    use launcher::menu::settings_options::SettingsOptions;

    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/chat_base.json",
        r#"{
        "namespace": "chat",
        "messages_scrolling_panel": { "type": "panel" },
        "commands_panel": { "type": "panel" },
        "messages_text": { "type": "panel" },
        "auto_complete_panel_contents_with_item": { "type": "panel" }
    }"#,
    );
    let (path, _, bytes) = crate::hud::JAVA_HUD_PACK
        .iter()
        .find(|(_, namespace, _)| *namespace == "chat")
        .unwrap();
    catalog.overlay_text(path, std::str::from_utf8(bytes).unwrap());
    let context = crate::menu_screens::retail_context();
    let editor = ui::ChatEditor::new(ui::MAX_CHAT_INPUT_BYTES).unwrap();
    let messages = VecDeque::new();
    let clocks = BTreeMap::new();
    let input = ChatInput {
        editor: &editor,
        messages: &messages,
        suggestions: &[],
        usage: None,
        selected_suggestion: None,
        annotations: &[],
        clocks: &clocks,
        now_millis: 0,
    };
    for (reference, panel, fixed_alpha) in [
        (
            "chat.java_chat_content",
            Some("input_line"),
            Some(128.0 / 255.0),
        ),
        ("chat.messages_text", None, None),
        (
            "chat.auto_complete_panel_contents_with_item",
            None,
            Some(208.0 / 255.0),
        ),
    ] {
        let root = Arc::new(
            json_ui::resolve(&catalog, reference, &context)
                .control
                .unwrap(),
        );
        let mut state = BindState::new();
        for percent in [0, 25, 100, 50, 0] {
            let mut options = SettingsOptions::default();
            options.set(SettingId::ChatBackgroundOpacity.index(), percent);
            let mut settings = ChatSettings::default();
            settings.set_snapshot((Arc::new(options), None));
            let data = chat_data(&input, &settings, &|_| None);
            let (bound, _) = bind_stateful(&root, &data, &EmptyLibrary, &mut state);
            let panel = panel.map_or(&bound, |name| {
                bound.find(&|control| control.name == name).unwrap()
            });
            let background = panel.find(&|control| control.name == "background").unwrap();
            assert_eq!(
                background.properties["alpha"].as_f64(),
                Some(fixed_alpha.unwrap_or(f64::from(percent) / 100.0)),
                "{reference} at {percent}%"
            );
        }
    }
}

/// Captured sliders follow value segments and retain their option when the finger leaves the track.
#[test]
fn captured_chat_slider_clamps_and_survives_drift_between_value_segments() {
    let index = launcher::menu::settings_options::SettingId::ChatBackgroundOpacity.index() as u16;
    let mut chat = ChatScreen::default();
    chat.settings.set_open(true);
    let action = |value| launcher::menu::MenuAction::SettingsOption(index, value);
    for (slot, value) in [0, 50, 100].into_iter().enumerate() {
        let rect = UiRect::new(
            ui::UiPoint::new(slot as f32 * 10.0, 0.0).unwrap(),
            ui::UiPoint::new((slot + 1) as f32 * 10.0, 20.0).unwrap(),
        )
        .unwrap();
        chat.hits.push((
            ChatHit::SettingsAction(action(value)),
            rect,
            "slider".into(),
        ));
    }
    assert_eq!(
        chat.slider_at(ui::UiPoint::new(9.0, 5.0).unwrap()),
        Some(index)
    );
    assert_eq!(
        chat.slider_action(index, ui::UiPoint::new(11.0, 5.0).unwrap()),
        Some(action(50))
    );
    assert_eq!(
        chat.slider_action(index, ui::UiPoint::new(100.0, 200.0).unwrap()),
        Some(action(100))
    );
    assert_eq!(
        chat.slider_action(index, ui::UiPoint::new(-100.0, -200.0).unwrap()),
        Some(action(0))
    );
    chat.settings.set_open(false);
    assert_eq!(
        chat.slider_action(index, ui::UiPoint::new(11.0, 5.0).unwrap()),
        None
    );
}

/// Only an uncovered, painted history viewport may capture a scrolling finger.
#[test]
fn history_touch_capture_excludes_popups_and_other_controls() {
    let point = ui::UiPoint::new(20.0, 20.0).unwrap();
    let mut chat = ChatScreen {
        history_bounds: Some(
            ui::UiRect::new(
                ui::UiPoint::new(0.0, 0.0).unwrap(),
                ui::UiPoint::new(100.0, 100.0).unwrap(),
            )
            .unwrap(),
        ),
        ..Default::default()
    };
    assert!(chat.history_contains(point));
    assert!(!chat.history_contains(ui::UiPoint::new(200.0, 20.0).unwrap()));
    chat.settings.set_open(true);
    assert!(!chat.history_contains(point));
    chat.settings.set_open(false);
    chat.pending_link = Some("https://example.test".into());
    assert!(!chat.history_contains(point));
    chat.close();
    assert!(!chat.history_contains(point));
}

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

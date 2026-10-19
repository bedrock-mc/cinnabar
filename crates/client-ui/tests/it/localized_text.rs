//! Localization across text packet surfaces and rawtext arguments.

use std::sync::Arc;

use client_ui::ui_runtime::UiRuntime;
use protocol::{TextCategory, TextEvent, TextKind, UiEvent};

use super::fixtures::{envelope, raw_text_event};
use super::support::literal_raw_text;

#[test]
fn localized_text_packets_resolve_marked_keys_for_chat_and_popups() {
    let bytes = assets::encode_lang_catalog(
        [9; 32],
        [10; 32],
        &[assets::LangEntry {
            key: "multiplayer.player.joined".into(),
            value: "%s joined the game".into(),
        }],
    )
    .unwrap();
    for kind in [TextKind::Raw, TextKind::System, TextKind::Popup] {
        let mut player_runtime = player_state::PlayerState::new(1);
        let mut runtime = UiRuntime::new(1);
        runtime.set_lang_catalog(Arc::new(
            assets::RuntimeLangCatalog::decode(&bytes).unwrap(),
        ));
        runtime
            .apply(
                &mut player_runtime,
                envelope(
                    1,
                    1,
                    UiEvent::Text(TextEvent {
                        category: TextCategory::Parameters,
                        kind,
                        needs_translation: true,
                        source: None,
                        message: Arc::from("§e%multiplayer.player.joined"),
                        parameters: Arc::from([Arc::from("Alex")]),
                        xuid: Arc::from(""),
                        platform_chat_id: Arc::from(""),
                        filtered_message: None,
                    }),
                ),
            )
            .unwrap();
        let message = if kind == TextKind::Popup {
            runtime.hud().actionbar().unwrap().text.as_ref()
        } else {
            runtime.chat().messages().back().unwrap().message.as_ref()
        };
        assert_eq!(message, "§eAlex joined the game");
    }
}

#[test]
fn rawtext_game_mode_feedback_localizes_arguments_without_changing_literal_text() {
    let entries = [
        ("createWorldScreen.gameMode.creative", "Creative"),
        ("gameMode.changed", "Your game mode has been updated to %s"),
    ]
    .map(|(key, value)| assets::LangEntry {
        key: key.into(),
        value: value.into(),
    });
    let bytes = assets::encode_lang_catalog([9; 32], [10; 32], &entries).unwrap();
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(Arc::new(
        assets::RuntimeLangCatalog::decode(&bytes).unwrap(),
    ));
    for (sequence, arguments) in [
        r#"["%createWorldScreen.gameMode.creative"]"#,
        r#"{"rawtext":[{"text":"%createWorldScreen.gameMode.creative"}]}"#,
    ]
    .into_iter()
    .enumerate()
    {
        let document = format!(
            r#"{{"rawtext":[{{"translate":"gameMode.changed","with":{arguments}}},{{"text":" | 100% literal %createWorldScreen.gameMode.creative"}}]}}"#
        );
        runtime
            .apply(
                &mut player_runtime,
                envelope(1, sequence as u64 + 1, raw_text_event(&document)),
            )
            .unwrap();
        assert_eq!(
            runtime.chat().messages().back().unwrap().message.as_ref(),
            "Your game mode has been updated to Creative | 100% literal %createWorldScreen.gameMode.creative"
        );
    }
}

#[test]
fn flagged_rawtext_preserves_literal_and_resolved_percent_text() {
    let entries = [
        ("menu.play", "Play"),
        ("progress", "100%% translated %%menu.play"),
    ]
    .map(|(key, value)| assets::LangEntry {
        key: key.into(),
        value: value.into(),
    });
    let bytes = assets::encode_lang_catalog([9; 32], [10; 32], &entries).unwrap();
    for kind in [
        TextKind::Raw,
        TextKind::Json,
        TextKind::JsonWhisper,
        TextKind::Tip,
    ] {
        let mut runtime = UiRuntime::new(1);
        let mut player_runtime = player_state::PlayerState::new(1);
        runtime.set_lang_catalog(Arc::new(
            assets::RuntimeLangCatalog::decode(&bytes).unwrap(),
        ));
        for (index, (json, expected)) in [
            (
                r#"{"rawtext":[{"text":"100% literal %menu.play"}]}"#,
                "100% literal %menu.play",
            ),
            (
                r#"{"rawtext":[{"translate":"progress"}]}"#,
                "100% translated %menu.play",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let UiEvent::RawText(mut event) = literal_raw_text(kind, json) else {
                unreachable!();
            };
            event.text.needs_translation = true;
            runtime
                .apply(
                    &mut player_runtime,
                    envelope(1, index as u64 + 1, UiEvent::RawText(event)),
                )
                .unwrap();
            let message = if kind == TextKind::Tip {
                runtime.hud().tip().unwrap().text.as_ref()
            } else {
                runtime.chat().messages().back().unwrap().message.as_ref()
            };
            assert_eq!(message, expected, "{kind:?}: {json}");
        }
    }
}

#[test]
fn large_server_translation_streams_through_text_and_death_consumers() {
    let value = "a".repeat(assets::MAX_LANG_VALUE_BYTES);
    let language = format!("k={value}\n");
    assert!(language.len() <= assets::MAX_SERVER_LANG_INPUT_BYTES);
    let overlay = assets::ServerLangOverlay::read(language.len(), |target| {
        target.copy_from_slice(language.as_bytes());
        true
    })
    .expect("the server language value fits the retained input policy");
    let message: Arc<str> = "%k ".repeat(5_000).into();
    assert!(message.len() <= protocol::MAX_UI_TEXT_BYTES);
    let localized_token = format!("{value} ");
    let retained_tokens = protocol::MAX_RAW_TEXT_OUTPUT_BYTES.div_ceil(localized_token.len()) + 1;
    let expected = protocol::format_translation(&localized_token.repeat(retained_tokens), &[]);
    for kind in [
        TextKind::Raw,
        TextKind::System,
        TextKind::Popup,
        TextKind::Translation,
    ] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = UiRuntime::new(1);
        runtime.set_server_lang(Some(Arc::clone(&overlay)));
        runtime
            .apply(
                &mut player,
                envelope(
                    1,
                    1,
                    UiEvent::Text(TextEvent {
                        category: if matches!(kind, TextKind::Raw | TextKind::System) {
                            TextCategory::MessageOnly
                        } else {
                            TextCategory::Parameters
                        },
                        kind,
                        needs_translation: true,
                        source: None,
                        message: Arc::clone(&message),
                        parameters: Arc::from([]),
                        xuid: Arc::from(""),
                        platform_chat_id: Arc::from(""),
                        filtered_message: None,
                    }),
                ),
            )
            .unwrap();
        match kind {
            TextKind::Popup => {
                assert_eq!(runtime.hud().actionbar().unwrap().text.as_ref(), expected)
            }
            TextKind::Translation => {
                let translator = runtime.translator();
                assert_eq!(
                    ui_screens::text::resolve_chat_line(
                        runtime.chat().messages().back().unwrap(),
                        |key| translator.lookup(key)
                    ),
                    expected
                );
            }
            _ => assert_eq!(
                runtime.chat().messages().back().unwrap().message.as_ref(),
                expected
            ),
        }
        runtime
            .apply(
                &mut player,
                envelope(
                    1,
                    2,
                    UiEvent::DeathInfo(protocol::DeathInfoEvent {
                        message: Arc::clone(&message),
                        parameters: Arc::from([]),
                    }),
                ),
            )
            .unwrap();
        assert_eq!(runtime.death_reason(), expected);
    }
}

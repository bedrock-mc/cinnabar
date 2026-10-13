//! Mention and quoted-target completion through the public chat API.

use super::*;

#[test]
fn mention_tab_cycles_quoted_roster_names_and_here_in_both_directions() {
    let mut player_runtime = player_state::PlayerState::new(2);
    let mut runtime = UiRuntime::new(2);
    runtime.refresh_raw_text_identities(
        |_| None,
        vec![Arc::from("Alice Two"), Arc::from("Alice One")],
    );
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("hello @").unwrap();
    assert!(runtime.service_pending_chat_autocomplete(&player_runtime));

    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "hello @\"Alice One\"");
    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "hello @\"Alice Two\"");
    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "hello @here");
    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "hello @\"Alice One\"");
    assert!(runtime.handle_chat_ui_action(UiAction::TabPrevious));
    assert_eq!(runtime.chat_editor().as_str(), "hello @here");
    assert!(runtime.handle_chat_ui_action(UiAction::TabPrevious));
    assert_eq!(runtime.chat_editor().as_str(), "hello @\"Alice Two\"");
}

#[test]
fn mention_acceptance_preserves_the_prefix_tail_and_outbound_text() {
    let mut player_runtime = player_state::PlayerState::new(2);
    let mut runtime = UiRuntime::new(2);
    runtime.set_chat_identity(Arc::from("Reader"), Arc::from(""));
    runtime.refresh_raw_text_identities(|_| None, vec![Arc::from("Alice One")]);
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("hello @Alice O!").unwrap();
    runtime.move_chat_cursor_left();
    assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
    assert!(runtime.handle_chat_ui_action(UiAction::Accept));
    assert_eq!(runtime.chat_editor().as_str(), "hello @\"Alice One\"!");
    assert_eq!(
        runtime.chat_editor().cursor_byte(),
        "hello @\"Alice One\"".len()
    );

    let request = runtime.queue_chat_send(0).unwrap();
    assert_eq!(request.message.as_ref(), "hello @\"Alice One\"!");
    let (_, packet) = runtime.next_chat_packet().unwrap().unwrap();
    assert_eq!(
        packet,
        chat_text_packet("Reader", "", &request.message).unwrap()
    );
}

#[test]
fn completed_mentions_can_be_sent_after_completion_is_serviced() {
    for (input, expected) in [
        ("hello @here", "hello @here"),
        ("hello @Alex", "hello @Alex"),
        ("hello @\"Alice One\"", "hello @\"Alice One\""),
        ("hello @h", "hello @here"),
        ("hello @Al", "hello @Alex"),
        ("hello @Alice O", "hello @\"Alice One\""),
    ] {
        let mut player_runtime = player_state::PlayerState::new(2);
        let mut runtime = UiRuntime::new(2);
        runtime
            .refresh_raw_text_identities(|_| None, vec![Arc::from("Alex"), Arc::from("Alice One")]);
        runtime.open_chat(&mut player_runtime);
        runtime.insert_chat_text(input).unwrap();
        assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
        if input != expected {
            assert!(dispatch_chat_ui_action(
                &mut runtime,
                UiAction::Accept,
                None,
                0
            ));
            assert_eq!(runtime.chat_editor().as_str(), expected);
            assert!(runtime.pending_chat_sends().is_empty());
            assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
        }
        assert!(runtime.chat_suggestions().is_empty());
        assert!(dispatch_chat_ui_action(
            &mut runtime,
            UiAction::Accept,
            None,
            0
        ));
        assert!(!runtime.chat_focused());
        assert_eq!(runtime.pending_chat_sends().len(), 1);
        assert_eq!(runtime.pending_chat_sends()[0].message.as_ref(), expected);
    }
}

#[test]
fn accepting_an_unchanged_tab_completion_ends_the_cycle_and_allows_send() {
    for (input, expected) in [
        ("hello @h", "hello @here"),
        ("hello @Alex", "hello @AlexTwo"),
        ("hello @Alice O", "hello @\"Alice One\""),
    ] {
        for action in [
            UiAction::Accept,
            UiAction::PointerPrimary {
                position: UiPoint::new(20.0, 533.0).unwrap(),
                phase: PointerPhase::Pressed,
            },
        ] {
            let mut player_runtime = player_state::PlayerState::new(2);
            let mut runtime = UiRuntime::new(2);
            runtime.refresh_raw_text_identities(
                |_| None,
                vec![Arc::from("AlexTwo"), Arc::from("Alice One")],
            );
            runtime.open_chat(&mut player_runtime);
            runtime.insert_chat_text(input).unwrap();
            assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
            assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
            assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
            assert_eq!(runtime.chat_editor().as_str(), expected);
            assert_eq!(runtime.chat_suggestions().len(), 1);
            assert!(dispatch_chat_ui_action(&mut runtime, action, Some(0), 0));
            assert_eq!(runtime.chat_editor().as_str(), expected);
            assert!(runtime.chat_suggestions().is_empty());
            assert!(runtime.pending_chat_sends().is_empty());
            assert!(!runtime.service_pending_chat_autocomplete(&player_runtime));
            assert!(dispatch_chat_ui_action(
                &mut runtime,
                UiAction::Accept,
                None,
                0
            ));
            assert_eq!(runtime.pending_chat_sends()[0].message.as_ref(), expected);
        }
    }
}

#[test]
fn command_target_completion_replaces_an_unfinished_quoted_name() {
    let mut player_runtime = player_state::PlayerState::new(2);
    let mut runtime = UiRuntime::new(2);
    runtime.refresh_raw_text_identities(|_| None, vec![Arc::from("Alice One")]);
    runtime.open_chat(&mut player_runtime);
    runtime
        .apply(
            &mut player_runtime,
            envelope(2, 1, UiEvent::AvailableCommands(warp_tree())),
        )
        .unwrap();
    runtime.insert_chat_text("/warp fast \"Alice O").unwrap();
    assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
    assert!(runtime.handle_chat_ui_action(UiAction::Accept));
    assert_eq!(runtime.chat_editor().as_str(), "/warp fast \"Alice One\"");
}

/// Provides a player target followed by an item enum to check quoted argument boundaries.
fn give_tree() -> protocol::CommandTreeEvent {
    protocol::CommandTreeEvent {
        commands: Arc::from([protocol::CommandSpec {
            name: Arc::from("give"),
            aliases: Arc::from([]),
            permission: 0,
            overloads: Arc::from([Arc::from([
                protocol::CommandParam {
                    name: Arc::from("player"),
                    optional: false,
                    collapse_enum: false,
                    kind: protocol::CommandParamKind::Target,
                },
                protocol::CommandParam {
                    name: Arc::from("item"),
                    optional: false,
                    collapse_enum: false,
                    kind: protocol::CommandParamKind::Enum {
                        name: Arc::from("Item"),
                        values: Arc::from([Arc::from("dirt"), Arc::from("stone")]),
                    },
                },
            ])]),
        }]),
        soft_enums: Arc::from([]),
    }
}

#[test]
fn quoted_completion_consumes_the_existing_delimiter_and_preserves_following_text() {
    for prefix in ["/give ", "héllo @"] {
        for (before, tail) in [
            ("\"Alice One", "\" di"),
            ("\"Alice O", "\" di"),
            ("\"Alice O", "ne\" di"),
        ] {
            for action in [
                UiAction::Accept,
                UiAction::PointerPrimary {
                    position: UiPoint::new(20.0, 533.0).unwrap(),
                    phase: PointerPhase::Pressed,
                },
            ] {
                let mut player_runtime = player_state::PlayerState::new(2);
                let mut runtime = UiRuntime::new(2);
                runtime.refresh_raw_text_identities(|_| None, vec![Arc::from("Alice One")]);
                runtime.open_chat(&mut player_runtime);
                runtime
                    .apply(
                        &mut player_runtime,
                        envelope(2, 1, UiEvent::AvailableCommands(give_tree())),
                    )
                    .unwrap();
                runtime
                    .insert_chat_text(&format!("{prefix}{before}{tail}"))
                    .unwrap();
                runtime
                    .mutate_chat_editor(|editor| editor.place_cursor(prefix.len() + before.len()));
                assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
                assert!(runtime.handle_chat_ui_action_with_suggestion_hit(action, Some(0)));

                let completed = format!("{prefix}\"Alice One\"");
                let expected = format!("{completed} di");
                assert_eq!(runtime.chat_editor().as_str(), expected);
                assert_eq!(runtime.chat_editor().cursor_byte(), completed.len());
                runtime.move_chat_cursor_end(false);
                if prefix.starts_with('/') {
                    assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
                    assert_eq!(runtime.chat_suggestions(), [Arc::from("dirt")]);
                }
                assert_eq!(
                    runtime.queue_chat_send(0).unwrap().message.as_ref(),
                    expected
                );
            }
        }
    }
}

#[test]
fn quoted_tab_completion_consumes_only_the_active_tokens_closing_quote() {
    for prefix in ["/give ", "héllo @"] {
        let mut player_runtime = player_state::PlayerState::new(2);
        let mut runtime = UiRuntime::new(2);
        runtime.refresh_raw_text_identities(
            |_| None,
            vec![Arc::from("Alice One"), Arc::from("Alice Two")],
        );
        runtime.open_chat(&mut player_runtime);
        runtime
            .apply(
                &mut player_runtime,
                envelope(2, 1, UiEvent::AvailableCommands(give_tree())),
            )
            .unwrap();
        runtime
            .insert_chat_text(&format!("{prefix}\"Alice \" \"café\""))
            .unwrap();
        runtime.mutate_chat_editor(|editor| editor.place_cursor(prefix.len() + "\"Alice ".len()));
        assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
        for (action, name) in [
            (UiAction::TabNext, "Alice One"),
            (UiAction::TabNext, "Alice Two"),
            (UiAction::TabPrevious, "Alice One"),
            (UiAction::TabNext, "Alice Two"),
        ] {
            assert!(runtime.handle_chat_ui_action(action));
            let completed = format!("{prefix}\"{name}\"");
            assert_eq!(
                runtime.chat_editor().as_str(),
                format!("{completed} \"café\"")
            );
            assert_eq!(runtime.chat_editor().cursor_byte(), completed.len());
        }
        assert!(dispatch_chat_ui_action(
            &mut runtime,
            UiAction::Accept,
            None,
            0
        ));
        assert!(runtime.chat_suggestions().is_empty());
        assert!(dispatch_chat_ui_action(
            &mut runtime,
            UiAction::Accept,
            None,
            0
        ));
        assert_eq!(
            runtime.pending_chat_sends()[0].message.as_ref(),
            format!("{prefix}\"Alice Two\" \"café\"")
        );
    }
}

#[test]
fn quoted_completion_at_the_input_limit_reuses_the_existing_closing_quote() {
    let mut player_runtime = player_state::PlayerState::new(2);
    let mut runtime = UiRuntime::new(2);
    runtime.refresh_raw_text_identities(|_| None, vec![Arc::from("Alice One")]);
    runtime.open_chat(&mut player_runtime);
    let mention = "@\"Alice One\"";
    let input = format!(
        "{}{mention}",
        "x".repeat(ui::MAX_CHAT_INPUT_BYTES - mention.len())
    );
    runtime.insert_chat_text(&input).unwrap();
    runtime.move_chat_cursor_left();
    assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
    assert!(runtime.handle_chat_ui_action(UiAction::Accept));
    assert_eq!(runtime.chat_editor().as_str(), input);
    assert_eq!(runtime.chat_editor().cursor_byte(), input.len());
    assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
    assert!(runtime.chat_suggestions().is_empty());
}

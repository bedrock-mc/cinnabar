//! Interactive chat, autocomplete, and outbound-send coverage split from the
//! ui_runtime test root to honor the architecture test-file line budget.

use super::*;

#[test]
fn accepted_chat_send_clears_editor_and_session_replacement_attributes_drops() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(11);

    let mut runtime = UiRuntime::new(11);
    runtime.set_chat_identity(Arc::from("Player"), Arc::from("1234"));
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("hello").unwrap();

    let request = runtime.queue_chat_send(100).unwrap();
    assert_eq!(request.session, 11);
    assert_eq!(request.sequence, 0);
    assert_eq!(request.message.as_ref(), "hello");
    assert!(runtime.chat_editor().as_str().is_empty());
    assert_eq!(runtime.pending_chat_sends().len(), 1);

    runtime.begin_session(&mut player_runtime, 12);
    assert!(runtime.pending_chat_sends().is_empty());
    assert_eq!(runtime.dropped_unsent_chat_messages(), 1);
}

#[test]
fn chat_packet_build_preserves_pending_request_until_transport_ack() {
    let mut runtime = UiRuntime::new(3);
    runtime.set_chat_identity(Arc::from("Alex"), Arc::from("xuid"));
    runtime.insert_chat_text("ordered").unwrap();
    runtime.queue_chat_send(0).unwrap();

    let (sequence, _packet) = runtime.front_chat_packet().unwrap().unwrap();
    assert_eq!(sequence, 0);
    assert_eq!(runtime.pending_chat_sends().len(), 1);
    assert!(!runtime.confirm_chat_send(1));
    assert!(runtime.confirm_chat_send(sequence));
    assert!(runtime.pending_chat_sends().is_empty());
}

#[test]
fn slash_chat_submission_uses_command_transport_without_consuming_the_request() {
    let mut runtime = UiRuntime::new(3);
    runtime.set_chat_identity(Arc::from("Alex"), Arc::from("xuid"));
    runtime.insert_chat_text("/kill @s").unwrap();
    runtime.queue_chat_send(0).unwrap();

    let (sequence, packet) = runtime.front_chat_packet().unwrap().unwrap();

    assert_eq!(sequence, 0);
    assert_eq!(packet.header.id as u32, 77);
    assert_eq!(runtime.pending_chat_sends().len(), 1);
}

struct ClipboardFixture(Option<Arc<str>>);

impl ChatClipboard for ClipboardFixture {
    type Error = ();

    fn read_text_bounded(
        &mut self,
        _maximum_bytes: usize,
    ) -> Result<Option<Arc<str>>, Self::Error> {
        Ok(self.0.take())
    }
}

#[test]
fn changed_editor_state_issues_one_complete_autocomplete_request_per_revision() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(5);

    let mut runtime = UiRuntime::new(5);
    runtime.open_chat(&mut player_runtime);

    runtime.insert_chat_text("").unwrap();
    assert!(runtime.take_chat_autocomplete_request().is_none());
    runtime.insert_chat_text("/gi").unwrap();
    let first = runtime.take_chat_autocomplete_request().unwrap();
    assert_eq!(first.session, 5);
    assert_eq!(first.input_revision, 1);
    assert_eq!(first.cursor_byte, 3);
    assert_eq!(first.input.as_ref(), "/gi");
    assert!(runtime.take_chat_autocomplete_request().is_none());

    runtime.move_chat_cursor_left();
    let second = runtime.take_chat_autocomplete_request().unwrap();
    assert_eq!(second.input_revision, 2);
    assert_eq!(second.cursor_byte, 2);
    assert_eq!(second.input.as_ref(), "/gi");
}

#[test]
fn autocomplete_response_and_ui_action_complete_the_editor_then_clear_on_close() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(2);

    let mut runtime = UiRuntime::new(2);
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("/g").unwrap();
    let request = runtime.take_chat_autocomplete_request().unwrap();
    runtime
        .apply(
            &mut player_runtime,
            envelope(
                2,
                1,
                UiEvent::ChatAutocomplete(ChatAutocompleteEvent {
                    enum_name: Arc::from("commands"),
                    action: ProtocolAutocompleteAction::Replace,
                    suggestions: Arc::from([Arc::from("/give"), Arc::from("/gamemode")]),
                }),
            ),
        )
        .unwrap();

    assert!(runtime.chat_suggestions().is_empty());
    assert!(runtime.complete_chat_autocomplete(&player_runtime, request));
    assert_eq!(runtime.chat_suggestions().len(), 2);
    runtime.handle_chat_ui_action(UiAction::Navigate([0, 1]));
    runtime.handle_chat_ui_action(UiAction::Accept);
    assert_eq!(runtime.chat_editor().as_str(), "/gamemode");

    runtime.close_chat();
    assert!(runtime.chat_suggestions().is_empty());
    assert!(runtime.take_chat_autocomplete_request().is_none());
}

#[test]
fn stale_autocomplete_request_cannot_apply_to_a_new_editor_revision() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(2);

    let mut runtime = UiRuntime::new(2);
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("/g").unwrap();
    let stale = runtime.take_chat_autocomplete_request().unwrap();
    runtime.insert_chat_text("i").unwrap();
    runtime.take_chat_autocomplete_request().unwrap();
    runtime
        .apply(
            &mut player_runtime,
            envelope(
                2,
                1,
                UiEvent::ChatAutocomplete(ChatAutocompleteEvent {
                    enum_name: Arc::from("commands"),
                    action: ProtocolAutocompleteAction::Replace,
                    suggestions: Arc::from([Arc::from("/give")]),
                }),
            ),
        )
        .unwrap();

    assert!(!runtime.complete_chat_autocomplete(&player_runtime, stale));
    assert!(runtime.chat_suggestions().is_empty());
}

#[test]
fn pending_autocomplete_request_is_serviced_once_against_catalog_revision() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(2);

    let mut runtime = UiRuntime::new(2);
    runtime.open_chat(&mut player_runtime);
    runtime
        .apply(
            &mut player_runtime,
            envelope(
                2,
                1,
                UiEvent::ChatAutocomplete(ChatAutocompleteEvent {
                    enum_name: Arc::from("commands"),
                    action: ProtocolAutocompleteAction::Replace,
                    suggestions: Arc::from([Arc::from("/give")]),
                }),
            ),
        )
        .unwrap();
    runtime.insert_chat_text("/g").unwrap();

    assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
    assert_eq!(runtime.chat_suggestions(), [Arc::from("/give")]);
    assert!(!runtime.service_pending_chat_autocomplete(&player_runtime));
}

#[test]
fn session_replacement_discards_the_prior_autocomplete_catalog() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(2);

    let mut runtime = UiRuntime::new(2);
    runtime
        .apply(
            &mut player_runtime,
            envelope(
                2,
                1,
                UiEvent::ChatAutocomplete(ChatAutocompleteEvent {
                    enum_name: Arc::from("commands"),
                    action: ProtocolAutocompleteAction::Replace,
                    suggestions: Arc::from([Arc::from("/give")]),
                }),
            ),
        )
        .unwrap();
    runtime.begin_session(&mut player_runtime, 3);
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("/g").unwrap();

    assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
    assert!(runtime.chat_suggestions().is_empty());
}

#[test]
fn history_navigation_replaces_the_presented_editor_text() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(4);

    let mut runtime = UiRuntime::new(4);
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("first").unwrap();
    runtime.queue_chat_send(0).unwrap();
    runtime.insert_chat_text("second").unwrap();
    runtime.queue_chat_send(500).unwrap();

    assert!(runtime.show_older_chat_history());
    assert_eq!(runtime.chat_editor().as_str(), "second");
    assert!(runtime.show_older_chat_history());
    assert_eq!(runtime.chat_editor().as_str(), "first");
    assert!(runtime.show_newer_chat_history());
    assert_eq!(runtime.chat_editor().as_str(), "second");
}

#[test]
fn fifo_flush_retries_backpressure_and_confirms_only_accepted_packets() {
    let mut runtime = UiRuntime::new(9);
    runtime.set_chat_identity(Arc::from("Alex"), Arc::from("xuid"));
    runtime.insert_chat_text("one").unwrap();
    runtime.queue_chat_send(0).unwrap();
    runtime.insert_chat_text("two").unwrap();
    runtime.queue_chat_send(500).unwrap();

    let error = flush_chat_sends(&mut runtime, 8, |_session, _sequence, _action, _packet| {
        Err("full")
    })
    .unwrap_err();
    assert_eq!(error, ChatFlushError::Transport("full"));
    assert_eq!(runtime.pending_chat_sends().len(), 2);

    let expected = [
        chat_text_packet("Alex", "xuid", "one").unwrap(),
        chat_text_packet("Alex", "xuid", "two").unwrap(),
    ];
    let mut sent = 0usize;
    assert_eq!(
        flush_chat_sends(&mut runtime, 8, |session, sequence, action, packet| {
            assert_eq!(session, 9);
            assert_eq!(sequence, sent as u64);
            assert_eq!(action, None);
            assert_eq!(packet, expected[sent]);
            sent += 1;
            Ok::<_, &str>(())
        })
        .unwrap(),
        1
    );
    assert_eq!(sent, 1);
    assert_eq!(runtime.pending_chat_sends().len(), 2);
    assert_eq!(runtime.in_flight_chat_send(), Some((9, 0)));
    assert_eq!(
        flush_chat_sends(
            &mut runtime,
            8,
            |_session, _sequence, _action, _packet| -> Result<(), &str> {
                panic!("an in-flight chat packet cannot be enqueued twice")
            }
        )
        .unwrap(),
        0
    );
    assert!(!runtime.acknowledge_chat_send(8, 0));
    assert!(runtime.acknowledge_chat_send(9, 0));

    assert_eq!(
        flush_chat_sends(&mut runtime, 8, |session, sequence, action, packet| {
            assert_eq!((session, sequence), (9, 1));
            assert_eq!(action, None);
            assert_eq!(packet, expected[1]);
            sent += 1;
            Ok::<_, &str>(())
        })
        .unwrap(),
        1
    );
    assert_eq!(sent, 2);
    assert!(runtime.acknowledge_chat_send(9, 1));
    assert!(runtime.pending_chat_sends().is_empty());
}

#[test]
fn fast_transfer_action_is_exact_and_carries_session_ordinal_identity() {
    let mut runtime = UiRuntime::new(7);
    runtime.set_chat_identity(Arc::from("Alex"), Arc::from("xuid"));
    runtime.insert_chat_text("/transfer sm3").unwrap();
    runtime.queue_chat_send(0).unwrap();

    let mut observed = None;
    flush_chat_sends(&mut runtime, 1, |session, sequence, action, packet| {
        assert_eq!(
            packet.header.id as u32, 77,
            "expected CommandRequest packet ID"
        );
        observed = action.map(|action| action.marker(session, sequence, 1_000_000));
        Ok::<_, &str>(())
    })
    .unwrap();
    assert_eq!(
        observed.as_deref(),
        Some(
            "RUST_MCBE_FAST_TRANSFER_ACTION={\"action_ordinal\":0,\"command\":\"/transfer sm3\",\"kind\":\"command_sent\",\"schema\":\"rust-mcbe-fast-transfer-action-v1\",\"sent_unix_ms\":1000000,\"session_generation\":7}"
        )
    );
}

#[test]
fn session_replacement_clears_editor_autocomplete_and_old_outbox() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("/old").unwrap();
    runtime.queue_chat_send(0).unwrap();
    runtime.insert_chat_text("/draft").unwrap();
    assert!(runtime.take_chat_autocomplete_request().is_some());

    runtime.begin_session(&mut player_runtime, 2);

    assert!(!runtime.chat_focused());
    assert!(runtime.chat_editor().as_str().is_empty());
    assert!(runtime.chat_suggestions().is_empty());
    assert!(runtime.pending_chat_sends().is_empty());
    assert_eq!(runtime.dropped_unsent_chat_messages(), 1);
}

#[test]
fn focused_chat_suppresses_production_gameplay_inputs() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    let mut cursor = CursorOptions {
        grab_mode: CursorGrabMode::Locked,
        visible: false,
        ..CursorOptions::default()
    };
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::KeyW);
    let mut mouse_buttons = ButtonInput::<MouseButton>::default();
    mouse_buttons.press(MouseButton::Left);
    let mut mouse_motion = AccumulatedMouseMotion {
        delta: Vec2::new(4.0, 2.0),
    };

    suppress_gameplay_input_for_chat(
        &player_runtime,
        &runtime,
        &mut cursor,
        &mut keys,
        &mut mouse_buttons,
        &mut mouse_motion,
    );

    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    assert!(cursor.visible);
    assert!(keys.get_pressed().next().is_none());
    assert!(mouse_buttons.get_pressed().next().is_none());
    assert_eq!(mouse_motion.delta, Vec2::ZERO);
}

#[test]
fn control_or_command_v_uses_the_bounded_clipboard_adapter() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    let mut clipboard = ClipboardFixture(Some(Arc::from("bounded paste")));

    assert!(paste_chat_shortcut(
        &mut runtime,
        KeyCode::KeyV,
        &keys,
        &mut clipboard,
    ));
    assert_eq!(runtime.chat_editor().as_str(), "bounded paste");

    keys.release(KeyCode::ControlLeft);
    assert!(!paste_chat_shortcut(
        &mut runtime,
        KeyCode::KeyV,
        &keys,
        &mut ClipboardFixture(None),
    ));
}

#[test]
fn controller_buttons_map_to_the_shared_ui_action_adapter() {
    assert_eq!(
        gamepad_chat_action(GamepadButton::DPadUp),
        Some(UiAction::Navigate([0, -1]))
    );
    assert_eq!(
        gamepad_chat_action(GamepadButton::South),
        Some(UiAction::Accept)
    );
    assert_eq!(
        gamepad_chat_action(GamepadButton::East),
        Some(UiAction::Cancel)
    );
    assert_eq!(gamepad_chat_action(GamepadButton::North), None);
}

#[test]
fn accept_and_cancel_actions_close_chat_and_restore_gameplay_authority() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("hello").unwrap();
    assert!(dispatch_chat_ui_action(
        &mut runtime,
        UiAction::Accept,
        None,
        0,
    ));
    assert!(!runtime.chat_focused());
    assert_eq!(runtime.pending_chat_sends().len(), 1);

    runtime.open_chat(&mut player_runtime);
    assert!(dispatch_chat_ui_action(
        &mut runtime,
        UiAction::Cancel,
        None,
        1,
    ));
    assert!(!runtime.chat_focused());
}

#[test]
fn closing_chat_immediately_regrabs_and_hides_the_gameplay_cursor() {
    let mut cursor = CursorOptions {
        grab_mode: CursorGrabMode::None,
        visible: true,
        ..CursorOptions::default()
    };
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::Enter);
    let mut mouse_buttons = ButtonInput::<MouseButton>::default();
    mouse_buttons.press(MouseButton::Left);
    let mut mouse_motion = AccumulatedMouseMotion {
        delta: Vec2::new(4.0, 2.0),
    };

    restore_gameplay_input_after_chat(
        &mut cursor,
        &mut keys,
        &mut mouse_buttons,
        &mut mouse_motion,
    );

    assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
    assert!(!cursor.visible);
    assert!(keys.get_pressed().next().is_none());
    assert!(mouse_buttons.get_pressed().next().is_none());
    assert_eq!(mouse_motion.delta, Vec2::ZERO);
}

#[test]
fn provided_suggestion_hit_selects_the_matching_scrolled_row() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    runtime
        .apply(
            &mut player_runtime,
            envelope(
                1,
                1,
                UiEvent::ChatAutocomplete(ChatAutocompleteEvent {
                    enum_name: Arc::from("commands"),
                    action: ProtocolAutocompleteAction::Replace,
                    suggestions: (0..12)
                        .map(|index| Arc::from(format!("/s{index}")))
                        .collect::<Vec<_>>()
                        .into(),
                }),
            ),
        )
        .unwrap();
    runtime.insert_chat_text("/").unwrap();
    assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
    for _ in 0..10 {
        runtime.handle_chat_ui_action(UiAction::Navigate([0, 1]));
    }
    assert_eq!(runtime.chat_selected_suggestion(), Some(10));

    assert!(runtime.handle_chat_ui_action_with_suggestion_hit(
        UiAction::PointerPrimary {
            position: UiPoint::new(20.0, 533.0).unwrap(),
            phase: PointerPhase::Pressed,
        },
        Some(4),
    ));
    assert_eq!(runtime.chat_editor().as_str(), "/s4");
}

fn warp_tree() -> protocol::CommandTreeEvent {
    let param = |name: &str, kind| protocol::CommandParam {
        name: Arc::from(name),
        optional: false,
        collapse_enum: false,
        kind,
    };
    let mode = protocol::CommandParamKind::Enum {
        name: Arc::from("Mode"),
        values: Arc::from([Arc::from("fast"), Arc::from("far"), Arc::from("slow")]),
    };
    protocol::CommandTreeEvent {
        commands: Arc::from([protocol::CommandSpec {
            name: Arc::from("warp"),
            aliases: Arc::from([]),
            permission: 0,
            overloads: Arc::from([Arc::from([
                param("mode", mode),
                param("who", protocol::CommandParamKind::Target),
            ])]),
        }]),
        soft_enums: Arc::from([]),
    }
}

#[test]
fn command_tree_drives_suggestions_usage_and_mid_line_token_replacement() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(2);

    let mut runtime = UiRuntime::new(2);
    runtime.open_chat(&mut player_runtime);
    runtime
        .apply(
            &mut player_runtime,
            envelope(2, 1, UiEvent::AvailableCommands(warp_tree())),
        )
        .unwrap();
    runtime.insert_chat_text("/warp fa").unwrap();
    assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
    assert_eq!(
        runtime.chat_suggestions(),
        [Arc::from("far"), Arc::from("fast")]
    );
    assert_eq!(
        runtime.chat_usage_hint(),
        Some("/warp <fast|far|slow> <who>")
    );

    runtime.handle_chat_ui_action(UiAction::Navigate([0, 1]));
    runtime.handle_chat_ui_action(UiAction::Accept);
    assert_eq!(runtime.chat_editor().as_str(), "/warp fast");
}

#[test]
fn tab_inserts_then_cycles_the_same_suggestion_list() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(2);

    let mut runtime = UiRuntime::new(2);
    runtime.open_chat(&mut player_runtime);
    runtime
        .apply(
            &mut player_runtime,
            envelope(2, 1, UiEvent::AvailableCommands(warp_tree())),
        )
        .unwrap();
    runtime.insert_chat_text("/warp f").unwrap();
    assert!(runtime.service_pending_chat_autocomplete(&player_runtime));

    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "/warp far");
    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "/warp fast");
    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "/warp far");
    assert_eq!(runtime.chat_suggestions().len(), 2);
    assert!(runtime.take_chat_autocomplete_request().is_none());
}

#[test]
fn typing_after_tab_restarts_completion() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(2);

    let mut runtime = UiRuntime::new(2);
    runtime.open_chat(&mut player_runtime);
    runtime
        .apply(
            &mut player_runtime,
            envelope(2, 1, UiEvent::AvailableCommands(warp_tree())),
        )
        .unwrap();
    runtime.insert_chat_text("/warp f").unwrap();
    assert!(runtime.service_pending_chat_autocomplete(&player_runtime));
    runtime.handle_chat_ui_action(UiAction::TabNext);
    runtime.insert_chat_text(" ").unwrap();
    assert!(runtime.take_chat_autocomplete_request().is_some());
    assert!(runtime.chat_suggestions().is_empty());
}

#[test]
fn local_chat_line_does_not_consume_the_next_server_sequence() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(2);

    let mut runtime = UiRuntime::new(2);
    runtime.push_local_chat_line(Arc::from("Saved screenshot as a.png"), 5);
    runtime.push_local_chat_line(Arc::from("Saved screenshot as b.png"), 6);
    runtime
        .apply(&mut player_runtime, envelope(2, 0, text("server")))
        .unwrap();
    assert_eq!(runtime.chat().messages().len(), 3);
}

#[test]
fn tab_cycles_whitespace_suggestions_as_complete_replacements() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(2);
    let mut runtime = UiRuntime::new(2);
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("/tell A").unwrap();
    let request = runtime.take_chat_autocomplete_request().unwrap();
    runtime
        .chat_autocomplete
        .apply(
            request,
            ui::ChatAutocompleteDelta {
                enum_name: Arc::from("players"),
                action: ui::ChatAutocompleteAction::Replace,
                suggestions: Arc::from([Arc::from("Alice One"), Arc::from("Alice Two")]),
            },
        )
        .unwrap();
    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "/tell Alice One");
    assert!(runtime.handle_chat_ui_action(UiAction::TabNext));
    assert_eq!(runtime.chat_editor().as_str(), "/tell Alice Two");
}

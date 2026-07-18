use std::sync::Arc;

use bevy::{
    input::{gamepad::GamepadButton, mouse::AccumulatedMouseMotion},
    prelude::{ButtonInput, KeyCode, MouseButton, Vec2},
    window::{CursorGrabMode, CursorOptions},
};
use protocol::{
    BlockCrackAction, BlockCrackEvent, ChatAutocompleteAction as ProtocolAutocompleteAction,
    ChatAutocompleteEvent, HudEvent, TextCategory, TextEvent, TextKind, TitleAction, TitleEvent,
    UiEvent, chat_text_packet,
};
use semantic_input::{Action, DeviceFrame, InputContext, KeyboardMouseFrame, SemanticInputRouter};
use ui::{ChatClipboard, PointerPhase, UiAction, UiPoint};

use super::*;

fn envelope(session_id: u64, sequence: u64, event: UiEvent) -> SequencedUiEvent {
    SequencedUiEvent {
        session_id,
        fifo_sequence: sequence,
        local_millis: sequence * 10,
        server_tick: None,
        event,
    }
}

fn text(message: &str) -> UiEvent {
    UiEvent::Text(TextEvent {
        category: TextCategory::MessageOnly,
        kind: TextKind::Chat,
        needs_translation: false,
        source: Some(Arc::from("player")),
        message: Arc::from(message),
        parameters: Arc::from([]),
        xuid: Arc::from(""),
        platform_chat_id: Arc::from(""),
        filtered_message: None,
    })
}

fn title(message: &str) -> UiEvent {
    UiEvent::Title(TitleEvent {
        action: TitleAction::SetTitle,
        text: Arc::from(message),
        fade_in_ticks: 10,
        stay_ticks: 70,
        fade_out_ticks: 20,
        xuid: Arc::from(""),
        platform_online_id: Arc::from(""),
        filtered_message: Arc::from(""),
    })
}

#[test]
fn session_replacement_clears_receive_side_ui_atomically() {
    let mut runtime = UiRuntime::new(1);
    runtime.apply(envelope(1, 1, text("old chat"))).unwrap();
    runtime.apply(envelope(1, 2, title("old title"))).unwrap();
    runtime
        .apply(envelope(
            1,
            3,
            UiEvent::Hud(HudEvent::Toast {
                title: Arc::from("old toast"),
                message: Arc::from("message"),
            }),
        ))
        .unwrap();

    runtime.begin_session(2);

    assert!(runtime.chat().messages().is_empty());
    assert!(runtime.hud().title().is_none());
    assert!(runtime.hud().toasts().is_empty());
    assert_eq!(runtime.session_id(), 2);
}

#[test]
fn stale_session_sequence_and_clock_fail_without_mutation() {
    let mut runtime = UiRuntime::new(7);
    runtime.apply(envelope(7, 10, text("accepted"))).unwrap();

    assert!(matches!(
        runtime.apply(envelope(6, 11, text("wrong session"))),
        Err(UiRuntimeError::WrongSession { .. })
    ));
    assert!(matches!(
        runtime.apply(envelope(7, 10, text("duplicate"))),
        Err(UiRuntimeError::StaleFifoSequence { .. })
    ));
    let mut backwards = envelope(7, 11, text("backwards"));
    backwards.local_millis = 1;
    assert!(matches!(
        runtime.apply(backwards),
        Err(UiRuntimeError::NonMonotonicLocalTime { .. })
    ));
    assert_eq!(runtime.chat().messages().len(), 1);
    assert_eq!(runtime.chat().messages()[0].message.as_ref(), "accepted");
}

#[test]
fn chat_focus_requests_context_and_router_releases_gameplay_actions() {
    let mut router = SemanticInputRouter::default();
    let held = KeyboardMouseFrame {
        activity_sequence: 1,
        keys: vec![0x1a],
        mouse_buttons: vec![1],
        ..KeyboardMouseFrame::default()
    };
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(held),
            ..DeviceFrame::default()
        })
        .unwrap();
    let pressed = router.finalize().unwrap();
    assert!(pressed.phases[Action::MoveForward as usize].held);
    assert!(pressed.phases[Action::Attack as usize].held);

    let mut runtime = UiRuntime::new(1);
    let transition = runtime.open_chat();
    assert!(transition.ui_consumed_text());
    assert_eq!(
        transition.requested_input_context(),
        InputContext::UiFocused
    );
    router.set_context(transition.requested_input_context());
    router.route(DeviceFrame::default()).unwrap();
    let released = router.finalize().unwrap();
    assert!(released.phases[Action::MoveForward as usize].released);
    assert!(released.phases[Action::Attack as usize].released);
}

#[test]
fn local_server_tick_drives_title_clock_when_present() {
    let mut runtime = UiRuntime::new(1);
    let mut event = envelope(1, 1, title("server clock"));
    event.local_millis = 9_000;
    event.server_tick = Some(20);
    runtime.apply(event).unwrap();

    assert_eq!(runtime.hud().title().unwrap().started_millis, 1_000);
}

#[test]
fn block_cracks_are_retained_in_sequence_and_cleared_on_session_change() {
    let mut runtime = UiRuntime::new(4);
    let event = BlockCrackEvent {
        position: [3, 64, -2],
        action: BlockCrackAction::Start {
            progress_per_tick: 1_024,
        },
    };

    runtime
        .retain_block_crack(SequencedBlockCrackEvent {
            session_id: 4,
            fifo_sequence: 7,
            dimension: 0,
            event,
        })
        .unwrap();

    assert_eq!(
        runtime.pending_block_cracks().front(),
        Some(&SequencedBlockCrackEvent {
            session_id: 4,
            fifo_sequence: 7,
            dimension: 0,
            event,
        })
    );
    assert!(matches!(
        runtime.retain_block_crack(SequencedBlockCrackEvent {
            session_id: 4,
            fifo_sequence: 7,
            dimension: 0,
            event,
        }),
        Err(UiRuntimeError::StaleBlockCrackSequence { .. })
    ));

    runtime.begin_session(5);
    assert!(runtime.pending_block_cracks().is_empty());
}

#[test]
fn block_crack_handoff_is_bounded_without_dropping_existing_events() {
    let mut runtime = UiRuntime::new(9);
    for sequence in 0..MAX_PENDING_BLOCK_CRACK_EVENTS {
        runtime
            .retain_block_crack(SequencedBlockCrackEvent {
                session_id: 9,
                fifo_sequence: sequence as u64,
                dimension: 1,
                event: BlockCrackEvent {
                    position: [sequence as i32, 0, 0],
                    action: BlockCrackAction::Stop,
                },
            })
            .unwrap();
    }

    let before = runtime.pending_block_cracks().clone();
    assert_eq!(
        runtime.retain_block_crack(SequencedBlockCrackEvent {
            session_id: 9,
            fifo_sequence: MAX_PENDING_BLOCK_CRACK_EVENTS as u64,
            dimension: 1,
            event: BlockCrackEvent {
                position: [0, 0, 0],
                action: BlockCrackAction::Stop,
            },
        }),
        Err(UiRuntimeError::BlockCrackQueueFull {
            maximum: MAX_PENDING_BLOCK_CRACK_EVENTS,
        })
    );
    assert_eq!(runtime.pending_block_cracks(), &before);
}

#[test]
fn local_actor_health_and_hunger_attributes_fan_into_hud_state() {
    let mut runtime = UiRuntime::new(1);
    let attributes = Arc::from(
        [
            ("minecraft:health", 17.5, 20.0),
            ("minecraft:player.hunger", 14.0, 20.0),
        ]
        .map(|(name, current, max)| protocol::ActorAttribute {
            name: Arc::from(name),
            min: 0.0,
            max,
            current,
            default: Some(max),
            modifiers: Arc::from([]),
        }),
    );

    runtime
        .apply_local_attributes(SequencedLocalAttributes {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 100,
            server_tick: 2,
            attributes,
        })
        .unwrap();

    assert_eq!(
        runtime.hud().health(),
        BoundedStat::new_scaled(1_750, 2_000, 100)
    );
    assert_eq!(
        runtime.hud().hunger(),
        BoundedStat::new_scaled(1_400, 2_000, 100)
    );
    assert_eq!(runtime.hud().view_nodes(100)[0].text.as_ref(), "17.5/20");
}

#[test]
fn partial_local_attributes_patch_without_clearing_authoritative_health() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(envelope(
            1,
            1,
            UiEvent::Hud(HudEvent::Health { health: 19 }),
        ))
        .unwrap();
    runtime
        .apply_local_attributes(SequencedLocalAttributes {
            session_id: 1,
            fifo_sequence: 2,
            local_millis: 20,
            server_tick: 1,
            attributes: Arc::from([protocol::ActorAttribute {
                name: Arc::from("minecraft:player.hunger"),
                min: 0.0,
                max: 20.0,
                current: 12.0,
                default: Some(20.0),
                modifiers: Arc::from([]),
            }]),
        })
        .unwrap();

    assert_eq!(runtime.hud().health(), BoundedStat::new(19, 20));
    assert_eq!(
        runtime.hud().hunger(),
        BoundedStat::new_scaled(1_200, 2_000, 100)
    );
}

#[test]
fn accepted_chat_send_clears_editor_and_session_replacement_attributes_drops() {
    let mut runtime = UiRuntime::new(11);
    runtime.set_chat_identity(Arc::from("Player"), Arc::from("1234"));
    runtime.open_chat();
    runtime.insert_chat_text("hello").unwrap();

    let request = runtime.queue_chat_send(100).unwrap();
    assert_eq!(request.session, 11);
    assert_eq!(request.sequence, 0);
    assert_eq!(request.message.as_ref(), "hello");
    assert!(runtime.chat_editor().as_str().is_empty());
    assert_eq!(runtime.pending_chat_sends().len(), 1);

    runtime.begin_session(12);
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
    let mut runtime = UiRuntime::new(5);
    runtime.open_chat();

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
    let mut runtime = UiRuntime::new(2);
    runtime.open_chat();
    runtime.insert_chat_text("/g").unwrap();
    let request = runtime.take_chat_autocomplete_request().unwrap();
    runtime
        .apply(envelope(
            2,
            1,
            UiEvent::ChatAutocomplete(ChatAutocompleteEvent {
                enum_name: Arc::from("commands"),
                action: ProtocolAutocompleteAction::Replace,
                suggestions: Arc::from([Arc::from("/give"), Arc::from("/gamemode")]),
            }),
        ))
        .unwrap();

    assert!(runtime.chat_suggestions().is_empty());
    assert!(runtime.complete_chat_autocomplete(request));
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
    let mut runtime = UiRuntime::new(2);
    runtime.open_chat();
    runtime.insert_chat_text("/g").unwrap();
    let stale = runtime.take_chat_autocomplete_request().unwrap();
    runtime.insert_chat_text("i").unwrap();
    runtime.take_chat_autocomplete_request().unwrap();
    runtime
        .apply(envelope(
            2,
            1,
            UiEvent::ChatAutocomplete(ChatAutocompleteEvent {
                enum_name: Arc::from("commands"),
                action: ProtocolAutocompleteAction::Replace,
                suggestions: Arc::from([Arc::from("/give")]),
            }),
        ))
        .unwrap();

    assert!(!runtime.complete_chat_autocomplete(stale));
    assert!(runtime.chat_suggestions().is_empty());
}

#[test]
fn pending_autocomplete_request_is_serviced_once_against_catalog_revision() {
    let mut runtime = UiRuntime::new(2);
    runtime.open_chat();
    runtime
        .apply(envelope(
            2,
            1,
            UiEvent::ChatAutocomplete(ChatAutocompleteEvent {
                enum_name: Arc::from("commands"),
                action: ProtocolAutocompleteAction::Replace,
                suggestions: Arc::from([Arc::from("/give")]),
            }),
        ))
        .unwrap();
    runtime.insert_chat_text("/g").unwrap();

    assert!(runtime.service_pending_chat_autocomplete());
    assert_eq!(runtime.chat_suggestions(), [Arc::from("/give")]);
    assert!(!runtime.service_pending_chat_autocomplete());
}

#[test]
fn session_replacement_discards_the_prior_autocomplete_catalog() {
    let mut runtime = UiRuntime::new(2);
    runtime
        .apply(envelope(
            2,
            1,
            UiEvent::ChatAutocomplete(ChatAutocompleteEvent {
                enum_name: Arc::from("commands"),
                action: ProtocolAutocompleteAction::Replace,
                suggestions: Arc::from([Arc::from("/give")]),
            }),
        ))
        .unwrap();
    runtime.begin_session(3);
    runtime.open_chat();
    runtime.insert_chat_text("/g").unwrap();

    assert!(runtime.service_pending_chat_autocomplete());
    assert!(runtime.chat_suggestions().is_empty());
}

#[test]
fn history_navigation_replaces_the_presented_editor_text() {
    let mut runtime = UiRuntime::new(4);
    runtime.open_chat();
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

    let error =
        flush_chat_sends(&mut runtime, 8, |_session, _sequence, _packet| Err("full")).unwrap_err();
    assert_eq!(error, ChatFlushError::Transport("full"));
    assert_eq!(runtime.pending_chat_sends().len(), 2);

    let expected = [
        chat_text_packet("Alex", "xuid", "one").unwrap(),
        chat_text_packet("Alex", "xuid", "two").unwrap(),
    ];
    let mut sent = 0usize;
    assert_eq!(
        flush_chat_sends(&mut runtime, 8, |session, sequence, packet| {
            assert_eq!(session, 9);
            assert_eq!(sequence, sent as u64);
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
            |_session, _sequence, _packet| -> Result<(), &str> {
                panic!("an in-flight chat packet cannot be enqueued twice")
            }
        )
        .unwrap(),
        0
    );
    assert!(!runtime.acknowledge_chat_send(8, 0));
    assert!(runtime.acknowledge_chat_send(9, 0));

    assert_eq!(
        flush_chat_sends(&mut runtime, 8, |session, sequence, packet| {
            assert_eq!((session, sequence), (9, 1));
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
fn session_replacement_clears_editor_autocomplete_and_old_outbox() {
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat();
    runtime.insert_chat_text("/old").unwrap();
    runtime.queue_chat_send(0).unwrap();
    runtime.insert_chat_text("/draft").unwrap();
    assert!(runtime.take_chat_autocomplete_request().is_some());

    runtime.begin_session(2);

    assert!(!runtime.chat_focused());
    assert!(runtime.chat_editor().as_str().is_empty());
    assert!(runtime.chat_suggestions().is_empty());
    assert!(runtime.pending_chat_sends().is_empty());
    assert_eq!(runtime.dropped_unsent_chat_messages(), 1);
}

#[test]
fn focused_chat_suppresses_production_gameplay_inputs() {
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat();
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
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat();
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
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat();
    runtime.insert_chat_text("hello").unwrap();
    assert!(dispatch_chat_ui_action(
        &mut runtime,
        UiAction::Accept,
        None,
        0,
    ));
    assert!(!runtime.chat_focused());
    assert_eq!(runtime.pending_chat_sends().len(), 1);

    runtime.open_chat();
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
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat();
    runtime
        .apply(envelope(
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
        ))
        .unwrap();
    runtime.insert_chat_text("/").unwrap();
    assert!(runtime.service_pending_chat_autocomplete());
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

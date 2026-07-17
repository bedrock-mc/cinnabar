use std::sync::Arc;

use protocol::{
    BlockCrackAction, BlockCrackEvent, HudEvent, TextCategory, TextEvent, TextKind, TitleAction,
    TitleEvent, UiEvent,
};
use semantic_input::{Action, DeviceFrame, InputContext, KeyboardMouseFrame, SemanticInputRouter};

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
    let attributes = [
        ("minecraft:health", 17.5, 20.0),
        ("minecraft:player.hunger", 14.0, 20.0),
    ]
    .into_iter()
    .map(|(name, current, max)| {
        (
            Arc::from(name),
            protocol::ActorAttribute {
                name: Arc::from(name),
                min: 0.0,
                max,
                current,
                default: Some(max),
                modifiers: Arc::from([]),
            },
        )
    })
    .collect();

    runtime.sync_local_attributes(&attributes);

    assert_eq!(runtime.hud().health(), BoundedStat::new(1_750, 2_000));
    assert_eq!(runtime.hud().hunger(), BoundedStat::new(1_400, 2_000));
}

#[test]
fn accepted_chat_send_clears_editor_and_session_replacement_attributes_drops() {
    let mut runtime = UiRuntime::new(11);
    runtime.set_chat_identity(Arc::from("Player"), Arc::from("1234"));
    runtime.open_chat();
    runtime.chat_editor_mut().insert("hello").unwrap();

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
    runtime.chat_editor_mut().insert("ordered").unwrap();
    runtime.queue_chat_send(0).unwrap();

    let (sequence, _packet) = runtime.front_chat_packet().unwrap().unwrap();
    assert_eq!(sequence, 0);
    assert_eq!(runtime.pending_chat_sends().len(), 1);
    assert!(!runtime.confirm_chat_send(1));
    assert!(runtime.confirm_chat_send(sequence));
    assert!(runtime.pending_chat_sends().is_empty());
}

use std::sync::Arc;

use bevy::{
    input::{gamepad::GamepadButton, mouse::AccumulatedMouseMotion},
    prelude::{ButtonInput, KeyCode, MouseButton, Vec2},
    window::{CursorGrabMode, CursorOptions},
};
use protocol::{
    BedrockSession, BlockCrackAction, BlockCrackEvent, BossAction as ProtocolBossAction,
    BossColor as ProtocolBossColor, BossEvent, BossOverlay as ProtocolBossOverlay,
    BossStyle as ProtocolBossStyle, ChatAutocompleteAction as ProtocolAutocompleteAction,
    ChatAutocompleteEvent, CommandOutputEvent, CommandOutputMessage, HudEvent, ObjectiveEvent,
    RawTextEvent, ScoreAction as ProtocolScoreAction, ScoreEntry as ProtocolScoreEntry, ScoreEvent,
    ScoreIdentity as ProtocolScoreIdentity, TextCategory, TextEvent, TextKind, TitleAction,
    TitleEvent, UiEvent, WorldEvent, chat_text_packet, decode_batch, into_world_event,
    parse_raw_text,
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

#[test]
fn protocol_1001_raw_text_reaches_chat_store_as_human_text_not_json() {
    const FIXTURE: &[u8] =
        include_bytes!("../../../crates/protocol/fixtures/text_object_rawtext.bin");
    let mut packets = decode_batch(FIXTURE.into(), &BedrockSession { shield_item_id: 0 }).unwrap();
    let event = match into_world_event(packets.pop().unwrap(), 0).unwrap() {
        Some(WorldEvent::Ui(event @ UiEvent::RawText(_))) => event,
        other => panic!("expected RawText UI event, got {other:?}"),
    };
    let mut runtime = UiRuntime::new(1);
    runtime.apply(envelope(1, 1, event)).unwrap();

    let message = runtime.chat().messages().back().unwrap();
    assert_eq!(message.message.as_ref(), "\u{a7}aLBSG human chat");
    assert!(!message.message.contains('{'));
    assert!(!message.message.contains("rawtext"));
}

#[test]
fn local_hotbar_selection_is_client_authoritative_until_session_reset() {
    let mut runtime = UiRuntime::new(1);
    // Survival game mode defaults the highlight to slot 0.
    runtime.publish_player_game_mode(protocol::PlayerGameMode::Survival);
    assert_eq!(runtime.selected_hotbar_slot(), Some(0));

    // A local selection (number key / scroll) is predicted immediately and wins.
    runtime.set_local_selected_slot(4);
    assert_eq!(runtime.selected_hotbar_slot(), Some(4));

    // A later server equipment event for the local player does not override the local prediction.
    runtime.retain_local_selected_equipment(
        7,
        protocol::EquipmentEvent {
            actor_runtime_id: 42,
            stack: protocol::NetworkItemStack::empty(),
            inventory_slot: 0,
            selected_slot: 2,
            window_id: 0,
            handedness: None,
        },
    );
    assert_eq!(runtime.selected_hotbar_slot(), Some(4));

    // A new session clears the local prediction (and every other per-session field).
    runtime.begin_session(2);
    assert_eq!(runtime.selected_hotbar_slot(), None);
}

#[test]
fn command_output_rows_from_one_packet_reach_chat_in_order() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(envelope(
            1,
            1,
            UiEvent::CommandOutput(CommandOutputEvent {
                output_type: Arc::from("all_output"),
                success_count: 1,
                messages: Arc::from([
                    CommandOutputMessage {
                        message_id: Arc::from("commands.transfer.started"),
                        success: true,
                        parameters: Arc::from([Arc::from("sm3")]),
                    },
                    CommandOutputMessage {
                        message_id: Arc::from("commands.transfer.finished"),
                        success: true,
                        parameters: Arc::from([]),
                    },
                ]),
                data: None,
            }),
        ))
        .unwrap();

    assert_eq!(runtime.chat().messages().len(), 2);
    assert_eq!(runtime.chat().messages()[0].fifo_sequence, 1);
    assert_eq!(
        runtime.chat().messages()[0].message.as_ref(),
        "commands.transfer.started"
    );
    assert_eq!(runtime.chat().messages()[1].fifo_sequence, 1);
    assert_eq!(
        runtime.chat().messages()[1].message.as_ref(),
        "commands.transfer.finished"
    );
    runtime.apply(envelope(1, 2, text("later"))).unwrap();
}

#[test]
fn resolver_backed_raw_text_presents_human_text_instead_of_dropping_messages() {
    const FIXTURE: &[u8] =
        include_bytes!("../../../crates/protocol/fixtures/text_object_whisper_rawtext.bin");
    let mut packets = decode_batch(FIXTURE.into(), &BedrockSession { shield_item_id: 0 }).unwrap();
    let event = match into_world_event(packets.pop().unwrap(), 0).unwrap() {
        Some(WorldEvent::Ui(event @ UiEvent::RawText(_))) => event,
        other => panic!("expected RawText UI event, got {other:?}"),
    };
    let mut runtime = UiRuntime::new(1);

    // The captured whisper carries resolver-dependent components; they now
    // degrade per the vanilla rules and present as human text, never JSON and
    // never a silently dropped message.
    assert_eq!(
        runtime.apply(envelope(1, 1, event)).unwrap(),
        UiApplyOutcome::Applied
    );
    let message = runtime.chat().messages().back().unwrap();
    assert!(!message.message.contains('{'));
    assert!(!message.message.contains("rawtext"));
}

fn literal_raw_text(kind: TextKind, json: &str) -> UiEvent {
    let document = parse_raw_text(json).unwrap();
    UiEvent::RawText(RawTextEvent {
        text: TextEvent {
            category: TextCategory::MessageOnly,
            kind,
            needs_translation: false,
            source: None,
            message: Arc::from(document.literal_text()),
            parameters: Arc::from([]),
            xuid: Arc::from(""),
            platform_chat_id: Arc::from(""),
            filtered_message: None,
        },
        document,
    })
}

#[test]
fn typed_rawtext_routes_by_packet_surface_without_cross_presenting() {
    let mut runtime = UiRuntime::new(1);

    runtime
        .apply(envelope(
            1,
            1,
            literal_raw_text(
                TextKind::Raw,
                r#"{"rawtext":[{"text":"Transfer accepted"}]}"#,
            ),
        ))
        .unwrap();
    runtime
        .apply(envelope(
            1,
            2,
            literal_raw_text(TextKind::Tip, r#"{"rawtext":[{"text":"Action prompt"}]}"#),
        ))
        .unwrap();

    assert_eq!(runtime.chat().messages().len(), 1);
    assert_eq!(
        runtime.chat().messages()[0].message.as_ref(),
        "Transfer accepted"
    );
    assert_eq!(
        runtime.hud().actionbar().unwrap().text.as_ref(),
        "Action prompt"
    );
    assert!(runtime.hud().title().is_none());
    assert!(runtime.scoreboards().sidebar().is_none());
    assert!(runtime.boss_bars().stacked().is_empty());
}

fn title_object(action: TitleAction, json: &str) -> UiEvent {
    let document = parse_raw_text(json).unwrap();
    UiEvent::Title(TitleEvent {
        action,
        text: Arc::from(document.literal_text()),
        document: Some(document),
        fade_in_ticks: 10,
        stay_ticks: 70,
        fade_out_ticks: 20,
        xuid: Arc::from(""),
        platform_online_id: Arc::from(""),
        filtered_message: Arc::from(""),
    })
}

#[test]
fn literal_title_object_actions_apply_human_text_without_json_leakage() {
    let json = r#"{"rawtext":[{"text":"Human title"}]}"#;
    let mut runtime = UiRuntime::new(1);

    runtime
        .apply(envelope(
            1,
            1,
            title_object(TitleAction::SetTitleJson, json),
        ))
        .unwrap();
    runtime
        .apply(envelope(
            1,
            2,
            title_object(TitleAction::SetSubtitleJson, json),
        ))
        .unwrap();
    runtime
        .apply(envelope(
            1,
            3,
            title_object(TitleAction::ActionBarJson, json),
        ))
        .unwrap();

    for presented in [
        runtime.hud().title().unwrap(),
        runtime.hud().subtitle().unwrap(),
        runtime.hud().actionbar().unwrap(),
    ] {
        assert_eq!(presented.text.as_ref(), "Human title");
        assert!(!presented.text.contains("rawtext"));
        assert!(!presented.text.contains('{'));
    }
    assert!(runtime.chat().messages().is_empty());
    assert!(runtime.scoreboards().sidebar().is_none());
    assert!(runtime.boss_bars().stacked().is_empty());
}

#[test]
fn resolver_title_object_actions_present_degraded_components_without_json() {
    // An unknown translation key presents its raw key (the vanilla unknown-key
    // behavior) beside the literal text, never JSON and never a dropped title.
    let json = r#"{"rawtext":[{"text":"partial "},{"translate":"key"}]}"#;
    type ReadTitle = fn(&UiRuntime) -> Option<Arc<str>>;
    let cases: [(TitleAction, ReadTitle); 3] = [
        (TitleAction::SetTitleJson, |runtime| {
            runtime.hud().title().map(|title| Arc::clone(&title.text))
        }),
        (TitleAction::SetSubtitleJson, |runtime| {
            runtime
                .hud()
                .subtitle()
                .map(|subtitle| Arc::clone(&subtitle.text))
        }),
        (TitleAction::ActionBarJson, |runtime| {
            runtime
                .hud()
                .actionbar()
                .map(|actionbar| Arc::clone(&actionbar.text))
        }),
    ];
    for (action, read) in cases {
        let mut runtime = UiRuntime::new(1);
        assert_eq!(
            runtime
                .apply(envelope(1, 1, title_object(action, json)))
                .unwrap(),
            UiApplyOutcome::Applied
        );
        let presented = read(&runtime).expect("resolved title text is presented");
        assert_eq!(presented.as_ref(), "partial key");
        assert!(!presented.contains('{'));
    }
}

#[test]
fn rawtext_scores_resolve_from_the_retained_scoreboard_and_reader_identity() {
    let mut runtime = UiRuntime::new(1);
    runtime.set_chat_identity(Arc::from("Hashim"), Arc::from("1"));
    runtime
        .apply(envelope(
            1,
            1,
            UiEvent::Objective(ObjectiveEvent::Display {
                display_slot: Arc::from("sidebar"),
                objective_name: Arc::from("coins"),
                display_name: Arc::from("Coins"),
                criteria_name: Arc::from("dummy"),
                sort_order: 1,
            }),
        ))
        .unwrap();
    runtime
        .apply(envelope(
            1,
            2,
            UiEvent::Score(ScoreEvent {
                action: ProtocolScoreAction::Change,
                entries: Arc::from([
                    ProtocolScoreEntry {
                        scoreboard_id: 1,
                        objective_name: Arc::from("coins"),
                        score: 250,
                        identity: ProtocolScoreIdentity::FakePlayer(Arc::from("Hashim")),
                    },
                    ProtocolScoreEntry {
                        scoreboard_id: 2,
                        objective_name: Arc::from("coins"),
                        score: 9,
                        identity: ProtocolScoreIdentity::FakePlayer(Arc::from("Steve")),
                    },
                ]),
            }),
        ))
        .unwrap();

    // `*` resolves to the reader; a named owner resolves to its row; a
    // selector degrades to empty rather than exposing the component.
    let json = r#"{"rawtext":[{"text":"You: "},{"score":{"name":"*","objective":"coins"}},{"text":" Steve: "},{"score":{"name":"Steve","objective":"coins"}},{"selector":"@a"}]}"#;
    runtime
        .apply(envelope(1, 3, literal_raw_text(TextKind::Raw, json)))
        .unwrap();
    let message = runtime.chat().messages().back().unwrap();
    assert_eq!(message.message.as_ref(), "You: 250 Steve: 9");

    // An unknown owner or objective degrades to empty text, keeping the rest.
    let missing = r#"{"rawtext":[{"text":"Alex: "},{"score":{"name":"Alex","objective":"coins"}},{"text":"!"}]}"#;
    runtime
        .apply(envelope(1, 4, literal_raw_text(TextKind::Raw, missing)))
        .unwrap();
    assert_eq!(
        runtime.chat().messages().back().unwrap().message.as_ref(),
        "Alex: !"
    );
}

fn title(message: &str) -> UiEvent {
    UiEvent::Title(TitleEvent {
        action: TitleAction::SetTitle,
        text: Arc::from(message),
        document: None,
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
    runtime
        .apply(envelope(
            1,
            4,
            UiEvent::Objective(ObjectiveEvent::Display {
                display_slot: Arc::from("sidebar"),
                objective_name: Arc::from("kills"),
                display_name: Arc::from("Kills"),
                criteria_name: Arc::from("dummy"),
                sort_order: 1,
            }),
        ))
        .unwrap();
    runtime
        .apply(envelope(
            1,
            5,
            UiEvent::Boss(BossEvent {
                target_entity_id: 99,
                player_id: 7,
                action: ProtocolBossAction::Show,
                title: Arc::from("Old boss"),
                filtered_title: Arc::from(""),
                progress: 0.5,
                style: ProtocolBossStyle {
                    color: ProtocolBossColor::Red,
                    overlay: ProtocolBossOverlay::Notched10,
                    darken_sky: None,
                    create_world_fog: None,
                },
            }),
        ))
        .unwrap();

    assert!(runtime.hud().title().is_some());
    assert_eq!(runtime.boss_bars().stacked().len(), 1);

    runtime.begin_session(2);

    assert!(runtime.chat().messages().is_empty());
    assert!(runtime.hud().title().is_none());
    assert!(runtime.hud().toasts().is_empty());
    assert!(runtime.scoreboards().sidebar().is_none());
    assert!(runtime.boss_bars().stacked().is_empty());
    assert_eq!(runtime.session_id(), 2);
}

#[test]
fn protocol_scoreboard_and_boss_events_route_into_ui_owned_state() {
    let mut runtime = UiRuntime::new(3);
    assert_eq!(
        runtime
            .apply(envelope(
                3,
                1,
                UiEvent::Objective(ObjectiveEvent::Display {
                    display_slot: Arc::from("sidebar"),
                    objective_name: Arc::from("wins"),
                    display_name: Arc::from("Wins"),
                    criteria_name: Arc::from("dummy"),
                    sort_order: 1,
                }),
            ))
            .unwrap(),
        UiApplyOutcome::Applied
    );
    runtime
        .apply(envelope(
            3,
            2,
            UiEvent::Score(ScoreEvent {
                action: ProtocolScoreAction::Change,
                entries: Arc::from([ProtocolScoreEntry {
                    scoreboard_id: 8,
                    objective_name: Arc::from("wins"),
                    score: 12,
                    identity: ProtocolScoreIdentity::FakePlayer(Arc::from("player")),
                }]),
            }),
        ))
        .unwrap();
    runtime
        .apply(envelope(
            3,
            3,
            UiEvent::Boss(BossEvent {
                target_entity_id: 44,
                player_id: 3,
                action: ProtocolBossAction::Show,
                title: Arc::from("Boss"),
                filtered_title: Arc::from(""),
                progress: 0.25,
                style: ProtocolBossStyle {
                    color: ProtocolBossColor::Green,
                    overlay: ProtocolBossOverlay::Progress,
                    darken_sky: Some(false),
                    create_world_fog: Some(true),
                },
            }),
        ))
        .unwrap();

    let sidebar = runtime.scoreboards().sidebar().unwrap();
    assert_eq!(sidebar.rows.len(), 1);
    assert_eq!(sidebar.rows[0].identity.entry_id, 8);
    assert_eq!(sidebar.rows[0].score, 12);
    let bosses = runtime.boss_bars().stacked();
    assert_eq!(bosses.len(), 1);
    assert_eq!(bosses[0].target_entity_id, 44);
    assert_eq!(bosses[0].style.color, ui::BossColor::Green);
    assert!(runtime.chat().messages().is_empty());
    assert!(runtime.hud().title().is_none());
    assert!(runtime.hud().actionbar().is_none());

    let before_scoreboard = runtime.scoreboards().sidebar();
    let before_bosses = runtime.boss_bars().stacked();
    assert!(matches!(
        runtime.apply(envelope(
            3,
            3,
            UiEvent::Objective(ObjectiveEvent::Remove {
                objective_name: Arc::from("wins"),
            })
        )),
        Err(UiRuntimeError::StaleFifoSequence { .. })
    ));
    assert_eq!(runtime.scoreboards().sidebar(), before_scoreboard);
    assert_eq!(runtime.boss_bars().stacked(), before_bosses);
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
fn local_experience_attributes_populate_the_hud_xp_bar() {
    let mut runtime = UiRuntime::new(1);
    let attributes = Arc::from(
        [
            ("minecraft:player.experience", 0.42, 1.0),
            ("minecraft:player.level", 7.0, 24791.0),
        ]
        .map(|(name, current, max)| protocol::ActorAttribute {
            name: Arc::from(name),
            min: 0.0,
            max,
            current,
            default: Some(0.0),
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

    let xp = runtime
        .hud()
        .experience()
        .expect("experience attributes populate the HUD XP state");
    assert_eq!(xp.level, 7);
    assert!((xp.progress - 0.42).abs() < 1e-6);
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
fn gameplay_touch_targets_cover_movement_jump_use_look_and_release_transitions() {
    use crate::semantic_controls::SemanticTouchTargets;
    use crate::ui_runtime::gameplay_touch::{
        GameplayTouchSample, reconcile_gameplay_touch_targets,
    };

    let mut targets = SemanticTouchTargets::default();
    reconcile_gameplay_touch_targets(
        &mut targets,
        &[
            GameplayTouchSample::new(1, [0.25, 0.75], [0.0, 0.0]),
            GameplayTouchSample::new(2, [0.75, 0.75], [0.0, 0.0]),
            GameplayTouchSample::new(3, [0.90, 0.75], [0.0, 0.0]),
            GameplayTouchSample::new(4, [0.70, 0.40], [0.08, 0.01]),
            GameplayTouchSample::new(5, [0.25, 0.25], [0.0, 0.0]),
        ],
    );

    assert!(targets.is_movement(1));
    assert_eq!(targets.target(1), None);
    assert_eq!(targets.target(2), Some(semantic_input::touch::JUMP));
    assert_eq!(targets.target(3), Some(semantic_input::touch::USE));
    assert_eq!(targets.target(4), Some(semantic_input::touch::LOOK_RIGHT));
    assert!(!targets.is_movement(5));
    assert_eq!(targets.target(5), None);

    reconcile_gameplay_touch_targets(
        &mut targets,
        &[GameplayTouchSample::new(4, [0.62, 0.40], [-0.08, 0.01])],
    );
    assert!(!targets.is_movement(1));
    assert_eq!(targets.target(2), None);
    assert_eq!(targets.target(3), None);
    assert_eq!(targets.target(4), Some(semantic_input::touch::LOOK_LEFT));

    reconcile_gameplay_touch_targets(&mut targets, &[]);
    assert_eq!(targets.target(4), None);
}

mod chat_tests;
mod gameplay_hud_tests;
mod retained_bounds_tests;

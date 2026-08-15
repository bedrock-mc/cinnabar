use bytes::{BufMut, BytesMut};
use protocol::{
    BedrockSession, BossAction, BossColor, ChatAutocompleteAction, MAX_CHAT_AUTOCOMPLETE,
    MAX_FORM_JSON_BYTES, MAX_SCORE_ENTRIES_PER_PACKET, MAX_UI_TEXT_BYTES, UiEvent, UiPacketError,
    WorldEvent, decode_batch, into_world_event,
};
use valentine::bedrock::version::v1_26_40::{
    ActorUniqueId, BossEventPacket, CommandOutput, CommandOutputMessage, CommandOutputPacket,
    EnumsBossBarColor, EnumsBossBarOverlay, EnumsBossEventUpdateType, EnumsPlayStatus,
    EnumsSetTitlePacketPayloadTitleType, EnumsSoftEnumUpdateType, LevelEventPacket, McpePacketName,
    ModalFormRequestPacket, PlayStatusPacket, SetHealthPacket, SetScorePacket,
    SetScorePacketScoreInfoItem, SetTitlePacket, TextPacket, TextPacketBody,
    TextPacketPayloadMessageOnly, ToastRequestPacket, UpdateSoftEnumPacket, Vec3,
};
use valentine::protocol::wire;

const TEXT_FIXTURE: &[u8] = include_bytes!("../fixtures/text.bin");
const TITLE_FIXTURE: &[u8] = include_bytes!("../fixtures/set_title.bin");
const BOSS_FIXTURE: &[u8] = include_bytes!("../fixtures/boss_event.bin");
const FORM_FIXTURE: &[u8] = include_bytes!("../fixtures/modal_form_request.bin");

/// gophertunnel's `LevelEventStartBlockCracking` / `LevelEventStopBlockCracking`
/// / `LevelEventUpdateBlockCracking` (`minecraft/protocol/packet/level_event.go`).
/// 1.26.40 carries LevelEvent as a raw `event_id` rather than a named enum.
const LEVEL_EVENT_START_BLOCK_CRACKING: i32 = 3600;
const LEVEL_EVENT_UPDATE_BLOCK_CRACKING: i32 = 3602;

fn ui(packet: impl Into<protocol::Packet>) -> Result<UiEvent, UiPacketError> {
    match into_world_event(packet.into(), 0) {
        Ok(Some(WorldEvent::Ui(event))) => Ok(event),
        Ok(other) => panic!("expected UI event, got {other:?}"),
        Err(protocol::WorldPacketError::Ui(error)) => Err(error),
        Err(other) => panic!("unexpected world packet error: {other}"),
    }
}

fn decode_ui_fixture(bytes: &'static [u8]) -> UiEvent {
    let mut packets = decode_batch(bytes.into(), &BedrockSession { shield_item_id: 0 })
        .expect("decode pinned UI fixture");
    assert_eq!(packets.len(), 1);
    match into_world_event(packets.pop().unwrap(), 0).expect("normalize pinned UI fixture") {
        Some(WorldEvent::Ui(event)) => event,
        other => panic!("expected one UI event, got {other:?}"),
    }
}

fn raw_text_packet(message: String) -> TextPacket {
    TextPacket {
        body: TextPacketBody::Raw(TextPacketPayloadMessageOnly { message }),
        ..Default::default()
    }
}

#[test]
fn pinned_gophertunnel_ui_fixtures_normalize_without_vendor_types() {
    assert!(matches!(decode_ui_fixture(TEXT_FIXTURE), UiEvent::Text(_)));
    assert!(matches!(
        decode_ui_fixture(TITLE_FIXTURE),
        UiEvent::Title(_)
    ));
    assert!(matches!(decode_ui_fixture(BOSS_FIXTURE), UiEvent::Boss(_)));
    assert!(matches!(decode_ui_fixture(FORM_FIXTURE), UiEvent::Form(_)));
}

#[test]
fn representative_ui_packets_normalize_without_vendor_types() {
    let text = raw_text_packet("§ahello".to_owned());
    let title = SetTitlePacket {
        title_type: EnumsSetTitlePacketPayloadTitleType::Title,
        title_text: "Round one".to_owned(),
        fade_in_time: 5,
        stay_time: 40,
        fade_out_time: 10,
        ..Default::default()
    };
    // `Add` is wire value 0, gophertunnel's `BossEventShow`.
    let boss = BossEventPacket {
        target_actor_id: ActorUniqueId {
            actor_unique_id: 17,
        },
        event_type: EnumsBossEventUpdateType::Add,
        name: "Dragon".to_owned(),
        health_percent: 0.75,
        color: EnumsBossBarColor::RebeccaPurple,
        overlay: EnumsBossBarOverlay::Notched10,
        ..Default::default()
    };
    let form = ModalFormRequestPacket {
        form_id: 91,
        form_uijson: r#"{"type":"form","title":"Pick"}"#.to_owned(),
    };

    assert!(matches!(ui(text).unwrap(), UiEvent::Text(_)));
    assert!(matches!(ui(title).unwrap(), UiEvent::Title(_)));
    let UiEvent::Boss(boss) = ui(boss).unwrap() else {
        panic!("expected boss event")
    };
    assert_eq!(boss.action, BossAction::Show);
    assert_eq!(boss.style.color, BossColor::RebeccaPurple);
    assert_eq!(boss.style.darken_sky, None);
    assert_eq!(boss.style.create_world_fog, None);
    assert!(matches!(ui(form).unwrap(), UiEvent::Form(_)));
    assert!(matches!(
        ui(SetHealthPacket { health: 19 }).unwrap(),
        UiEvent::Hud(protocol::HudEvent::Health { health: 19 })
    ));
    assert!(matches!(
        ui(PlayStatusPacket {
            status: EnumsPlayStatus::PlayerSpawn,
        })
        .unwrap(),
        UiEvent::Hud(protocol::HudEvent::PlayerStatus(
            protocol::PlayerStatus::PlayerSpawn
        ))
    ));
    let toast = ToastRequestPacket {
        title: "Saved".to_owned(),
        content: "world backed up".to_owned(),
    };
    let UiEvent::Hud(protocol::HudEvent::Toast { title, message }) = ui(toast).unwrap() else {
        panic!("expected toast event")
    };
    assert_eq!(title.as_ref(), "Saved");
    assert_eq!(message.as_ref(), "world backed up");
    // `Replace` is wire value 2, gophertunnel's `SoftEnumActionSet`.
    let autocomplete = UpdateSoftEnumPacket {
        enum_name: "commands".to_owned(),
        values: vec!["give".to_owned(), "gamerule".to_owned()],
        update_type: EnumsSoftEnumUpdateType::Replace,
    };
    let UiEvent::ChatAutocomplete(autocomplete) = ui(autocomplete).unwrap() else {
        panic!("expected autocomplete update")
    };
    assert_eq!(autocomplete.enum_name.as_ref(), "commands");
    assert_eq!(autocomplete.action, ChatAutocompleteAction::Replace);
    assert_eq!(
        autocomplete
            .suggestions
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>(),
        ["give", "gamerule"]
    );
}

#[test]
fn command_output_is_bounded_and_normalized_for_chat_presentation() {
    let packet = CommandOutputPacket {
        output: CommandOutput {
            output_type: "all_output".to_owned(),
            success_count: 1,
            output_messages: vec![CommandOutputMessage {
                message_id: "commands.generic.success".to_owned(),
                successful: true,
                parameters: vec!["sm3".to_owned()],
            }],
            data_set: Some("transfer accepted".to_owned()),
        },
        ..Default::default()
    };
    let UiEvent::CommandOutput(output) = ui(packet).unwrap() else {
        panic!("expected command output event")
    };
    assert_eq!(output.output_type.as_ref(), "all_output");
    assert_eq!(output.success_count, 1);
    assert_eq!(output.messages.len(), 1);
    assert_eq!(
        output.messages[0].message_id.as_ref(),
        "commands.generic.success"
    );
    assert_eq!(output.messages[0].parameters[0].as_ref(), "sm3");
    assert_eq!(output.data.as_deref(), Some("transfer accepted"));
}

#[test]
fn score_entries_carry_their_own_verb() {
    use valentine::bedrock::version::v1_26_40::{ChangeFakePlayerScore, RemoveScore, ScoreboardId};

    // 1.26.40 moved the add/remove verb into each entry, so one packet may mix
    // removals with changes (gophertunnel `ScoreboardEntry.Marshal`).
    let packet = SetScorePacket {
        score_info: vec![
            SetScorePacketScoreInfoItem::RemoveScore(RemoveScore {
                action: "remove".to_owned(),
                scoreboard_id: ScoreboardId { scoreboard_id: 7 },
                objective_name: Some("kills".to_owned()),
            }),
            SetScorePacketScoreInfoItem::ChangeFakePlayerScore(Box::new(ChangeFakePlayerScore {
                action: "changefakeplayer".to_owned(),
                scoreboard_id: ScoreboardId { scoreboard_id: 8 },
                objective_name: "kills".to_owned(),
                score_value: 12,
                fake_player_name: "Server".to_owned(),
            })),
        ],
    };
    let UiEvent::Score(score) = ui(packet).unwrap() else {
        panic!("expected score event")
    };
    assert_eq!(score.entries.len(), 2);
    assert_eq!(score.entries[0].action, protocol::ScoreAction::Remove);
    assert_eq!(score.entries[0].scoreboard_id, 7);
    assert_eq!(score.entries[0].objective_name.as_ref(), "kills");
    // A removal carries no score or identity on the wire.
    assert_eq!(score.entries[0].score, 0);
    assert_eq!(score.entries[0].identity, protocol::ScoreIdentity::None);
    assert_eq!(score.entries[1].action, protocol::ScoreAction::Change);
    assert_eq!(score.entries[1].scoreboard_id, 8);
    assert_eq!(score.entries[1].score, 12);
    let protocol::ScoreIdentity::FakePlayer(name) = &score.entries[1].identity else {
        panic!("expected a fake-player identity")
    };
    assert_eq!(name.as_ref(), "Server");
}

#[test]
fn oversized_text_scores_and_form_json_fail_closed() {
    let text = raw_text_packet("x".repeat(MAX_UI_TEXT_BYTES + 1));
    assert_eq!(
        ui(text).unwrap_err(),
        UiPacketError::TextTooLong {
            bytes: MAX_UI_TEXT_BYTES + 1,
            max: MAX_UI_TEXT_BYTES,
        }
    );

    let scores = SetScorePacket {
        score_info: vec![SetScorePacketScoreInfoItem::default(); MAX_SCORE_ENTRIES_PER_PACKET + 1],
    };
    assert_eq!(
        ui(scores).unwrap_err(),
        UiPacketError::TooManyScores {
            count: MAX_SCORE_ENTRIES_PER_PACKET + 1,
            max: MAX_SCORE_ENTRIES_PER_PACKET,
        }
    );

    let form = ModalFormRequestPacket {
        form_id: 1,
        form_uijson: "x".repeat(MAX_FORM_JSON_BYTES + 1),
    };
    assert_eq!(
        ui(form).unwrap_err(),
        UiPacketError::FormTooLarge {
            bytes: MAX_FORM_JSON_BYTES + 1,
            max: MAX_FORM_JSON_BYTES,
        }
    );
}

#[test]
fn raw_ui_strings_reject_invalid_utf8_before_owned_materialization() {
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, McpePacketName::ModalFormRequestPacket as u32);
    wire::write_var_u32(&mut payload, 7);
    wire::write_var_u32(&mut payload, 1);
    payload.put_u8(0xff);

    let mut batch = BytesMut::new();
    batch.put_u8(0xfe);
    wire::write_var_u32(&mut batch, payload.len() as u32);
    batch.extend_from_slice(&payload);

    let error = decode_batch(batch.freeze(), &BedrockSession { shield_item_id: 0 })
        .expect_err("invalid UI UTF-8 must fail closed");
    assert!(error.to_string().contains("UTF-8"), "{error}");
}

#[test]
fn raw_score_strings_reject_invalid_utf8_before_owned_materialization() {
    // 1.26.40 SetScore wire (gophertunnel `SetScore.Marshal` plus
    // `ScoreboardEntry.Marshal`): entry count, then per entry a varuint32
    // variant, the lowercase variant name, the entry id, and the variant body.
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, McpePacketName::SetScorePacket as u32);
    wire::write_var_u32(&mut payload, 1);
    wire::write_var_u32(&mut payload, 3);
    wire::write_var_u32(&mut payload, "changefakeplayer".len() as u32);
    payload.extend_from_slice(b"changefakeplayer");
    wire::write_var_u64(&mut payload, 2);
    wire::write_var_u32(&mut payload, 1);
    payload.put_u8(0xff);
    payload.put_i32_le(0);
    wire::write_var_u32(&mut payload, 1);
    payload.put_u8(b'a');

    let mut batch = BytesMut::new();
    batch.put_u8(0xfe);
    wire::write_var_u32(&mut batch, payload.len() as u32);
    batch.extend_from_slice(&payload);

    let error = decode_batch(batch.freeze(), &BedrockSession { shield_item_id: 0 })
        .expect_err("invalid score UTF-8 must fail closed");
    assert!(error.to_string().contains("UTF-8"), "{error}");
}

#[test]
fn raw_text_parameter_count_is_bounded_before_parameter_allocation() {
    // Text wire is unchanged in 1.26.40: NeedsTranslation, the category byte,
    // the message type, then the payload for that category.
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, McpePacketName::TextPacket as u32);
    payload.put_u8(0);
    payload.put_u8(2);
    payload.put_u8(2);
    wire::write_var_u32(&mut payload, 1);
    payload.put_u8(b'x');
    wire::write_var_u32(&mut payload, (protocol::MAX_CHAT_PARAMETERS + 1) as u32);

    let mut batch = BytesMut::new();
    batch.put_u8(0xfe);
    wire::write_var_u32(&mut batch, payload.len() as u32);
    batch.extend_from_slice(&payload);

    let error = decode_batch(batch.freeze(), &BedrockSession { shield_item_id: 0 })
        .expect_err("oversized text parameter count must fail before allocation");
    assert!(error.to_string().contains("parameters"), "{error}");
}

#[test]
fn raw_soft_enum_count_is_bounded_before_suggestion_allocation() {
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, McpePacketName::UpdateSoftEnumPacket as u32);
    wire::write_var_u32(&mut payload, 8);
    payload.extend_from_slice(b"commands");
    wire::write_var_u32(&mut payload, (MAX_CHAT_AUTOCOMPLETE + 1) as u32);

    let mut batch = BytesMut::new();
    batch.put_u8(0xfe);
    wire::write_var_u32(&mut batch, payload.len() as u32);
    batch.extend_from_slice(&payload);

    let error = decode_batch(batch.freeze(), &BedrockSession { shield_item_id: 0 })
        .expect_err("oversized soft enum count must fail before allocation");
    assert!(error.to_string().contains("suggestions"), "{error}");
}

#[test]
fn block_crack_events_preserve_server_progress_rate_without_inventing_stage_or_actor() {
    let start = LevelEventPacket {
        event_id: LEVEL_EVENT_START_BLOCK_CRACKING,
        position: Vec3 {
            x: 1.0,
            y: 64.0,
            z: -2.0,
        },
        data: 6_553,
    };
    let Some(WorldEvent::BlockCrack(start)) = into_world_event(start.into(), 0).unwrap() else {
        panic!("expected block crack start")
    };
    assert_eq!(start.position, [1, 64, -2]);
    assert_eq!(
        start.action,
        protocol::BlockCrackAction::Start {
            progress_per_tick: 6_553
        }
    );

    let fractional = LevelEventPacket {
        event_id: LEVEL_EVENT_UPDATE_BLOCK_CRACKING,
        position: Vec3 {
            x: 1.5,
            y: 64.0,
            z: -2.0,
        },
        data: 1,
    };
    assert!(matches!(
        into_world_event(fractional.into(), 0),
        Err(protocol::WorldPacketError::Ui(
            UiPacketError::InvalidBlockCrackPosition { field: "x", .. }
        ))
    ));

    let overflowing = LevelEventPacket {
        event_id: LEVEL_EVENT_START_BLOCK_CRACKING,
        position: Vec3 {
            x: 2_147_483_648.0,
            y: 64.0,
            z: -2.0,
        },
        data: 1,
    };
    assert!(matches!(
        into_world_event(overflowing.into(), 0),
        Err(protocol::WorldPacketError::Ui(
            UiPacketError::InvalidBlockCrackPosition { field: "x", .. }
        ))
    ));
}

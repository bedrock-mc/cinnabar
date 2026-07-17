use bytes::{BufMut, BytesMut};
use protocol::{
    BedrockSession, BossAction, BossColor, ChatAutocompleteAction, MAX_CHAT_AUTOCOMPLETE,
    MAX_FORM_JSON_BYTES, MAX_SCORE_ENTRIES_PER_PACKET, MAX_UI_TEXT_BYTES, UiEvent, UiPacketError,
    WorldEvent, decode_batch, into_world_event,
};
use valentine::bedrock::version::v1_26_30::{
    BossEventPacket, BossEventPacketColor, BossEventPacketOverlay, BossEventPacketType,
    LevelEventPacket, LevelEventPacketEvent, McpePacketName, ModalFormRequestPacket,
    PlayStatusPacket, PlayStatusPacketStatus, SetHealthPacket, SetScorePacket,
    SetScorePacketAction, SetScorePacketEntriesItem, SetTitlePacket, SetTitlePacketType,
    TextPacket, TextPacketCategory, TextPacketContent, TextPacketContentJson, TextPacketType,
    UpdateSoftEnumPacket, UpdateSoftEnumPacketActionType, Vec3F,
};
use valentine::protocol::wire;

const TEXT_FIXTURE: &[u8] = include_bytes!("../fixtures/text.bin");
const TITLE_FIXTURE: &[u8] = include_bytes!("../fixtures/set_title.bin");
const BOSS_FIXTURE: &[u8] = include_bytes!("../fixtures/boss_event.bin");
const FORM_FIXTURE: &[u8] = include_bytes!("../fixtures/modal_form_request.bin");

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
    let text = TextPacket {
        category: TextPacketCategory::MessageOnly,
        type_: TextPacketType::Raw,
        content: Some(TextPacketContent::Raw(TextPacketContentJson {
            message: "§ahello".to_owned(),
        })),
        ..Default::default()
    };
    let title = SetTitlePacket {
        type_: SetTitlePacketType::SetTitle,
        text: "Round one".to_owned(),
        fade_in_time: 5,
        stay_time: 40,
        fade_out_time: 10,
        ..Default::default()
    };
    let boss = BossEventPacket {
        target_entity_id: 17,
        type_: BossEventPacketType::ShowBar,
        title: "Dragon".to_owned(),
        progress: 0.75,
        color: BossEventPacketColor::RebeccaPurple,
        overlay: BossEventPacketOverlay::Notched10,
        ..Default::default()
    };
    let form = ModalFormRequestPacket {
        form_id: 91,
        data: r#"{"type":"form","title":"Pick"}"#.to_owned(),
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
            status: PlayStatusPacketStatus::PlayerSpawn,
        })
        .unwrap(),
        UiEvent::Hud(protocol::HudEvent::PlayerStatus(
            protocol::PlayerStatus::PlayerSpawn
        ))
    ));
    let autocomplete = UpdateSoftEnumPacket {
        enum_type: "commands".to_owned(),
        options: vec!["give".to_owned(), "gamerule".to_owned()],
        action_type: UpdateSoftEnumPacketActionType::Update,
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
fn oversized_text_scores_and_form_json_fail_closed() {
    let text = TextPacket {
        category: TextPacketCategory::MessageOnly,
        type_: TextPacketType::Raw,
        content: Some(TextPacketContent::Raw(TextPacketContentJson {
            message: "x".repeat(MAX_UI_TEXT_BYTES + 1),
        })),
        ..Default::default()
    };
    assert_eq!(
        ui(text).unwrap_err(),
        UiPacketError::TextTooLong {
            bytes: MAX_UI_TEXT_BYTES + 1,
            max: MAX_UI_TEXT_BYTES,
        }
    );

    let scores = SetScorePacket {
        action: SetScorePacketAction::Remove,
        entries: vec![SetScorePacketEntriesItem::default(); MAX_SCORE_ENTRIES_PER_PACKET + 1],
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
        data: "x".repeat(MAX_FORM_JSON_BYTES + 1),
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
    wire::write_var_u32(&mut payload, McpePacketName::PacketModalFormRequest as u32);
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
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, McpePacketName::PacketSetScore as u32);
    payload.put_u8(0);
    wire::write_var_u32(&mut payload, 1);
    wire::write_var_u64(&mut payload, 2);
    wire::write_var_u32(&mut payload, 1);
    payload.put_u8(0xff);
    payload.put_i32_le(0);
    payload.put_i8(3);
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
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, McpePacketName::PacketText as u32);
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
    wire::write_var_u32(&mut payload, McpePacketName::PacketUpdateSoftEnum as u32);
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
        event: LevelEventPacketEvent::BlockStartBreak,
        position: Vec3F {
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
        event: LevelEventPacketEvent::BlockBreakSpeed,
        position: Vec3F {
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
        event: LevelEventPacketEvent::BlockStartBreak,
        position: Vec3F {
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

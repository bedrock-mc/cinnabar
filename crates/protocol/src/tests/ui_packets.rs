use std::sync::Arc;

use bedrock_protocol_1_26_51::enums::{
    BossBarColor, BossBarOverlay, BossEventUpdateType, ModalFormCancelReason, PlayStatusType,
    SoftEnumUpdateType, TextPacketType, TitleType,
};
use bedrock_protocol_1_26_51::packets::{
    self, BossEvent as BossEventPacket, CommandOutput, LevelEvent, ModalFormRequest,
    ModalFormResponse, PacketId, PlayStatus, SetHealth, SetScore, SetTitle, Text, ToastRequest,
    UpdateSoftEnum,
};
use bedrock_protocol_1_26_51::types::{
    ActorUniqueID, CommandOutputData, CommandOutputMessage, ScoreboardId, SetScoreInfoItem,
    TextData,
};
use bedrock_protocol_1_26_51::wire::{self, Decode, Encode};
use bytes::{BufMut, BytesMut};
use protocol::{
    BossAction, BossColor, ChatAutocompleteAction, CustomFormValue, FormButtonImage, FormKind,
    MAX_CHAT_AUTOCOMPLETE, MAX_FORM_JSON_BYTES, MAX_SCORE_ENTRIES_PER_PACKET, MAX_UI_TEXT_BYTES,
    ModalFormResponseSelection, ProtocolError, UiEvent, UiPacketError, WorldEvent,
    custom_form_submit_response, decode_batch, into_world_event, modal_form_cancel_response,
    modal_form_submit_response,
};

const TEXT_FIXTURE: &[u8] = include_bytes!("../../fixtures/text.bin");
const TITLE_FIXTURE: &[u8] = include_bytes!("../../fixtures/set_title.bin");
const BOSS_FIXTURE: &[u8] = include_bytes!("../../fixtures/boss_event.bin");
const FORM_FIXTURE: &[u8] = include_bytes!("../../fixtures/modal_form_request.bin");

#[test]
fn boss_remove_ignores_unused_invalid_presentation_fields() {
    for packet in [
        BossEventPacket {
            health_percent: wire::F32LE(f32::NAN),
            ..Default::default()
        },
        BossEventPacket {
            color: BossBarColor::Unknown(u8::MAX),
            overlay: BossBarOverlay::Unknown(u8::MAX),
            ..Default::default()
        },
        BossEventPacket {
            name: "x".repeat(MAX_UI_TEXT_BYTES + 1).into(),
            ..Default::default()
        },
    ] {
        let UiEvent::Boss(event) = ui(BossEventPacket {
            target_actor_id: ActorUniqueID(-17),
            event_type: BossEventUpdateType::Remove,
            ..packet
        })
        .expect("removal only consumes the boss identity") else {
            panic!("expected boss event");
        };
        assert_eq!(event.action, BossAction::Hide);
        assert_eq!(event.target_entity_id, -17);
    }
}

/// gophertunnel's `LevelEventStartBlockCracking` / `LevelEventStopBlockCracking`
/// / `LevelEventUpdateBlockCracking` (`minecraft/protocol/packet/level_event.go`).
/// 1.26.40 carries LevelEvent as a raw `event_id` rather than a named enum.
const LEVEL_EVENT_START_BLOCK_CRACKING: i32 =
    protocol_ids::LevelEvent::StartBlockCracking.wire_value();
const LEVEL_EVENT_UPDATE_BLOCK_CRACKING: i32 =
    protocol_ids::LevelEvent::UpdateBlockCracking.wire_value();

fn put_var_u64(out: &mut BytesMut, mut value: u64) {
    while value >= 0x80 {
        out.put_u8(value as u8 | 0x80);
        value >>= 7;
    }
    out.put_u8(value as u8);
}

fn put_var_u32(out: &mut BytesMut, value: u32) {
    put_var_u64(out, value.into());
}

fn ui<P>(packet: P) -> Result<UiEvent, UiPacketError>
where
    packets::Packet: From<P>,
{
    match into_world_event(protocol::Packet::new(packet), 0) {
        Ok(Some(WorldEvent::Ui(event))) => Ok(event),
        Ok(other) => panic!("expected UI event, got {other:?}"),
        Err(protocol::WorldPacketError::Ui(error)) => Err(error),
        Err(other) => panic!("unexpected world packet error: {other}"),
    }
}

fn decode_ui_fixture(bytes: &'static [u8]) -> UiEvent {
    let mut packets = decode_batch(bytes.into()).expect("decode pinned UI fixture");
    assert_eq!(packets.len(), 1);
    match into_world_event(packets.pop().unwrap(), 0).expect("normalize pinned UI fixture") {
        Some(WorldEvent::Ui(event)) => event,
        other => panic!("expected one UI event, got {other:?}"),
    }
}

fn raw_text_packet(message: String) -> Text {
    Text {
        body: TextData::MessageOnly {
            message_type: TextPacketType::Raw,
            message: message.into(),
        },
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
    let UiEvent::Form(form) = decode_ui_fixture(FORM_FIXTURE) else {
        panic!("expected form event")
    };
    assert_eq!(form.form_id, 91);
    assert_eq!(form.kind, FormKind::Menu);
    assert_eq!(form.title.as_deref(), Some("Fixture"));
    assert_eq!(form.json.as_ref(), r#"{"type":"form","title":"Fixture"}"#);
}

#[test]
fn representative_ui_packets_normalize_without_vendor_types() {
    let text = raw_text_packet("§ahello".to_owned());
    let title = SetTitle {
        title_type: TitleType::Title,
        title_text: "Round one".into(),
        fade_in_time: wire::ZigZag32(5),
        stay_time: wire::ZigZag32(40),
        fade_out_time: wire::ZigZag32(10),
        ..Default::default()
    };
    // `Add` is wire value 0, gophertunnel's `BossEventShow`.
    let boss = BossEventPacket {
        target_actor_id: ActorUniqueID(17),
        event_type: BossEventUpdateType::Add,
        name: "Dragon".into(),
        health_percent: wire::F32LE(0.75),
        color: BossBarColor::RebeccaPurple,
        overlay: BossBarOverlay::Notched10,
        ..Default::default()
    };
    let form = ModalFormRequest {
        form_id: wire::VarUInt(91),
        form_ui_json: r#"{"type":"form","title":"Pick"}"#.into(),
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
    let UiEvent::Form(form) = ui(form).unwrap() else {
        panic!("expected form event")
    };
    assert_eq!(form.form_id, 91);
    assert_eq!(form.kind, FormKind::Menu);
    assert_eq!(form.title.as_deref(), Some("Pick"));
    assert!(matches!(
        ui(SetHealth {
            health: wire::ZigZag32(19)
        })
        .unwrap(),
        UiEvent::Hud(protocol::HudEvent::Health { health: 19 })
    ));
    assert!(matches!(
        ui(PlayStatus {
            status: PlayStatusType::Playerspawn,
        })
        .unwrap(),
        UiEvent::Hud(protocol::HudEvent::PlayerStatus(
            protocol::PlayerStatus::PlayerSpawn
        ))
    ));
    let toast = ToastRequest {
        title: "Saved".into(),
        content: "world backed up".into(),
    };
    let UiEvent::Hud(protocol::HudEvent::Toast { title, message }) = ui(toast).unwrap() else {
        panic!("expected toast event")
    };
    assert_eq!(title.as_ref(), "Saved");
    assert_eq!(message.as_ref(), "world backed up");
    // `Replace` is wire value 2, gophertunnel's `SoftEnumActionSet`.
    let autocomplete = UpdateSoftEnum {
        enum_name: "commands".into(),
        values: vec!["give".into(), "gamerule".into()],
        update_type: SoftEnumUpdateType::Replace,
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
    let packet = CommandOutput {
        output: CommandOutputData {
            output_type: "all_output".into(),
            success_count: wire::U32LE(1),
            output_messages: vec![CommandOutputMessage {
                message_id: "commands.generic.success".into(),
                successful: true,
                parameters: vec!["sm3".into()],
            }],
            data_set: Some("transfer accepted".into()),
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
    // The add/remove verb is carried per entry, so one packet may mix removals
    // with changes (gophertunnel `ScoreboardEntry.Marshal`).
    let packet = SetScore {
        score_info: vec![
            SetScoreInfoItem::RemoveScore {
                action: "remove".into(),
                scoreboard_id: ScoreboardId(7),
                objective_name: Some("kills".into()),
            },
            SetScoreInfoItem::ChangeFakePlayerScore {
                action: "changefakeplayer".into(),
                scoreboard_id: ScoreboardId(8),
                objective_name: "kills".into(),
                score_value: wire::I32LE(12),
                fake_player_name: "Server".into(),
            },
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
fn remove_score_writes_one_optional_objective_marker() {
    // Each case leads with the RemoveScore union selector 0.
    let cases = [
        (None, vec![0, 6, b'r', b'e', b'm', b'o', b'v', b'e', 14, 0]),
        (
            Some("obj".into()),
            vec![
                0, 6, b'r', b'e', b'm', b'o', b'v', b'e', 14, 1, 3, b'o', b'b', b'j',
            ],
        ),
    ];

    for (objective_name, expected) in cases {
        let value = SetScoreInfoItem::RemoveScore {
            action: "remove".into(),
            scoreboard_id: ScoreboardId(7),
            objective_name,
        };
        let encoded = value.encode_to_vec().expect("encode RemoveScore");
        assert_eq!(encoded, expected);

        let decoded = SetScoreInfoItem::decode_exact(&expected).expect("decode RemoveScore");
        assert_eq!(decoded, value);
    }
}

// gophertunnel `ScoreboardEntry.Marshal` writes one presence byte before a
// removal's objective name; decoding must walk the same shape or the next entry misaligns.
#[test]
fn named_removal_is_followed_by_the_next_entry() {
    fn text(out: &mut Vec<u8>, value: &str) {
        out.push(value.len() as u8);
        out.extend_from_slice(value.as_bytes());
    }
    let mut payload = vec![2];
    payload.push(0);
    text(&mut payload, "remove");
    payload.extend([14, 1]);
    text(&mut payload, "kills");
    payload.push(3);
    text(&mut payload, "changefakeplayer");
    payload.push(16);
    text(&mut payload, "kills");
    payload.extend(12_i32.to_le_bytes());
    text(&mut payload, "Server");
    let mut batch = vec![0xfe, payload.len() as u8 + 1, PacketId::SetScore as u8];
    batch.extend(payload);

    let mut packets = decode_batch(batch.into()).expect("SetScore enters the play receive path");
    let Ok(Some(WorldEvent::Ui(UiEvent::Score(score)))) = into_world_event(packets.remove(0), 0)
    else {
        panic!("expected a score event")
    };
    assert_eq!(score.entries.len(), 2);
    assert_eq!(score.entries[0].objective_name.as_ref(), "kills");
    assert_eq!(score.entries[1].scoreboard_id, 8);
}

// Native clients clear an objective-less removal's entry from every objective,
// so an absent name must stay distinguishable from a present empty one.
#[test]
fn absent_removal_objective_differs_from_an_empty_name() {
    let removal = |objective_name| {
        let packet = SetScore {
            score_info: vec![SetScoreInfoItem::RemoveScore {
                action: "remove".into(),
                scoreboard_id: ScoreboardId(7),
                objective_name,
            }],
        };
        let UiEvent::Score(score) = ui(packet).unwrap() else {
            panic!("expected score event")
        };
        score.entries[0].clone()
    };
    let absent = removal(None);
    assert_ne!(removal(Some(wire::Str::default())), absent);
}

#[test]
fn oversized_text_scores_and_form_json_fail_closed() {
    let text = raw_text_packet("x".repeat(crate::ui::MAX_TEXT_MESSAGE_BYTES + 1));
    assert_eq!(
        ui(text).unwrap_err(),
        UiPacketError::TextTooLong {
            bytes: crate::ui::MAX_TEXT_MESSAGE_BYTES + 1,
            max: crate::ui::MAX_TEXT_MESSAGE_BYTES,
        }
    );

    let scores = SetScore {
        score_info: vec![SetScoreInfoItem::default(); MAX_SCORE_ENTRIES_PER_PACKET + 1],
    };
    assert_eq!(
        ui(scores).unwrap_err(),
        UiPacketError::TooManyScores {
            count: MAX_SCORE_ENTRIES_PER_PACKET + 1,
            max: MAX_SCORE_ENTRIES_PER_PACKET,
        }
    );

    let form = ModalFormRequest {
        form_id: wire::VarUInt(1),
        form_ui_json: "x".repeat(MAX_FORM_JSON_BYTES + 1).into(),
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
fn decoded_ui_strings_reject_invalid_utf8() {
    let mut payload = BytesMut::new();
    put_var_u32(&mut payload, PacketId::ModalFormRequest as u32);
    put_var_u32(&mut payload, 7);
    put_var_u32(&mut payload, 1);
    payload.put_u8(0xff);

    let mut batch = BytesMut::new();
    batch.put_u8(0xfe);
    put_var_u32(&mut batch, payload.len() as u32);
    batch.extend_from_slice(&payload);

    let error = decode_batch(batch.freeze()).expect_err("invalid UI UTF-8 must fail closed");
    assert!(error.to_string().contains("UTF-8"), "{error}");
}

#[test]
fn decoded_score_strings_reject_invalid_utf8() {
    // Protocol 2168 SetScore wire (gophertunnel `SetScore.Marshal` plus
    // `ScoreboardEntry.Marshal`): entry count, then per entry a varuint32
    // variant, the lowercase variant name, the entry id, and the variant body.
    let mut payload = BytesMut::new();
    put_var_u32(&mut payload, PacketId::SetScore as u32);
    put_var_u32(&mut payload, 1);
    put_var_u32(&mut payload, 3);
    put_var_u32(&mut payload, "changefakeplayer".len() as u32);
    payload.extend_from_slice(b"changefakeplayer");
    put_var_u64(&mut payload, 2);
    put_var_u32(&mut payload, 1);
    payload.put_u8(0xff);
    payload.put_i32_le(0);
    put_var_u32(&mut payload, 1);
    payload.put_u8(b'a');

    let mut batch = BytesMut::new();
    batch.put_u8(0xfe);
    put_var_u32(&mut batch, payload.len() as u32);
    batch.extend_from_slice(&payload);

    let error = decode_batch(batch.freeze()).expect_err("invalid score UTF-8 must fail closed");
    assert!(error.to_string().contains("UTF-8"), "{error}");
}

#[test]
fn raw_text_parameter_count_is_bounded_before_parameter_allocation() {
    // Text wire is unchanged in 1.26.40: NeedsTranslation, the category byte,
    // the message type, then the payload for that category.
    let mut payload = BytesMut::new();
    put_var_u32(&mut payload, PacketId::Text as u32);
    payload.put_u8(0);
    payload.put_u8(2);
    payload.put_u8(2);
    put_var_u32(&mut payload, 1);
    payload.put_u8(b'x');
    put_var_u32(&mut payload, (protocol::MAX_CHAT_PARAMETERS + 1) as u32);
    // Empty parameters, so the declared count is held by the input rather than truncated.
    payload.put_bytes(0, protocol::MAX_CHAT_PARAMETERS + 1);

    let mut batch = BytesMut::new();
    batch.put_u8(0xfe);
    put_var_u32(&mut batch, payload.len() as u32);
    batch.extend_from_slice(&payload);

    // The Text schema caps parameters below MAX_CHAT_PARAMETERS, so the count is a
    // skippable schema rejection before any parameter is allocated.
    let error = decode_batch(batch.freeze())
        .expect_err("oversized text parameter count must fail before allocation");
    assert!(
        matches!(
            error,
            ProtocolError::Ui(UiPacketError::SchemaViolation { .. })
        ),
        "{error}"
    );
}

#[test]
fn raw_soft_enum_count_is_bounded_before_suggestion_allocation() {
    let mut payload = BytesMut::new();
    put_var_u32(&mut payload, PacketId::UpdateSoftEnum as u32);
    put_var_u32(&mut payload, 8);
    payload.extend_from_slice(b"commands");
    put_var_u32(&mut payload, (MAX_CHAT_AUTOCOMPLETE + 1) as u32);
    // Empty suggestions, so the declared count is held by the input rather than truncated.
    payload.put_bytes(0, MAX_CHAT_AUTOCOMPLETE + 1);

    let mut batch = BytesMut::new();
    batch.put_u8(0xfe);
    put_var_u32(&mut batch, payload.len() as u32);
    batch.extend_from_slice(&payload);

    let error = decode_batch(batch.freeze())
        .expect_err("oversized soft enum count must fail before allocation");
    assert!(error.to_string().contains("suggestions"), "{error}");
}

#[test]
fn block_crack_events_preserve_server_progress_rate_without_inventing_stage_or_actor() {
    let start = LevelEvent {
        event_id: wire::ZigZag32(LEVEL_EVENT_START_BLOCK_CRACKING),
        position: [1.0, 64.0, -2.0].into(),
        data: wire::ZigZag32(6_553),
    };
    let Some(WorldEvent::BlockCrack(start)) =
        into_world_event(protocol::Packet::new(start), 0).unwrap()
    else {
        panic!("expected block crack start")
    };
    assert_eq!(start.position, [1, 64, -2]);
    assert_eq!(
        start.action,
        protocol::BlockCrackAction::Start {
            progress_per_tick: 6_553
        }
    );

    // Vanilla floors fractional coordinates and keeps a zero (stationary) speed.
    let fractional = LevelEvent {
        event_id: wire::ZigZag32(LEVEL_EVENT_UPDATE_BLOCK_CRACKING),
        position: [1.5, 64.0, -2.5].into(),
        data: wire::ZigZag32(0),
    };
    let Some(WorldEvent::BlockCrack(fractional)) =
        into_world_event(protocol::Packet::new(fractional), 0).unwrap()
    else {
        panic!("expected block crack update")
    };
    assert_eq!(fractional.position, [1, 64, -3]);
    assert_eq!(
        fractional.action,
        protocol::BlockCrackAction::UpdateSpeed {
            progress_per_tick: 0
        }
    );
    let non_finite = LevelEvent {
        event_id: wire::ZigZag32(LEVEL_EVENT_UPDATE_BLOCK_CRACKING),
        position: [f32::NAN, 64.0, -2.0].into(),
        data: wire::ZigZag32(1),
    };
    assert!(matches!(
        into_world_event(protocol::Packet::new(non_finite), 0),
        Err(protocol::WorldPacketError::Ui(
            UiPacketError::InvalidBlockCrackPosition { field: "x", .. }
        ))
    ));

    let overflowing = LevelEvent {
        event_id: wire::ZigZag32(LEVEL_EVENT_START_BLOCK_CRACKING),
        position: [2_147_483_648.0, 64.0, -2.0].into(),
        data: wire::ZigZag32(1),
    };
    assert!(matches!(
        into_world_event(protocol::Packet::new(overflowing), 0),
        Err(protocol::WorldPacketError::Ui(
            UiPacketError::InvalidBlockCrackPosition { field: "x", .. }
        ))
    ));
}

/// Crack speeds preserve the signed wire value, including stationary and fast cracks.
#[test]
fn block_crack_events_preserve_the_full_wire_speed() {
    for data in [i32::MIN, -1, 0, 65535, 65536, i32::MAX] {
        let packet = LevelEvent {
            event_id: wire::ZigZag32(LEVEL_EVENT_UPDATE_BLOCK_CRACKING),
            position: [1.0, 64.0, -2.0].into(),
            data: wire::ZigZag32(data),
        };
        let Some(WorldEvent::BlockCrack(event)) =
            into_world_event(protocol::Packet::new(packet), 0).unwrap()
        else {
            panic!("expected block crack update")
        };
        assert_eq!(
            event.action,
            protocol::BlockCrackAction::UpdateSpeed {
                progress_per_tick: data
            }
        );
    }
}

fn form_event(json: &str) -> Result<protocol::FormRequestEvent, UiPacketError> {
    let packet = ModalFormRequest {
        form_id: wire::VarUInt(3),
        form_ui_json: json.into(),
    };
    match ui(packet) {
        Ok(UiEvent::Form(form)) => Ok(form),
        Ok(other) => panic!("expected form event, got {other:?}"),
        Err(error) => Err(error),
    }
}

#[test]
fn text_button_form_model_preserves_unicode_and_wire_indices() {
    let event = form_event(r#"{"type":"form","title":"Choose 世界","content":"Pick one\nα β","buttons":[{"text":"First ✓"},{"text":"第二"}]}"#).unwrap();
    let protocol::ServerFormModel::TextMenu(menu) = event.model else {
        panic!("expected supported text-only menu")
    };
    assert_eq!(menu.title.as_ref(), "Choose 世界");
    assert_eq!(menu.content.as_ref(), "Pick one\nα β");
    assert_eq!(menu.buttons.len(), 2);
    assert_eq!(menu.buttons[1].as_ref(), "第二");
}

#[test]
fn element_button_form_model_preserves_unicode_and_wire_indices() {
    let event = form_event(r#"{"type":"form","title":"Menu 世界","content":"Select α β","elements":[{"type":"button","text":"First ✓","image":null},{"type":"button","text":"第二","image":null}]}"#).unwrap();
    let protocol::ServerFormModel::TextMenu(menu) = event.model else {
        panic!("element buttons must normalize as a supported text menu")
    };
    assert_eq!(menu.title.as_ref(), "Menu 世界");
    assert_eq!(menu.content.as_ref(), "Select α β");
    assert_eq!(
        menu.buttons
            .iter()
            .map(|label| label.as_ref())
            .collect::<Vec<_>>(),
        ["First ✓", "第二"]
    );
    assert_eq!(menu.omitted_images, 0);
}

#[test]
fn element_button_forms_reject_ambiguous_or_unsupported_controls_without_renumbering() {
    for json in [
        r#"{"type":"form","elements":null}"#,
        r#"{"type":"form","elements":{}}"#,
        r#"{"type":"form","elements":["button"]}"#,
        r#"{"type":"form","elements":[{"text":"A","image":null}]}"#,
        r#"{"type":"form","elements":[{"type":7,"text":"A","image":null}]}"#,
    ] {
        assert_eq!(
            form_event(json).unwrap().model,
            protocol::ServerFormModel::Unsupported(protocol::UnsupportedForm::Controls)
        );
    }
}

#[test]
fn element_button_forms_keep_long_display_strings() {
    let long = "x".repeat(MAX_UI_TEXT_BYTES + 1);
    let json = format!(
        r#"{{"type":"form","elements":[{{"type":"button","text":"{long}","image":null}}]}}"#
    );
    let protocol::ServerFormModel::TextMenu(menu) = form_event(&json).unwrap().model else {
        panic!("long element button text");
    };
    assert_eq!(menu.buttons[0].as_ref(), long);
}

#[test]
fn pinned_text_menu_and_response_fixtures_match_exact_wire_payloads() {
    let UiEvent::Form(form) =
        decode_ui_fixture(include_bytes!("../../fixtures/modal_form_text_menu.bin"))
    else {
        panic!("expected form")
    };
    let protocol::ServerFormModel::TextMenu(menu) = form.model else {
        panic!("expected text-only menu")
    };
    assert_eq!(form.form_id, 92);
    assert_eq!(menu.buttons[1].as_ref(), "第二");
    for (fixture, mut expected) in [
        (
            include_bytes!("../../fixtures/modal_form_response_button.bin").as_slice(),
            modal_form_submit_response(92, ModalFormResponseSelection::ButtonIndex(1)),
        ),
        (
            include_bytes!("../../fixtures/modal_form_response_closed.bin").as_slice(),
            modal_form_cancel_response(92),
        ),
        (
            include_bytes!("../../fixtures/modal_form_response_busy.bin").as_slice(),
            protocol::modal_form_busy_response(92),
        ),
    ] {
        expected.header.from_subclient = 1;
        expected.header.to_subclient = 2;
        assert_eq!(protocol::encode(&expected).unwrap().as_ref(), fixture);
        let decoded = decode_batch(fixture.to_vec().into()).unwrap();
        assert_eq!(decoded, vec![expected]);
    }
}

#[test]
fn unsupported_form_controls_are_nonfatal_and_never_fake_text_buttons() {
    for json in [
        r#"{"type":"modal","title":"Question"}"#,
        r#"{"type":"form","buttons":[{"text":"Icon","image":{"type":"url","data":5}}]}"#,
    ] {
        assert!(matches!(
            form_event(json).unwrap().model,
            protocol::ServerFormModel::Unsupported(_)
        ));
    }
}

#[test]
fn valid_button_images_are_retained_by_kind_without_changing_indexes() {
    let json = serde_json::json!({
        "type": "form", "content": "x".repeat(MAX_UI_TEXT_BYTES),
        "buttons": [
            {"text": "First", "image": {"type": "url", "data": "https://example.invalid/icon.png"}},
            {"text": "第二", "image": {"type": "path", "data": "textures/items/apple"}},
        ],
    })
    .to_string();
    let event = form_event(&json).unwrap();
    let protocol::ServerFormModel::TextMenu(menu) = event.model else {
        panic!("valid text buttons must remain playable")
    };
    assert_eq!(menu.content.len(), MAX_UI_TEXT_BYTES);
    assert_eq!(
        menu.buttons
            .iter()
            .map(|text| text.as_ref())
            .collect::<Vec<_>>(),
        ["First", "第二"]
    );
    // The images are retained (no longer discarded), routed by wire kind, aligned by
    // button index.
    assert_eq!(
        menu.button_images.as_ref(),
        [
            Some(FormButtonImage::Url(Arc::from(
                "https://example.invalid/icon.png"
            ))),
            Some(FormButtonImage::Path(Arc::from("textures/items/apple"))),
        ]
    );
    assert_eq!(menu.omitted_images, 2);
    let response =
        modal_form_submit_response(event.form_id, ModalFormResponseSelection::ButtonIndex(1));
    let packets::Packet::ModalFormResponse(response) = *response.data else {
        panic!("response packet")
    };
    assert_eq!(response.json_response, Some("1\n".into()));
}

#[test]
fn imageless_buttons_retain_a_none_slot_per_index() {
    let event =
        form_event(r#"{"type":"form","buttons":[{"text":"A"},{"text":"B","image":{"type":"path","data":"textures/x"}}]}"#)
            .unwrap();
    let protocol::ServerFormModel::TextMenu(menu) = event.model else {
        panic!("text menu")
    };
    assert_eq!(
        menu.button_images.as_ref(),
        [None, Some(FormButtonImage::Path(Arc::from("textures/x")))]
    );
    assert_eq!(menu.omitted_images, 1);
}

#[test]
fn modal_and_custom_form_responses_round_trip() {
    fn json_of(packet: &protocol::Packet) -> Option<&str> {
        match &*packet.data {
            packets::Packet::ModalFormResponse(response) => response
                .json_response
                .as_ref()
                .map(|json| json.to_str().expect("UTF-8 response")),
            _ => panic!("response packet"),
        }
    }

    // Modal button1/button2 answer with true/false, not an index.
    let yes = modal_form_submit_response(5, ModalFormResponseSelection::ModalButton(true));
    let no = modal_form_submit_response(5, ModalFormResponseSelection::ModalButton(false));
    assert_eq!(json_of(&yes), Some("true\n"));
    assert_eq!(json_of(&no), Some("false\n"));

    // A custom form answers with an ordered JSON array of typed values; a null holds
    // a non-input element's slot.
    let mut submit = custom_form_submit_response(
        9,
        &[
            CustomFormValue::Null,
            CustomFormValue::Toggle(true),
            CustomFormValue::Slider(3.5),
            CustomFormValue::Step(2),
            CustomFormValue::Dropdown(1),
            CustomFormValue::Input("Steve".into()),
        ],
    );
    assert_eq!(json_of(&submit), Some("[null,true,3.5,2,1,\"Steve\"]\n"));

    // The custom answer survives an encode/decode wire round trip.
    submit.header.from_subclient = 1;
    submit.header.to_subclient = 2;
    let bytes = protocol::encode(&submit).unwrap();
    let decoded = decode_batch(bytes.as_ref().to_vec().into()).unwrap();
    assert_eq!(decoded, vec![submit]);
}

#[test]
fn text_button_form_images_keep_their_own_reference_limit() {
    let text = "x".repeat(MAX_UI_TEXT_BYTES + 1);
    let json = format!(
        r#"{{"type":"form","buttons":[{{"text":"Icon","image":{{"type":"url","data":"{text}"}}}}]}}"#
    );
    assert!(matches!(
        form_event(&json).unwrap().model,
        protocol::ServerFormModel::Unsupported(_)
    ));
}

#[test]
fn server_form_families_classify_from_the_type_member() {
    let modal = form_event(r#"{"type":"modal","title":"Yes?","content":"Pick one"}"#).unwrap();
    assert_eq!(modal.kind, FormKind::Modal);
    assert_eq!(modal.title.as_deref(), Some("Yes?"));

    let custom = form_event(
        r#"{"type":"custom_form","title":"Settings","content":[{"type":"toggle","text":"On"}]}"#,
    )
    .unwrap();
    assert_eq!(custom.kind, FormKind::Custom);

    // A missing, non-string, or unrecognized type member keeps the raw text
    // without inventing a family.
    assert_eq!(
        form_event(r#"{"title":"T"}"#).unwrap().kind,
        FormKind::Unknown
    );
    assert_eq!(
        form_event(r#"{"type":7,"title":"T"}"#).unwrap().kind,
        FormKind::Unknown
    );
    assert_eq!(
        form_event(r#"{"type":"slider_form"}"#).unwrap().kind,
        FormKind::Unknown
    );

    // Titles may arrive as non-string rawtext components; the family and the
    // raw text still survive with no title metadata.
    let component_title =
        form_event(r#"{"type":"form","title":{"rawtext":[{"text":"Hi"}]}}"#).unwrap();
    assert_eq!(component_title.kind, FormKind::Menu);
    assert_eq!(component_title.title, None);
}

#[test]
fn server_form_metadata_survives_escaped_and_nested_payloads() {
    let escaped = form_event(r#"{"type":"modal","title":"Line\nQuote\"End"}"#).unwrap();
    assert_eq!(escaped.title.as_deref(), Some("Line\nQuote\"End"));

    // Members beyond the metadata are skipped without interpretation.
    let deep = r#"{"type":"custom_form","title":"C","elements":[{"a":[1,2,{"b":"x"}]}]}"#;
    let event = form_event(deep).unwrap();
    assert_eq!(event.kind, FormKind::Custom);
}

#[test]
fn oversized_or_malformed_form_json_is_a_semantic_error_not_wire_fault() {
    for (json, expected) in [
        (r#"{"type":"modal""#, UiPacketError::InvalidFormJson),
        (r#"[1,2,3]"#, UiPacketError::InvalidFormJson),
        (r#"{} trailing"#, UiPacketError::InvalidFormJson),
        (r#"{"type" "modal"}"#, UiPacketError::InvalidFormJson),
    ] {
        assert_eq!(form_event(json).unwrap_err(), expected);
    }
}

#[test]
fn form_titles_keep_long_display_strings() {
    let title = "x".repeat(MAX_UI_TEXT_BYTES + 1);
    let json = format!(r#"{{"type":"form","title":"{title}","buttons":[]}}"#);
    let event = form_event(&json).unwrap();
    assert_eq!(event.title.as_deref(), Some(title.as_str()));
    let protocol::ServerFormModel::TextMenu(menu) = event.model else {
        panic!("long title");
    };
    assert_eq!(menu.title.as_ref(), title);
}

#[test]
fn modal_form_responses_encode_exact_submit_and_cancel_markers() {
    // Submitting button 2 of form 7: batch header, length, packet id 101, then
    // id, present(1), len 2, '2' and its line break, cancel absent(0).
    let submit = modal_form_submit_response(7, ModalFormResponseSelection::ButtonIndex(2));
    assert_eq!(
        protocol::encode(&submit).unwrap().as_ref(),
        &[0xfe, 0x07, 101, 0x07, 0x01, 0x02, b'2', b'\n', 0x00]
    );
    let direct = ModalFormResponse {
        form_id: wire::VarUInt(7),
        json_response: Some("2\n".into()),
        form_cancel_reason: None,
    };
    assert_eq!(
        protocol::encode(&submit).unwrap(),
        protocol::encode(&protocol::Packet::new(direct)).unwrap()
    );

    // Dismissing form 7: id, response absent(0), cancel present(1) UserClosed(0).
    let cancel = modal_form_cancel_response(7);
    assert_eq!(
        protocol::encode(&cancel).unwrap().as_ref(),
        &[0xfe, 0x05, 101, 0x07, 0x00, 0x01, 0x00]
    );
    let direct_cancel = ModalFormResponse {
        form_id: wire::VarUInt(7),
        json_response: None,
        form_cancel_reason: Some(ModalFormCancelReason::Userclosed),
    };
    assert_eq!(
        protocol::encode(&cancel).unwrap(),
        protocol::encode(&protocol::Packet::new(direct_cancel)).unwrap()
    );

    // Both directions round-trip through the pinned codec unchanged.
    let encoded = ModalFormResponse {
        form_id: wire::VarUInt(7),
        json_response: Some("2".into()),
        form_cancel_reason: None,
    }
    .encode_to_vec()
    .unwrap();
    let mut payload = BytesMut::new();
    put_var_u32(&mut payload, PacketId::ModalFormResponse as u32);
    payload.extend_from_slice(&encoded);
    let mut frame = BytesMut::new();
    frame.put_u8(0xfe);
    put_var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    let mut packets = decode_batch(frame.freeze()).unwrap();
    assert_eq!(packets.len(), 1);
    assert!(matches!(
        *packets.pop().unwrap().data,
        packets::Packet::ModalFormResponse(ModalFormResponse {
            form_id: wire::VarUInt(7),
            json_response: Some(response),
            form_cancel_reason: None,
        }) if response == "2"
    ));
}

#[test]
fn element_menus_keep_decorations_and_count_only_buttons() {
    let event = form_event(r#"{"type":"form","title":"T","content":"C","elements":[{"type":"button","text":"A","image":null},{"type":"label","text":"B","image":null},{"type":"divider"},{"type":"header","text":"H"},{"type":"button","text":"C","image":null}]}"#).unwrap();
    let protocol::ServerFormModel::ElementMenu(menu) = event.model else {
        panic!("decorated element menu")
    };
    assert_eq!(menu.button_count(), 2);
    assert_eq!(
        menu.elements.as_ref(),
        [
            protocol::MenuElement::Button {
                text: protocol::FormText::from("A"),
                image: None,
            },
            protocol::MenuElement::Label(protocol::FormText::from("B")),
            protocol::MenuElement::Divider,
            protocol::MenuElement::Header(protocol::FormText::from("H")),
            protocol::MenuElement::Button {
                text: protocol::FormText::from("C"),
                image: None,
            },
        ]
    );
}

#[test]
fn modal_forms_keep_required_buttons_and_display_fallbacks() {
    let event = form_event(
        r#"{"type":"modal","title":"Sure?","content":"Body","button1":"Yes","button2":"No"}"#,
    )
    .unwrap();
    assert_eq!(
        event.model,
        protocol::ServerFormModel::Modal(protocol::ModalDialogForm {
            title: protocol::FormText::from("Sure?"),
            content: protocol::FormText::from("Body"),
            button1: protocol::FormText::from("Yes"),
            button2: protocol::FormText::from("No"),
        })
    );
    assert_eq!(
        form_event(r#"{"type":"modal","button1":"Yes"}"#)
            .unwrap()
            .model,
        protocol::ServerFormModel::Unsupported(protocol::UnsupportedForm::Controls)
    );
}

#[test]
fn numeric_element_button_labels_keep_their_wire_indices() {
    let event = form_event(r#"{"type":"form","elements":[{"type":"button","text":"First"},{"type":"button","text":7},{"type":"button","text":"Last"}]}"#).unwrap();
    let protocol::ServerFormModel::TextMenu(menu) = event.model else {
        panic!("numeric display fallback");
    };
    assert_eq!(
        menu.buttons.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
        ["First", "", "Last"]
    );
    let event = form_event(r#"{"type":"modal","button1":"Yes","button2":5}"#).unwrap();
    let protocol::ServerFormModel::Modal(modal) = event.model else {
        panic!("modal display fallback");
    };
    assert_eq!(modal.button2.as_ref(), "");
}

#[test]
fn custom_form_elements_start_from_vanilla_defaults() {
    use protocol::{CustomFormElement, FormNumber};
    let event = form_event(
        r#"{"type":"custom_form","title":"S","submit":"Go","content":[
        {"type":"label","text":"L"},
        {"type":"toggle","text":"T","default":true},
        {"type":"slider","text":"V","min":2,"max":1},
        {"type":"step_slider","text":"P","steps":["a","b"],"default":1},
        {"type":"dropdown","text":"D","options":["x","y"],"default":9},
        {"type":"input","text":"N","placeholder":"name"},
        {"type":"divider"}
    ]}"#,
    )
    .unwrap();
    let protocol::ServerFormModel::Custom(form) = event.model else {
        panic!("custom form")
    };
    assert_eq!(form.submit.as_deref(), Some("Go"));
    assert_eq!(form.elements.len(), 7);
    let CustomFormElement::Slider {
        min,
        max,
        step,
        default,
        ..
    } = &form.elements[2]
    else {
        panic!("slider")
    };
    // max below min is raised to min; step defaults to 1; the start is min.
    assert_eq!(
        (min.get(), max.get(), step.get(), default.get()),
        (2.0, 2.0, 1.0, 2.0)
    );
    assert!(matches!(
        form.elements[3],
        CustomFormElement::StepSlider { default: 1, .. }
    ));
    // An untouched default is retained even when it has no matching option.
    assert!(matches!(
        form.elements[4],
        CustomFormElement::Dropdown { default: 9, .. }
    ));
    assert!(matches!(
        &form.elements[5],
        CustomFormElement::Input { placeholder, default, .. }
            if placeholder.as_ref() == "name" && default.is_empty()
    ));
    assert_eq!(FormNumber::new(f64::NAN), None);
}

/// Submitted scalars retain the line terminator inside the encoded JSON string.
#[test]
fn form_response_payloads_terminate_scalar_json() {
    for (selection, expected) in [
        (ModalFormResponseSelection::ButtonIndex(2), "2\n"),
        (ModalFormResponseSelection::ModalButton(true), "true\n"),
        (ModalFormResponseSelection::ModalButton(false), "false\n"),
    ] {
        let bytes = protocol::encode(&modal_form_submit_response(7, selection)).unwrap();
        let packet = decode_batch(bytes).unwrap().pop().unwrap();
        let packets::Packet::ModalFormResponse(response) = *packet.data else {
            panic!("expected form response");
        };
        assert_eq!(response.json_response, Some(expected.into()));
        assert!(response.form_cancel_reason.is_none());
    }
}

/// A submitted array has one final newline while preserving its typed values and escaped input.
#[test]
fn form_response_payloads_terminate_custom_json() {
    let values = [
        CustomFormValue::Null,
        CustomFormValue::Toggle(false),
        CustomFormValue::Input("one\ntwo".into()),
    ];
    let bytes = protocol::encode(&custom_form_submit_response(9, &values)).unwrap();
    let packet = decode_batch(bytes).unwrap().pop().unwrap();
    let packets::Packet::ModalFormResponse(response) = *packet.data else {
        panic!("expected form response");
    };
    assert_eq!(
        response.json_response,
        Some("[null,false,\"one\\ntwo\"]\n".into())
    );
    assert!(response.form_cancel_reason.is_none());
}

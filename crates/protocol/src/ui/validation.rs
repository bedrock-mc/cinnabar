//! UI admission for decoded server packets: per-field budgets bound the decoder before it
//! allocates, and decoded packets are checked for UTF-8, enum and aggregate budgets.

use bedrock_protocol_1_26_51::enums::{BossEventUpdateType, SoftEnumUpdateType, TextPacketType};
use bedrock_protocol_1_26_51::fields;
use bedrock_protocol_1_26_51::packets;
use bedrock_protocol_1_26_51::types::{SetScoreInfoItem, TextData};
use bedrock_protocol_1_26_51::wire;

use super::{
    MAX_CHAT_AUTOCOMPLETE, MAX_CHAT_AUTOCOMPLETE_BYTES, MAX_CHAT_PARAMETERS, MAX_FORM_JSON_BYTES,
    MAX_SCORE_ENTRIES_PER_PACKET, MAX_UI_TEXT_BYTES, UiPacketError,
};

/// Which UI budget a field site carries, and so which rejection breaking it reports.
#[derive(Clone, Copy)]
enum UiBudget {
    Text,
    ChatParameters,
    Scores,
    Autocomplete,
    FormJson,
}

impl UiBudget {
    const fn limit(self) -> usize {
        match self {
            Self::Text => MAX_UI_TEXT_BYTES,
            Self::ChatParameters => MAX_CHAT_PARAMETERS,
            Self::Scores => MAX_SCORE_ENTRIES_PER_PACKET,
            Self::Autocomplete => MAX_CHAT_AUTOCOMPLETE,
            Self::FormJson => MAX_FORM_JSON_BYTES,
        }
    }

    fn rejection(self, actual: usize) -> UiPacketError {
        let max = self.limit();
        match self {
            Self::Text => UiPacketError::TextTooLong { bytes: actual, max },
            Self::ChatParameters => UiPacketError::TooManyChatParameters { count: actual, max },
            Self::Scores => UiPacketError::TooManyScores { count: actual, max },
            Self::Autocomplete => {
                UiPacketError::TooManyAutocompleteSuggestions { count: actual, max }
            }
            Self::FormJson => UiPacketError::FormTooLarge { bytes: actual, max },
        }
    }
}

/// The server UI fields the client bounds while decoding; fields whose schema bound is already
/// tighter carry no budget here.
const UI_FIELD_BUDGETS: &[(wire::FieldId, UiBudget)] = &[
    (fields::TEXT_DATA_MESSAGE_ONLY_MESSAGE, UiBudget::Text),
    (fields::TEXT_DATA_AUTHOR_AND_MESSAGE_MESSAGE, UiBudget::Text),
    (fields::TEXT_DATA_MESSAGE_AND_PARAMS_MESSAGE, UiBudget::Text),
    (
        fields::TEXT_DATA_MESSAGE_AND_PARAMS_PARAMETER_LIST_ITEM,
        UiBudget::Text,
    ),
    (fields::TEXT_FILTERED_MESSAGE, UiBudget::Text),
    (fields::DEATH_INFO_DEATH_CAUSE_ATTACK_NAME, UiBudget::Text),
    (
        fields::DEATH_INFO_DEATH_CAUSE_MESSAGE_LIST,
        UiBudget::ChatParameters,
    ),
    (
        fields::DEATH_INFO_DEATH_CAUSE_MESSAGE_LIST_ITEM,
        UiBudget::Text,
    ),
    (fields::UPDATE_SOFT_ENUM_ENUM_NAME, UiBudget::Text),
    (fields::UPDATE_SOFT_ENUM_VALUES, UiBudget::Autocomplete),
    (fields::UPDATE_SOFT_ENUM_VALUES_ITEM, UiBudget::Text),
    (fields::SET_SCORE_SCORE_INFO, UiBudget::Scores),
    (
        fields::SET_SCORE_INFO_ITEM_REMOVE_SCORE_ACTION,
        UiBudget::Text,
    ),
    (
        fields::SET_SCORE_INFO_ITEM_REMOVE_SCORE_OBJECTIVE_NAME,
        UiBudget::Text,
    ),
    (
        fields::SET_SCORE_INFO_ITEM_CHANGE_PLAYER_SCORE_ACTION,
        UiBudget::Text,
    ),
    (
        fields::SET_SCORE_INFO_ITEM_CHANGE_PLAYER_SCORE_OBJECTIVE_NAME,
        UiBudget::Text,
    ),
    (
        fields::SET_SCORE_INFO_ITEM_CHANGE_ENTITY_SCORE_ACTION,
        UiBudget::Text,
    ),
    (
        fields::SET_SCORE_INFO_ITEM_CHANGE_ENTITY_SCORE_OBJECTIVE_NAME,
        UiBudget::Text,
    ),
    (
        fields::SET_SCORE_INFO_ITEM_CHANGE_FAKE_PLAYER_SCORE_ACTION,
        UiBudget::Text,
    ),
    (
        fields::SET_SCORE_INFO_ITEM_CHANGE_FAKE_PLAYER_SCORE_OBJECTIVE_NAME,
        UiBudget::Text,
    ),
    (
        fields::SET_SCORE_INFO_ITEM_CHANGE_FAKE_PLAYER_SCORE_FAKE_PLAYER_NAME,
        UiBudget::Text,
    ),
    (fields::SET_TITLE_TITLE_TEXT, UiBudget::Text),
    (fields::SET_TITLE_XUID, UiBudget::Text),
    (fields::SET_TITLE_PLATFORM_ONLINE_ID, UiBudget::Text),
    (fields::SET_TITLE_FILTERED_TITLE_MESSAGE, UiBudget::Text),
    (fields::TOAST_REQUEST_TITLE, UiBudget::Text),
    (fields::TOAST_REQUEST_CONTENT, UiBudget::Text),
    (fields::REMOVE_OBJECTIVE_OBJECTIVE_NAME, UiBudget::Text),
    (
        fields::SET_DISPLAY_OBJECTIVE_DISPLAY_SLOT_NAME,
        UiBudget::Text,
    ),
    (fields::SET_DISPLAY_OBJECTIVE_OBJECTIVE_NAME, UiBudget::Text),
    (
        fields::SET_DISPLAY_OBJECTIVE_OBJECTIVE_DISPLAY_NAME,
        UiBudget::Text,
    ),
    (fields::SET_DISPLAY_OBJECTIVE_CRITERIA_NAME, UiBudget::Text),
    (fields::MODAL_FORM_REQUEST_FORM_UI_JSON, UiBudget::FormJson),
];

/// Adds the UI budgets to `limits` as per-field decode limits.
pub(crate) fn with_field_limits(mut limits: wire::Limits) -> wire::Limits {
    for &(field, budget) in UI_FIELD_BUDGETS {
        limits.set_field(field, budget.limit());
    }
    limits
}

/// The skippable UI rejection a body decode failure stands for, if any: a broken UI budget, an
/// unknown union selector or a value outside its schema bounds. Other failures stay fatal.
pub(crate) fn semantic_rejection(error: &wire::DecodeError) -> Option<UiPacketError> {
    match *error {
        wire::DecodeError::LengthLimitExceeded { field, actual, .. } => UI_FIELD_BUDGETS
            .iter()
            .find(|(budgeted, _)| *budgeted == field)
            .map(|(_, budget)| budget.rejection(usize::try_from(actual).unwrap_or(usize::MAX))),
        wire::DecodeError::UnknownVariant { type_name, value } => {
            Some(UiPacketError::UnknownEnum {
                kind: type_name,
                value,
            })
        }
        wire::DecodeError::SchemaConstraint(reason) => {
            Some(UiPacketError::SchemaViolation { reason })
        }
        _ => None,
    }
}

/// Rejects decoded UI text that is not UTF-8 and UI values the client must not admit; other
/// packets pass.
pub(crate) fn validate_ui_packet(packet: &packets::Packet) -> Result<(), UiPacketError> {
    match packet {
        packets::Packet::Text(packet) => {
            match &packet.body {
                TextData::MessageOnly {
                    message_type,
                    message,
                } => {
                    known_text_type(*message_type)?;
                    utf8(message, "text.message")?;
                }
                TextData::AuthorAndMessage {
                    message_type,
                    player_name,
                    message,
                } => {
                    known_text_type(*message_type)?;
                    utf8(player_name, "text.source_name")?;
                    utf8(message, "text.message")?;
                }
                TextData::MessageAndParams {
                    message_type,
                    message,
                    parameter_list,
                } => {
                    known_text_type(*message_type)?;
                    utf8(message, "text.message")?;
                    for parameter in parameter_list {
                        utf8(parameter, "text.parameter")?;
                    }
                }
            }
            utf8(&packet.sender_xuid, "text.xuid")?;
            utf8(&packet.platform_id, "text.platform_chat_id")?;
            if let Some(filtered_message) = &packet.filtered_message {
                utf8(filtered_message, "text.filtered_message")?;
            }
            Ok(())
        }
        packets::Packet::DeathInfo(packet) => {
            utf8(&packet.death_cause_attack_name, "death_info.message")?;
            for parameter in &packet.death_cause_message_list {
                utf8(parameter, "death_info.parameter")?;
            }
            Ok(())
        }
        packets::Packet::SetScore(packet) => {
            for entry in &packet.score_info {
                let (action, objective_name) = match entry {
                    SetScoreInfoItem::RemoveScore {
                        action,
                        objective_name,
                        ..
                    } => (action, objective_name.as_ref()),
                    SetScoreInfoItem::ChangePlayerScore {
                        action,
                        objective_name,
                        ..
                    }
                    | SetScoreInfoItem::ChangeEntityScore {
                        action,
                        objective_name,
                        ..
                    } => (action, Some(objective_name)),
                    SetScoreInfoItem::ChangeFakePlayerScore {
                        action,
                        objective_name,
                        fake_player_name,
                        ..
                    } => {
                        utf8(fake_player_name, "score.custom_name")?;
                        (action, Some(objective_name))
                    }
                };
                utf8(action, "score.action")?;
                if let Some(objective_name) = objective_name {
                    utf8(objective_name, "score.objective_name")?;
                }
            }
            Ok(())
        }
        packets::Packet::ModalFormRequest(packet) => utf8(&packet.form_ui_json, "modal_form.data"),
        packets::Packet::SetTitle(packet) => {
            utf8(&packet.title_text, "set_title.text")?;
            utf8(&packet.xuid, "set_title.xuid")?;
            utf8(&packet.platform_online_id, "set_title.platform_online_id")?;
            utf8(&packet.filtered_title_message, "set_title.filtered_message")
        }
        packets::Packet::BossEvent(packet) if packet.event_type != BossEventUpdateType::Remove => {
            utf8(&packet.name, "boss.title")?;
            utf8(&packet.filtered_name, "boss.filtered_title")
        }
        packets::Packet::ToastRequest(packet) => {
            utf8(&packet.title, "toast.title")?;
            utf8(&packet.content, "toast.message")
        }
        packets::Packet::RemoveObjective(packet) => utf8(&packet.objective_name, "objective.name"),
        packets::Packet::SetDisplayObjective(packet) => {
            utf8(&packet.display_slot_name, "objective.display_slot")?;
            utf8(&packet.objective_name, "objective.name")?;
            utf8(&packet.objective_display_name, "objective.display_name")?;
            utf8(&packet.criteria_name, "objective.criteria_name")
        }
        packets::Packet::UpdateSoftEnum(packet) => validate_soft_enum(packet),
        _ => Ok(()),
    }
}

/// Checks soft-enum text and the bytes its suggestions would retain together.
fn validate_soft_enum(packet: &packets::UpdateSoftEnum) -> Result<(), UiPacketError> {
    utf8(&packet.enum_name, "soft_enum.name")?;
    let mut retained_bytes = packet.enum_name.len();
    for option in &packet.values {
        utf8(option, "soft_enum.option")?;
        retained_bytes = retained_bytes.saturating_add(option.len());
        if retained_bytes > MAX_CHAT_AUTOCOMPLETE_BYTES {
            return Err(UiPacketError::AutocompleteTooLarge {
                bytes: retained_bytes,
                max: MAX_CHAT_AUTOCOMPLETE_BYTES,
            });
        }
    }
    if let SoftEnumUpdateType::Unknown(value) = packet.update_type {
        return Err(UiPacketError::UnknownEnum {
            kind: "soft enum action",
            value: i64::from(value),
        });
    }
    Ok(())
}

/// Rejects a Text message type the protocol does not name.
fn known_text_type(message_type: TextPacketType) -> Result<(), UiPacketError> {
    match message_type {
        TextPacketType::Unknown(value) => Err(UiPacketError::UnknownEnum {
            kind: "text type",
            value: i64::from(value),
        }),
        _ => Ok(()),
    }
}

fn utf8(value: &wire::Str, field: &'static str) -> Result<(), UiPacketError> {
    value
        .to_str()
        .map(|_| ())
        .map_err(|_| UiPacketError::InvalidUtf8 { field })
}

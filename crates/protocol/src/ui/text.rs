use std::sync::Arc;

use bedrock_protocol_1_26_51::enums::{TextPacketType, TitleType};
use bedrock_protocol_1_26_51::packets::{SetTitle, Text};
use bedrock_protocol_1_26_51::types::TextData;
use bedrock_protocol_1_26_51::wire;

use super::{MAX_CHAT_PARAMETERS, MAX_TEXT_MESSAGE_BYTES, UiEvent, UiPacketError, bounded_text};

/// Which of the three Text payload shapes the packet carried.
///
/// The leading union tag of `TextPacketBody`; the message type follows inside
/// the payload. gophertunnel derives the same byte from the message type
/// (`minecraft/protocol/packet/text.go`, `Text.Marshal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCategory {
    MessageOnly,
    Authored,
    Parameters,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    Raw,
    Chat,
    Translation,
    Popup,
    JukeboxPopup,
    Tip,
    System,
    Whisper,
    Announcement,
    JsonWhisper,
    Json,
    JsonAnnouncement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEvent {
    pub category: TextCategory,
    pub kind: TextKind,
    pub needs_translation: bool,
    pub source: Option<Arc<str>>,
    pub message: Arc<str>,
    pub parameters: Arc<[Arc<str>]>,
    pub xuid: Arc<str>,
    pub platform_chat_id: Arc<str>,
    pub filtered_message: Option<Arc<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTextEvent {
    pub text: TextEvent,
    pub document: Arc<crate::RawTextDocument>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleAction {
    Clear,
    Reset,
    SetTitle,
    SetSubtitle,
    ActionBar,
    SetDurations,
    SetTitleJson,
    SetSubtitleJson,
    ActionBarJson,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleEvent {
    pub action: TitleAction,
    pub text: Arc<str>,
    pub document: Option<Arc<crate::RawTextDocument>>,
    pub fade_in_ticks: i32,
    pub stay_ticks: i32,
    pub fade_out_ticks: i32,
    pub xuid: Arc<str>,
    pub platform_online_id: Arc<str>,
    pub filtered_message: Arc<str>,
}

pub(crate) fn normalize_text(packet: Text) -> Result<UiEvent, UiPacketError> {
    let (category, kind, source, message, raw_text, parameters) = match packet.body {
        TextData::MessageOnly {
            message_type,
            message,
        } => match message_type {
            TextPacketType::Raw => normalize_message_only(TextKind::Raw, message)?,
            TextPacketType::Tip => normalize_message_only(TextKind::Tip, message)?,
            TextPacketType::Systemmessage => normalize_message_only(TextKind::System, message)?,
            TextPacketType::Textobjectwhisper => {
                normalize_json_message(TextKind::JsonWhisper, message)?
            }
            TextPacketType::Textobject => normalize_json_message(TextKind::Json, message)?,
            TextPacketType::Textobjectannouncement => {
                normalize_json_message(TextKind::JsonAnnouncement, message)?
            }
            other => return Err(unknown_text_type(other.to_raw())),
        },
        TextData::AuthorAndMessage {
            message_type,
            player_name,
            message,
        } => {
            let kind = match message_type {
                TextPacketType::Chat => TextKind::Chat,
                TextPacketType::Whisper => TextKind::Whisper,
                TextPacketType::Announcement => TextKind::Announcement,
                other => return Err(unknown_text_type(other.to_raw())),
            };
            normalize_authored(kind, player_name, message)?
        }
        TextData::MessageAndParams {
            message_type,
            message,
            parameter_list,
        } => {
            let kind = match message_type {
                TextPacketType::Translate => TextKind::Translation,
                TextPacketType::Popup => TextKind::Popup,
                TextPacketType::Jukeboxpopup => TextKind::JukeboxPopup,
                other => return Err(unknown_text_type(other.to_raw())),
            };
            normalize_parameters(kind, message, parameter_list)?
        }
    };
    let event = TextEvent {
        category,
        kind,
        needs_translation: packet.localize,
        source,
        message,
        parameters,
        xuid: bounded_text(packet.sender_xuid)?,
        platform_chat_id: bounded_text(packet.platform_id)?,
        filtered_message: packet.filtered_message.map(bounded_message).transpose()?,
    };
    Ok(match raw_text {
        Some(document) => UiEvent::RawText(RawTextEvent {
            text: event,
            document,
        }),
        None => UiEvent::Text(event),
    })
}

fn unknown_text_type(value: u8) -> UiPacketError {
    UiPacketError::UnknownEnum {
        kind: "text type",
        value: i64::from(value),
    }
}

type NormalizedTextParts = (
    TextCategory,
    TextKind,
    Option<Arc<str>>,
    Arc<str>,
    Option<Arc<crate::RawTextDocument>>,
    Arc<[Arc<str>]>,
);

/// Retains a Text message within its schema byte bound, including for directly built packets.
fn bounded_message(value: wire::Str) -> Result<Arc<str>, UiPacketError> {
    let value = value.to_string_lossy();
    if value.len() > MAX_TEXT_MESSAGE_BYTES {
        return Err(UiPacketError::TextTooLong {
            bytes: value.len(),
            max: MAX_TEXT_MESSAGE_BYTES,
        });
    }
    Ok(Arc::from(value))
}

fn normalize_message_only(
    kind: TextKind,
    message: wire::Str,
) -> Result<NormalizedTextParts, UiPacketError> {
    let message = bounded_message(message)?;
    let document = crate::raw_text::parse_raw_text_envelope(&message)?;
    let message = match &document {
        Some(document) => Arc::from(document.literal_text()),
        None => message,
    };
    Ok((
        TextCategory::MessageOnly,
        kind,
        None,
        message,
        document,
        Arc::from([]),
    ))
}

fn normalize_json_message(
    kind: TextKind,
    message: wire::Str,
) -> Result<NormalizedTextParts, UiPacketError> {
    let message = bounded_message(message)?;
    let document = crate::parse_raw_text(&message)?;
    Ok((
        TextCategory::MessageOnly,
        kind,
        None,
        Arc::from(document.literal_text()),
        Some(document),
        Arc::from([]),
    ))
}

fn normalize_authored(
    kind: TextKind,
    player_name: wire::Str,
    message: wire::Str,
) -> Result<NormalizedTextParts, UiPacketError> {
    Ok((
        TextCategory::Authored,
        kind,
        Some(bounded_text(player_name)?),
        bounded_message(message)?,
        None,
        Arc::from([]),
    ))
}

fn normalize_parameters(
    kind: TextKind,
    message: wire::Str,
    parameter_list: Vec<wire::Str>,
) -> Result<NormalizedTextParts, UiPacketError> {
    if parameter_list.len() > MAX_CHAT_PARAMETERS {
        return Err(UiPacketError::TooManyChatParameters {
            count: parameter_list.len(),
            max: MAX_CHAT_PARAMETERS,
        });
    }
    let parameters = parameter_list
        .into_iter()
        .map(bounded_text)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((
        TextCategory::Parameters,
        kind,
        None,
        bounded_message(message)?,
        None,
        Arc::from(parameters),
    ))
}

pub(crate) fn normalize_title(packet: SetTitle) -> Result<UiEvent, UiPacketError> {
    // Wire values match gophertunnel's `TitleAction*` constants
    // (`minecraft/protocol/packet/set_title.go`); 1.26.40 renamed the generated
    // variants to the Mojang spellings.
    let action = match packet.title_type {
        TitleType::Clear => TitleAction::Clear,
        TitleType::Reset => TitleAction::Reset,
        TitleType::Title => TitleAction::SetTitle,
        TitleType::Subtitle => TitleAction::SetSubtitle,
        TitleType::Actionbar => TitleAction::ActionBar,
        TitleType::Times => TitleAction::SetDurations,
        TitleType::Titletextobject => TitleAction::SetTitleJson,
        TitleType::Subtitletextobject => TitleAction::SetSubtitleJson,
        TitleType::Actionbartextobject => TitleAction::ActionBarJson,
        TitleType::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "title action",
                value: i64::from(value),
            });
        }
    };
    let (text, document) = if matches!(
        action,
        TitleAction::SetTitleJson | TitleAction::SetSubtitleJson | TitleAction::ActionBarJson
    ) {
        let document = crate::parse_raw_text(&packet.title_text.to_string_lossy())?;
        (Arc::from(document.literal_text()), Some(document))
    } else {
        (bounded_text(packet.title_text)?, None)
    };
    Ok(UiEvent::Title(TitleEvent {
        action,
        text,
        document,
        fade_in_ticks: packet.fade_in_time.0,
        stay_ticks: packet.stay_time.0,
        fade_out_ticks: packet.fade_out_time.0,
        xuid: bounded_text(packet.xuid)?,
        platform_online_id: bounded_text(packet.platform_online_id)?,
        filtered_message: bounded_text(packet.filtered_title_message)?,
    }))
}

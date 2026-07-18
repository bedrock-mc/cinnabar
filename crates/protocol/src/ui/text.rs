use std::sync::Arc;

use valentine::bedrock::version::v1_26_30::{
    SetTitlePacket, SetTitlePacketType, TextPacket, TextPacketCategory, TextPacketContent,
    TextPacketType,
};

use super::{MAX_CHAT_PARAMETERS, UiEvent, UiPacketError, bounded_text};

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

pub(crate) fn normalize_text(packet: TextPacket) -> Result<UiEvent, UiPacketError> {
    let category = match packet.category {
        TextPacketCategory::MessageOnly => TextCategory::MessageOnly,
        TextPacketCategory::Authored => TextCategory::Authored,
        TextPacketCategory::Parameters => TextCategory::Parameters,
        TextPacketCategory::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "text category",
                value: i64::from(value),
            });
        }
    };
    let kind = match packet.type_ {
        TextPacketType::Raw => TextKind::Raw,
        TextPacketType::Chat => TextKind::Chat,
        TextPacketType::Translation => TextKind::Translation,
        TextPacketType::Popup => TextKind::Popup,
        TextPacketType::JukeboxPopup => TextKind::JukeboxPopup,
        TextPacketType::Tip => TextKind::Tip,
        TextPacketType::System => TextKind::System,
        TextPacketType::Whisper => TextKind::Whisper,
        TextPacketType::Announcement => TextKind::Announcement,
        TextPacketType::JsonWhisper => TextKind::JsonWhisper,
        TextPacketType::Json => TextKind::Json,
        TextPacketType::JsonAnnouncement => TextKind::JsonAnnouncement,
        TextPacketType::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "text type",
                value: i64::from(value),
            });
        }
    };
    let (source, message, raw_text, parameters) = match packet.content {
        Some(TextPacketContent::Announcement(value))
        | Some(TextPacketContent::Chat(value))
        | Some(TextPacketContent::Whisper(value)) => (
            Some(bounded_text(value.source_name)?),
            bounded_text(value.message)?,
            None,
            Arc::from([]),
        ),
        Some(TextPacketContent::Json(value))
        | Some(TextPacketContent::JsonAnnouncement(value))
        | Some(TextPacketContent::JsonWhisper(value)) => {
            let document = crate::parse_raw_text(&value.message)?;
            (
                None,
                Arc::from(document.literal_text()),
                Some(document),
                Arc::from([]),
            )
        }
        Some(TextPacketContent::Raw(value))
        | Some(TextPacketContent::System(value))
        | Some(TextPacketContent::Tip(value)) => {
            let document = crate::raw_text::parse_raw_text_envelope(&value.message)?;
            let message = match &document {
                Some(document) => Arc::from(document.literal_text()),
                None => bounded_text(value.message)?,
            };
            (None, message, document, Arc::from([]))
        }
        Some(TextPacketContent::JukeboxPopup(value))
        | Some(TextPacketContent::Popup(value))
        | Some(TextPacketContent::Translation(value)) => {
            if value.parameters.len() > MAX_CHAT_PARAMETERS {
                return Err(UiPacketError::TooManyChatParameters {
                    count: value.parameters.len(),
                    max: MAX_CHAT_PARAMETERS,
                });
            }
            let parameters = value
                .parameters
                .into_iter()
                .map(bounded_text)
                .collect::<Result<Vec<_>, _>>()?;
            (
                None,
                bounded_text(value.message)?,
                None,
                Arc::from(parameters),
            )
        }
        None => (None, Arc::from(""), None, Arc::from([])),
    };
    let event = TextEvent {
        category,
        kind,
        needs_translation: packet.needs_translation,
        source,
        message,
        parameters,
        xuid: bounded_text(packet.xuid)?,
        platform_chat_id: bounded_text(packet.platform_chat_id)?,
        filtered_message: packet.filtered_message.map(bounded_text).transpose()?,
    };
    Ok(match raw_text {
        Some(document) => UiEvent::RawText(RawTextEvent {
            text: event,
            document,
        }),
        None => UiEvent::Text(event),
    })
}

pub(crate) fn normalize_title(packet: SetTitlePacket) -> Result<UiEvent, UiPacketError> {
    let action = match packet.type_ {
        SetTitlePacketType::Clear => TitleAction::Clear,
        SetTitlePacketType::Reset => TitleAction::Reset,
        SetTitlePacketType::SetTitle => TitleAction::SetTitle,
        SetTitlePacketType::SetSubtitle => TitleAction::SetSubtitle,
        SetTitlePacketType::ActionBarMessage => TitleAction::ActionBar,
        SetTitlePacketType::SetDurations => TitleAction::SetDurations,
        SetTitlePacketType::SetTitleJson => TitleAction::SetTitleJson,
        SetTitlePacketType::SetSubtitleJson => TitleAction::SetSubtitleJson,
        SetTitlePacketType::ActionBarMessageJson => TitleAction::ActionBarJson,
        SetTitlePacketType::Unknown(value) => {
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
        let document = crate::parse_raw_text(&packet.text)?;
        (Arc::from(document.literal_text()), Some(document))
    } else {
        (bounded_text(packet.text)?, None)
    };
    Ok(UiEvent::Title(TitleEvent {
        action,
        text,
        document,
        fade_in_ticks: packet.fade_in_time,
        stay_ticks: packet.stay_time,
        fade_out_ticks: packet.fade_out_time,
        xuid: bounded_text(packet.xuid)?,
        platform_online_id: bounded_text(packet.platform_online_id)?,
        filtered_message: bounded_text(packet.filtered_message)?,
    }))
}

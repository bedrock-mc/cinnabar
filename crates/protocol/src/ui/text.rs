use std::sync::Arc;

use valentine::bedrock::version::v1_26_40::{
    SetTitlePacket, SetTitlePacketTitleType, TextPacket, TextPacketBody,
    TextPacketPayloadAuthorAndMessageMessageType, TextPacketPayloadMessageAndParamsMessageType,
    TextPacketPayloadMessageOnlyMessageType,
};

use super::{MAX_CHAT_PARAMETERS, UiEvent, UiPacketError, bounded_text};

/// Which of the three Text payload shapes the packet carried.
///
/// 1.26.40 models this as the leading union tag of `TextPacketBody` rather than
/// a standalone `category` field. gophertunnel writes the same byte and derives
/// it from the message type
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

/// 1.26.40 gives each Text payload its own copy of the message-type enum, so the
/// identical 0..=11 mapping is generated once per payload shape. The wire values
/// are gophertunnel's `TextType*` constants
/// (`minecraft/protocol/packet/text.go`).
macro_rules! text_kind_from_wire {
    ($name:ident, $wire:ident) => {
        fn $name(value: $wire) -> Result<TextKind, UiPacketError> {
            Ok(match value {
                $wire::Raw => TextKind::Raw,
                $wire::Chat => TextKind::Chat,
                $wire::Translate => TextKind::Translation,
                $wire::Popup => TextKind::Popup,
                $wire::JukeboxPopup => TextKind::JukeboxPopup,
                $wire::Tip => TextKind::Tip,
                $wire::SystemMessage => TextKind::System,
                $wire::Whisper => TextKind::Whisper,
                $wire::Announcement => TextKind::Announcement,
                $wire::TextObjectWhisper => TextKind::JsonWhisper,
                $wire::TextObject => TextKind::Json,
                $wire::TextObjectAnnouncement => TextKind::JsonAnnouncement,
                $wire::Unknown(value) => {
                    return Err(UiPacketError::UnknownEnum {
                        kind: "text type",
                        value: i64::from(value),
                    });
                }
            })
        }
    };
}

text_kind_from_wire!(message_only_kind, TextPacketPayloadMessageOnlyMessageType);
text_kind_from_wire!(
    author_and_message_kind,
    TextPacketPayloadAuthorAndMessageMessageType
);
text_kind_from_wire!(
    message_and_params_kind,
    TextPacketPayloadMessageAndParamsMessageType
);

pub(crate) fn normalize_text(packet: TextPacket) -> Result<UiEvent, UiPacketError> {
    let (category, kind, source, message, raw_text, parameters) = match packet.body {
        TextPacketBody::MessageOnly(payload) => {
            let kind = message_only_kind(payload.message_type)?;
            let (message, document) = match kind {
                // The TextObject* kinds always carry a RawText document.
                TextKind::Json | TextKind::JsonAnnouncement | TextKind::JsonWhisper => {
                    let document = crate::parse_raw_text(&payload.message)?;
                    (Arc::from(document.literal_text()), Some(document))
                }
                // Raw/Tip/System may carry a RawText envelope; anything else is
                // an ordinary literal message that must stay verbatim.
                TextKind::Raw | TextKind::Tip | TextKind::System => {
                    let document = crate::raw_text::parse_raw_text_envelope(&payload.message)?;
                    let message = match &document {
                        Some(document) => Arc::from(document.literal_text()),
                        None => bounded_text(payload.message)?,
                    };
                    (message, document)
                }
                _ => (bounded_text(payload.message)?, None),
            };
            (
                TextCategory::MessageOnly,
                kind,
                None,
                message,
                document,
                Arc::from([]),
            )
        }
        TextPacketBody::AuthorAndMessage(payload) => {
            let kind = author_and_message_kind(payload.message_type)?;
            (
                TextCategory::Authored,
                kind,
                Some(bounded_text(payload.player_name)?),
                bounded_text(payload.message)?,
                None,
                Arc::from([]),
            )
        }
        TextPacketBody::MessageAndParams(payload) => {
            let kind = message_and_params_kind(payload.message_type)?;
            if payload.parameter_list.len() > MAX_CHAT_PARAMETERS {
                return Err(UiPacketError::TooManyChatParameters {
                    count: payload.parameter_list.len(),
                    max: MAX_CHAT_PARAMETERS,
                });
            }
            let parameters = payload
                .parameter_list
                .into_iter()
                .map(bounded_text)
                .collect::<Result<Vec<_>, _>>()?;
            (
                TextCategory::Parameters,
                kind,
                None,
                bounded_text(payload.message)?,
                None,
                Arc::from(parameters),
            )
        }
    };
    let event = TextEvent {
        category,
        kind,
        needs_translation: packet.localize,
        source,
        message,
        parameters,
        xuid: bounded_text(packet.senders_xuid)?,
        platform_chat_id: bounded_text(packet.platform_id)?,
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
    // Wire values match gophertunnel's `TitleAction*` constants
    // (`minecraft/protocol/packet/set_title.go`); 1.26.40 renamed the generated
    // variants to the Mojang spellings.
    let action = match packet.title_type {
        SetTitlePacketTitleType::Clear => TitleAction::Clear,
        SetTitlePacketTitleType::Reset => TitleAction::Reset,
        SetTitlePacketTitleType::Title => TitleAction::SetTitle,
        SetTitlePacketTitleType::Subtitle => TitleAction::SetSubtitle,
        SetTitlePacketTitleType::Actionbar => TitleAction::ActionBar,
        SetTitlePacketTitleType::Times => TitleAction::SetDurations,
        SetTitlePacketTitleType::TitleTextObject => TitleAction::SetTitleJson,
        SetTitlePacketTitleType::SubtitleTextObject => TitleAction::SetSubtitleJson,
        SetTitlePacketTitleType::ActionbarTextObject => TitleAction::ActionBarJson,
        SetTitlePacketTitleType::Unknown(value) => {
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
        let document = crate::parse_raw_text(&packet.title_text)?;
        (Arc::from(document.literal_text()), Some(document))
    } else {
        (bounded_text(packet.title_text)?, None)
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
        filtered_message: bounded_text(packet.filtered_title_message)?,
    }))
}

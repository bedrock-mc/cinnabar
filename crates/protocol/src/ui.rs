use std::{collections::BTreeMap, sync::Arc};

use thiserror::Error;
use valentine::bedrock::borrowed::BorrowedStr;
use valentine::bedrock::version::v1_26_30::{
    BorrowedMcpePacketData, BossEventPacket, BossEventPacketColor, BossEventPacketOverlay,
    BossEventPacketType, LevelEventPacket, LevelEventPacketEvent, ModalFormRequestPacket,
    PlayStatusPacket, PlayStatusPacketStatus, RemoveObjectivePacket, SetDisplayObjectivePacket,
    SetHealthPacket, SetScorePacket, SetScorePacketAction, SetScorePacketEntriesItemContent,
    SetScorePacketEntriesItemContentEntityUniqueId, SetScorePacketEntriesItemContentEntryType,
    TextPacket, TextPacketCategory, TextPacketContent, TextPacketContentAnnouncement,
    TextPacketContentView, TextPacketType, ToastRequestPacket, UpdateSoftEnumPacket,
    UpdateSoftEnumPacketActionType,
};

mod text;

pub use text::{RawTextEvent, TextCategory, TextEvent, TextKind, TitleAction, TitleEvent};
pub(crate) use text::{normalize_text, normalize_title};

pub const MAX_UI_TEXT_BYTES: usize = 16_384;
pub const MAX_CHAT_PARAMETERS: usize = 128;
pub const MAX_CHAT_AUTOCOMPLETE: usize = 256;
pub const MAX_CHAT_AUTOCOMPLETE_BYTES: usize = 65_536;
pub const MAX_SCORE_ENTRIES_PER_PACKET: usize = 8_192;
pub const MAX_BOSS_EVENTS: usize = 64;
pub const MAX_FORM_JSON_BYTES: usize = 1_048_576;
pub const MAX_OUTBOUND_CHAT_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ChatPacketError {
    #[error("chat message must not be empty")]
    EmptyMessage,
    #[error("chat message has {bytes} bytes, exceeding {max}")]
    MessageTooLong { bytes: usize, max: usize },
    #[error("chat identity field {field} has {bytes} bytes, exceeding {max}")]
    IdentityTooLong {
        field: &'static str,
        bytes: usize,
        max: usize,
    },
}

pub fn chat_text_packet(
    source_name: &str,
    xuid: &str,
    message: &str,
) -> Result<crate::Packet, ChatPacketError> {
    if message.is_empty() {
        return Err(ChatPacketError::EmptyMessage);
    }
    if message.len() > MAX_OUTBOUND_CHAT_BYTES {
        return Err(ChatPacketError::MessageTooLong {
            bytes: message.len(),
            max: MAX_OUTBOUND_CHAT_BYTES,
        });
    }
    for (field, value) in [("source_name", source_name), ("xuid", xuid)] {
        if value.len() > MAX_UI_TEXT_BYTES {
            return Err(ChatPacketError::IdentityTooLong {
                field,
                bytes: value.len(),
                max: MAX_UI_TEXT_BYTES,
            });
        }
    }
    Ok(TextPacket {
        needs_translation: false,
        category: TextPacketCategory::Authored,
        type_: TextPacketType::Chat,
        content: Some(TextPacketContent::Chat(TextPacketContentAnnouncement {
            source_name: source_name.to_owned(),
            message: message.to_owned(),
        })),
        xuid: xuid.to_owned(),
        platform_chat_id: String::new(),
        filtered_message: None,
    }
    .into())
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    Text(TextEvent),
    RawText(RawTextEvent),
    Title(TitleEvent),
    Hud(HudEvent),
    Objective(ObjectiveEvent),
    Score(ScoreEvent),
    Boss(BossEvent),
    Form(FormRequestEvent),
    ChatAutocomplete(ChatAutocompleteEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HudEvent {
    Toast { title: Arc<str>, message: Arc<str> },
    Health { health: i32 },
    PlayerStatus(PlayerStatus),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerStatus {
    LoginSuccess,
    FailedClient,
    FailedSpawn,
    PlayerSpawn,
    FailedInvalidTenant,
    FailedVanillaEducation,
    FailedEducationVanilla,
    FailedServerFull,
    FailedEditorVanillaMismatch,
    FailedVanillaEditorMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectiveEvent {
    Display {
        display_slot: Arc<str>,
        objective_name: Arc<str>,
        display_name: Arc<str>,
        criteria_name: Arc<str>,
        sort_order: i32,
    },
    Remove {
        objective_name: Arc<str>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreAction {
    Change,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoreIdentity {
    Player(i64),
    Entity(i64),
    FakePlayer(Arc<str>),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreEntry {
    pub scoreboard_id: i64,
    pub objective_name: Arc<str>,
    pub score: i32,
    pub identity: ScoreIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreEvent {
    pub action: ScoreAction,
    pub entries: Arc<[ScoreEntry]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossAction {
    Show,
    RegisterPlayer,
    Hide,
    UnregisterPlayer,
    SetProgress,
    SetTitle,
    UpdateProperties,
    Texture,
    Query,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossColor {
    Pink,
    Blue,
    Red,
    Green,
    Yellow,
    Purple,
    RebeccaPurple,
    White,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossOverlay {
    Progress,
    Notched6,
    Notched10,
    Notched12,
    Notched20,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BossStyle {
    pub color: BossColor,
    pub overlay: BossOverlay,
    pub darken_sky: Option<bool>,
    pub create_world_fog: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BossEvent {
    pub target_entity_id: i64,
    pub player_id: i64,
    pub action: BossAction,
    pub title: Arc<str>,
    pub filtered_title: Arc<str>,
    pub progress: f32,
    pub style: BossStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormRequestEvent {
    pub form_id: i32,
    pub json: Arc<str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAutocompleteEvent {
    pub enum_name: Arc<str>,
    pub action: ChatAutocompleteAction,
    pub suggestions: Arc<[Arc<str>]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAutocompleteAction {
    Add,
    Remove,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAutocompleteCompletion {
    pub catalog_revision: u64,
    pub suggestions: Arc<[Arc<str>]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ChatAutocompleteCatalogError {
    #[error("autocomplete input or cursor is invalid")]
    InvalidInput,
    #[error("autocomplete catalog has {count} suggestions, exceeding {max}")]
    TooManySuggestions { count: usize, max: usize },
    #[error("autocomplete catalog retains {bytes} bytes, exceeding {max}")]
    SuggestionsTooLarge { bytes: usize, max: usize },
}

#[derive(Debug, Clone, Default)]
pub struct ChatAutocompleteCatalog {
    revision: u64,
    enums: BTreeMap<Arc<str>, Vec<Arc<str>>>,
}

impl ChatAutocompleteCatalog {
    pub fn apply(
        &mut self,
        event: ChatAutocompleteEvent,
    ) -> Result<u64, ChatAutocompleteCatalogError> {
        let mut values = self
            .enums
            .get(&event.enum_name)
            .cloned()
            .unwrap_or_default();
        match event.action {
            ChatAutocompleteAction::Add => {
                for suggestion in event.suggestions.iter() {
                    if !values.contains(suggestion) {
                        values.push(Arc::clone(suggestion));
                    }
                }
            }
            ChatAutocompleteAction::Remove => {
                values.retain(|value| !event.suggestions.contains(value));
            }
            ChatAutocompleteAction::Replace => {
                values.clear();
                for suggestion in event.suggestions.iter() {
                    if !values.contains(suggestion) {
                        values.push(Arc::clone(suggestion));
                    }
                }
            }
        }
        let mut next = self.enums.clone();
        if values.is_empty() {
            next.remove(&event.enum_name);
        } else {
            next.insert(event.enum_name, values);
        }
        let count = next.values().map(Vec::len).sum::<usize>();
        if count > MAX_CHAT_AUTOCOMPLETE {
            return Err(ChatAutocompleteCatalogError::TooManySuggestions {
                count,
                max: MAX_CHAT_AUTOCOMPLETE,
            });
        }
        let bytes = next
            .iter()
            .map(|(name, values)| {
                name.len() + values.iter().map(|value| value.len()).sum::<usize>()
            })
            .sum::<usize>();
        if bytes > MAX_CHAT_AUTOCOMPLETE_BYTES {
            return Err(ChatAutocompleteCatalogError::SuggestionsTooLarge {
                bytes,
                max: MAX_CHAT_AUTOCOMPLETE_BYTES,
            });
        }
        self.enums = next;
        self.revision = self.revision.saturating_add(1);
        Ok(self.revision)
    }

    pub fn complete(
        &self,
        input: &str,
        cursor_byte: usize,
    ) -> Result<ChatAutocompleteCompletion, ChatAutocompleteCatalogError> {
        if input.len() > MAX_OUTBOUND_CHAT_BYTES
            || cursor_byte > input.len()
            || !input.is_char_boundary(cursor_byte)
        {
            return Err(ChatAutocompleteCatalogError::InvalidInput);
        }
        let prefix = input[..cursor_byte]
            .rsplit_once(char::is_whitespace)
            .map_or(&input[..cursor_byte], |(_, prefix)| prefix);
        let mut suggestions = Vec::new();
        for suggestion in self
            .enums
            .values()
            .flatten()
            .filter(|suggestion| suggestion.starts_with(prefix))
        {
            if !suggestions.contains(suggestion) {
                suggestions.push(Arc::clone(suggestion));
            }
        }
        suggestions.truncate(MAX_CHAT_AUTOCOMPLETE);
        Ok(ChatAutocompleteCompletion {
            catalog_revision: self.revision,
            suggestions: Arc::from(suggestions),
        })
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockCrackAction {
    Start { progress_per_tick: u16 },
    UpdateSpeed { progress_per_tick: u16 },
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockCrackEvent {
    pub position: [i32; 3],
    pub action: BlockCrackAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UiPacketError {
    #[error("UI text is {bytes} bytes, exceeding the {max}-byte limit")]
    TextTooLong { bytes: usize, max: usize },
    #[error("RawText input is {bytes} bytes, exceeding the {max}-byte limit")]
    RawTextInputTooLarge { bytes: usize, max: usize },
    #[error("RawText JSON is malformed, ambiguous, or contains unsupported fields")]
    InvalidRawText,
    #[error("RawText has {count} nodes, exceeding the {max}-node limit")]
    RawTextNodeLimitExceeded { count: usize, max: usize },
    #[error("RawText depth {depth} exceeds the maximum depth {max}")]
    RawTextDepthExceeded { depth: usize, max: usize },
    #[error("RawText has {count} components, exceeding the {max}-component limit")]
    RawTextComponentLimitExceeded { count: usize, max: usize },
    #[error("RawText literal output is {bytes} bytes, exceeding the {max}-byte limit")]
    RawTextOutputTooLarge { bytes: usize, max: usize },
    #[error("chat packet has {count} parameters, exceeding the {max}-parameter limit")]
    TooManyChatParameters { count: usize, max: usize },
    #[error("score packet has {count} entries, exceeding the {max}-entry limit")]
    TooManyScores { count: usize, max: usize },
    #[error("form JSON is {bytes} bytes, exceeding the {max}-byte limit")]
    FormTooLarge { bytes: usize, max: usize },
    #[error("autocomplete update has {count} suggestions, exceeding the {max}-suggestion limit")]
    TooManyAutocompleteSuggestions { count: usize, max: usize },
    #[error("autocomplete update retains {bytes} UTF-8 bytes, exceeding the {max}-byte limit")]
    AutocompleteTooLarge { bytes: usize, max: usize },
    #[error("UI packet field {field} is not valid UTF-8")]
    InvalidUtf8 { field: &'static str },
    #[error("unknown required {kind} value {value}")]
    UnknownEnum { kind: &'static str, value: i64 },
    #[error("boss progress must be finite (wire bits {bits:#010x})")]
    NonFiniteBossProgress { bits: u32 },
    #[error(
        "block crack position component {field} is not an exact i32 coordinate (wire bits {bits:#010x})"
    )]
    InvalidBlockCrackPosition { field: &'static str, bits: u32 },
    #[error("block crack progress-per-tick value must be in 1..=65535, got {value}")]
    InvalidBlockCrackSpeed { value: i32 },
}

fn bounded_text(value: String) -> Result<Arc<str>, UiPacketError> {
    if value.len() > MAX_UI_TEXT_BYTES {
        return Err(UiPacketError::TextTooLong {
            bytes: value.len(),
            max: MAX_UI_TEXT_BYTES,
        });
    }
    Ok(Arc::from(value))
}

fn bounded_form(value: String) -> Result<Arc<str>, UiPacketError> {
    if value.len() > MAX_FORM_JSON_BYTES {
        return Err(UiPacketError::FormTooLarge {
            bytes: value.len(),
            max: MAX_FORM_JSON_BYTES,
        });
    }
    Ok(Arc::from(value))
}

pub(crate) fn normalize_toast(packet: ToastRequestPacket) -> Result<UiEvent, UiPacketError> {
    Ok(UiEvent::Hud(HudEvent::Toast {
        title: bounded_text(packet.title)?,
        message: bounded_text(packet.message)?,
    }))
}

pub(crate) fn normalize_health(packet: SetHealthPacket) -> UiEvent {
    UiEvent::Hud(HudEvent::Health {
        health: packet.health,
    })
}

pub(crate) fn normalize_player_status(packet: PlayStatusPacket) -> Result<UiEvent, UiPacketError> {
    let status = match packet.status {
        PlayStatusPacketStatus::LoginSuccess => PlayerStatus::LoginSuccess,
        PlayStatusPacketStatus::FailedClient => PlayerStatus::FailedClient,
        PlayStatusPacketStatus::FailedSpawn => PlayerStatus::FailedSpawn,
        PlayStatusPacketStatus::PlayerSpawn => PlayerStatus::PlayerSpawn,
        PlayStatusPacketStatus::FailedInvalidTenant => PlayerStatus::FailedInvalidTenant,
        PlayStatusPacketStatus::FailedVanillaEdu => PlayerStatus::FailedVanillaEducation,
        PlayStatusPacketStatus::FailedEduVanilla => PlayerStatus::FailedEducationVanilla,
        PlayStatusPacketStatus::FailedServerFull => PlayerStatus::FailedServerFull,
        PlayStatusPacketStatus::FailedEditorVanillaMismatch => {
            PlayerStatus::FailedEditorVanillaMismatch
        }
        PlayStatusPacketStatus::FailedVanillaEditorMismatch => {
            PlayerStatus::FailedVanillaEditorMismatch
        }
        PlayStatusPacketStatus::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "player status",
                value: i64::from(value),
            });
        }
    };
    Ok(UiEvent::Hud(HudEvent::PlayerStatus(status)))
}

pub(crate) fn normalize_display_objective(
    packet: SetDisplayObjectivePacket,
) -> Result<UiEvent, UiPacketError> {
    Ok(UiEvent::Objective(ObjectiveEvent::Display {
        display_slot: bounded_text(packet.display_slot)?,
        objective_name: bounded_text(packet.objective_name)?,
        display_name: bounded_text(packet.display_name)?,
        criteria_name: bounded_text(packet.criteria_name)?,
        sort_order: packet.sort_order,
    }))
}

pub(crate) fn normalize_remove_objective(
    packet: RemoveObjectivePacket,
) -> Result<UiEvent, UiPacketError> {
    Ok(UiEvent::Objective(ObjectiveEvent::Remove {
        objective_name: bounded_text(packet.objective_name)?,
    }))
}

fn score_identity(
    content: Option<Box<SetScorePacketEntriesItemContent>>,
) -> Result<ScoreIdentity, UiPacketError> {
    let Some(content) = content else {
        return Ok(ScoreIdentity::None);
    };
    match content.entry_type {
        SetScorePacketEntriesItemContentEntryType::Player => match content.entity_unique_id {
            Some(SetScorePacketEntriesItemContentEntityUniqueId::Player(id)) => {
                Ok(ScoreIdentity::Player(id))
            }
            _ => Ok(ScoreIdentity::None),
        },
        SetScorePacketEntriesItemContentEntryType::Entity => match content.entity_unique_id {
            Some(SetScorePacketEntriesItemContentEntityUniqueId::Entity(id)) => {
                Ok(ScoreIdentity::Entity(id))
            }
            _ => Ok(ScoreIdentity::None),
        },
        SetScorePacketEntriesItemContentEntryType::FakePlayer => Ok(ScoreIdentity::FakePlayer(
            bounded_text(content.custom_name.unwrap_or_default())?,
        )),
        SetScorePacketEntriesItemContentEntryType::Unknown(value) => {
            Err(UiPacketError::UnknownEnum {
                kind: "score identity",
                value: i64::from(value),
            })
        }
    }
}

pub(crate) fn normalize_score(packet: SetScorePacket) -> Result<UiEvent, UiPacketError> {
    if packet.entries.len() > MAX_SCORE_ENTRIES_PER_PACKET {
        return Err(UiPacketError::TooManyScores {
            count: packet.entries.len(),
            max: MAX_SCORE_ENTRIES_PER_PACKET,
        });
    }
    let action = match packet.action {
        SetScorePacketAction::Change => ScoreAction::Change,
        SetScorePacketAction::Remove => ScoreAction::Remove,
        SetScorePacketAction::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "score action",
                value: i64::from(value),
            });
        }
    };
    let entries = packet
        .entries
        .into_iter()
        .map(|entry| {
            Ok(ScoreEntry {
                scoreboard_id: entry.scoreboard_id,
                objective_name: bounded_text(entry.objective_name)?,
                score: entry.score,
                identity: score_identity(entry.content)?,
            })
        })
        .collect::<Result<Vec<_>, UiPacketError>>()?;
    Ok(UiEvent::Score(ScoreEvent {
        action,
        entries: Arc::from(entries),
    }))
}

pub(crate) fn normalize_boss(packet: BossEventPacket) -> Result<UiEvent, UiPacketError> {
    if !packet.progress.is_finite() {
        return Err(UiPacketError::NonFiniteBossProgress {
            bits: packet.progress.to_bits(),
        });
    }
    let action = match packet.type_ {
        BossEventPacketType::ShowBar => BossAction::Show,
        BossEventPacketType::RegisterPlayer => BossAction::RegisterPlayer,
        BossEventPacketType::HideBar => BossAction::Hide,
        BossEventPacketType::UnregisterPlayer => BossAction::UnregisterPlayer,
        BossEventPacketType::SetBarProgress => BossAction::SetProgress,
        BossEventPacketType::SetBarTitle => BossAction::SetTitle,
        BossEventPacketType::UpdateProperties => BossAction::UpdateProperties,
        BossEventPacketType::Texture => BossAction::Texture,
        BossEventPacketType::Query => BossAction::Query,
        BossEventPacketType::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "boss action",
                value: i64::from(value),
            });
        }
    };
    let color = match packet.color {
        BossEventPacketColor::Pink => BossColor::Pink,
        BossEventPacketColor::Blue => BossColor::Blue,
        BossEventPacketColor::Red => BossColor::Red,
        BossEventPacketColor::Green => BossColor::Green,
        BossEventPacketColor::Yellow => BossColor::Yellow,
        BossEventPacketColor::Purple => BossColor::Purple,
        BossEventPacketColor::RebeccaPurple => BossColor::RebeccaPurple,
        BossEventPacketColor::White => BossColor::White,
        BossEventPacketColor::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "boss color",
                value: i64::from(value),
            });
        }
    };
    let overlay = match packet.overlay {
        BossEventPacketOverlay::Progress => BossOverlay::Progress,
        BossEventPacketOverlay::Notched6 => BossOverlay::Notched6,
        BossEventPacketOverlay::Notched10 => BossOverlay::Notched10,
        BossEventPacketOverlay::Notched12 => BossOverlay::Notched12,
        BossEventPacketOverlay::Notched20 => BossOverlay::Notched20,
        BossEventPacketOverlay::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "boss overlay",
                value: i64::from(value),
            });
        }
    };
    Ok(UiEvent::Boss(BossEvent {
        target_entity_id: packet.target_entity_id,
        player_id: packet.player_id,
        action,
        title: bounded_text(packet.title)?,
        filtered_title: bounded_text(packet.filtered_title)?,
        progress: packet.progress,
        style: BossStyle {
            color,
            overlay,
            // Protocol 1001's BossEvent wire has no sky-darkening or fog fields.
            // Preserve that absence instead of inventing a vanilla style value.
            darken_sky: None,
            create_world_fog: None,
        },
    }))
}

pub(crate) fn normalize_form(packet: ModalFormRequestPacket) -> Result<UiEvent, UiPacketError> {
    Ok(UiEvent::Form(FormRequestEvent {
        form_id: packet.form_id,
        json: bounded_form(packet.data)?,
    }))
}

/// Normalizes the protocol-1001 server soft-enum delta used by local command completion.
/// Request identities belong to the local editor; this wire packet carries no request ID.
pub(crate) fn normalize_soft_enum(packet: UpdateSoftEnumPacket) -> Result<UiEvent, UiPacketError> {
    if packet.options.len() > MAX_CHAT_AUTOCOMPLETE {
        return Err(UiPacketError::TooManyAutocompleteSuggestions {
            count: packet.options.len(),
            max: MAX_CHAT_AUTOCOMPLETE,
        });
    }
    let enum_name = bounded_text(packet.enum_type)?;
    let mut retained_bytes = enum_name.len();
    let mut suggestions = Vec::with_capacity(packet.options.len());
    for option in packet.options {
        retained_bytes = retained_bytes.checked_add(option.len()).ok_or(
            UiPacketError::AutocompleteTooLarge {
                bytes: usize::MAX,
                max: MAX_CHAT_AUTOCOMPLETE_BYTES,
            },
        )?;
        if retained_bytes > MAX_CHAT_AUTOCOMPLETE_BYTES {
            return Err(UiPacketError::AutocompleteTooLarge {
                bytes: retained_bytes,
                max: MAX_CHAT_AUTOCOMPLETE_BYTES,
            });
        }
        suggestions.push(bounded_text(option)?);
    }
    let action = match packet.action_type {
        UpdateSoftEnumPacketActionType::Add => ChatAutocompleteAction::Add,
        UpdateSoftEnumPacketActionType::Remove => ChatAutocompleteAction::Remove,
        UpdateSoftEnumPacketActionType::Update => ChatAutocompleteAction::Replace,
        UpdateSoftEnumPacketActionType::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "soft enum action",
                value: i64::from(value),
            });
        }
    };
    Ok(UiEvent::ChatAutocomplete(ChatAutocompleteEvent {
        enum_name,
        action,
        suggestions: Arc::from(suggestions),
    }))
}

/// Normalizes protocol-1001 block cracking without inventing a stage or actor ID.
///
/// The wire `data` field is the server-authored progress rate (`65535 / break_ticks`).
/// A downstream tick owner may derive the ten visual atlas stages from accumulated
/// authoritative progress, but packet normalization preserves the exact rate.
pub(crate) fn normalize_block_crack(
    packet: LevelEventPacket,
) -> Result<BlockCrackEvent, UiPacketError> {
    let position = [
        exact_block_coordinate(packet.position.x, "x")?,
        exact_block_coordinate(packet.position.y, "y")?,
        exact_block_coordinate(packet.position.z, "z")?,
    ];
    let action = match packet.event {
        LevelEventPacketEvent::BlockStopBreak => BlockCrackAction::Stop,
        LevelEventPacketEvent::BlockStartBreak | LevelEventPacketEvent::BlockBreakSpeed => {
            let progress_per_tick = u16::try_from(packet.data)
                .ok()
                .filter(|value| *value != 0)
                .ok_or(UiPacketError::InvalidBlockCrackSpeed { value: packet.data })?;
            if packet.event == LevelEventPacketEvent::BlockStartBreak {
                BlockCrackAction::Start { progress_per_tick }
            } else {
                BlockCrackAction::UpdateSpeed { progress_per_tick }
            }
        }
        _ => unreachable!("caller only dispatches block crack level events"),
    };
    Ok(BlockCrackEvent { position, action })
}

fn exact_block_coordinate(value: f32, field: &'static str) -> Result<i32, UiPacketError> {
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(UiPacketError::InvalidBlockCrackPosition {
            field,
            bits: value.to_bits(),
        });
    }
    i32::try_from(value as i64).map_err(|_| UiPacketError::InvalidBlockCrackPosition {
        field,
        bits: value.to_bits(),
    })
}

fn validate_utf8(value: &BorrowedStr, field: &'static str) -> Result<(), UiPacketError> {
    if value.as_bytes().len() > MAX_UI_TEXT_BYTES {
        return Err(UiPacketError::TextTooLong {
            bytes: value.as_bytes().len(),
            max: MAX_UI_TEXT_BYTES,
        });
    }
    value
        .as_str()
        .map(|_| ())
        .map_err(|_| UiPacketError::InvalidUtf8 { field })
}

pub(crate) fn validate_borrowed_ui_packet(
    packet: &BorrowedMcpePacketData,
) -> Result<(), UiPacketError> {
    match packet {
        BorrowedMcpePacketData::PacketText(packet) => {
            if let Some(content) = &packet.content {
                match content {
                    TextPacketContentView::Announcement(value)
                    | TextPacketContentView::Chat(value)
                    | TextPacketContentView::Whisper(value) => {
                        validate_utf8(&value.source_name, "text.source_name")?;
                        validate_utf8(&value.message, "text.message")?;
                    }
                    TextPacketContentView::Json(value)
                    | TextPacketContentView::JsonAnnouncement(value)
                    | TextPacketContentView::JsonWhisper(value)
                    | TextPacketContentView::Raw(value)
                    | TextPacketContentView::System(value)
                    | TextPacketContentView::Tip(value) => {
                        validate_utf8(&value.message, "text.message")?;
                    }
                    TextPacketContentView::JukeboxPopup(value)
                    | TextPacketContentView::Popup(value)
                    | TextPacketContentView::Translation(value) => {
                        validate_utf8(&value.message, "text.message")?;
                        for parameter in &value.parameters {
                            validate_utf8(parameter, "text.parameter")?;
                        }
                    }
                }
            }
            validate_utf8(&packet.xuid, "text.xuid")?;
            validate_utf8(&packet.platform_chat_id, "text.platform_chat_id")?;
            if let Some(filtered_message) = &packet.filtered_message {
                validate_utf8(filtered_message, "text.filtered_message")?;
            }
            Ok(())
        }
        BorrowedMcpePacketData::PacketModalFormRequest(packet) => {
            if packet.data.as_bytes().len() > MAX_FORM_JSON_BYTES {
                return Err(UiPacketError::FormTooLarge {
                    bytes: packet.data.as_bytes().len(),
                    max: MAX_FORM_JSON_BYTES,
                });
            }
            packet
                .data
                .as_str()
                .map(|_| ())
                .map_err(|_| UiPacketError::InvalidUtf8 {
                    field: "modal_form.data",
                })
        }
        BorrowedMcpePacketData::PacketSetTitle(packet) => {
            validate_utf8(&packet.text, "set_title.text")?;
            validate_utf8(&packet.xuid, "set_title.xuid")?;
            validate_utf8(&packet.platform_online_id, "set_title.platform_online_id")?;
            validate_utf8(&packet.filtered_message, "set_title.filtered_message")
        }
        BorrowedMcpePacketData::PacketBossEvent(packet) => {
            validate_utf8(&packet.title, "boss.title")?;
            validate_utf8(&packet.filtered_title, "boss.filtered_title")
        }
        BorrowedMcpePacketData::PacketToastRequest(packet) => {
            validate_utf8(&packet.title, "toast.title")?;
            validate_utf8(&packet.message, "toast.message")
        }
        BorrowedMcpePacketData::PacketRemoveObjective(packet) => {
            validate_utf8(&packet.objective_name, "objective.name")
        }
        BorrowedMcpePacketData::PacketSetDisplayObjective(packet) => {
            validate_utf8(&packet.display_slot, "objective.display_slot")?;
            validate_utf8(&packet.objective_name, "objective.name")?;
            validate_utf8(&packet.display_name, "objective.display_name")?;
            validate_utf8(&packet.criteria_name, "objective.criteria_name")
        }
        BorrowedMcpePacketData::PacketUpdateSoftEnum(packet) => {
            if packet.options.len() > MAX_CHAT_AUTOCOMPLETE {
                return Err(UiPacketError::TooManyAutocompleteSuggestions {
                    count: packet.options.len(),
                    max: MAX_CHAT_AUTOCOMPLETE,
                });
            }
            validate_utf8(&packet.enum_type, "soft_enum.name")?;
            let mut retained_bytes = packet.enum_type.as_bytes().len();
            for option in &packet.options {
                validate_utf8(option, "soft_enum.option")?;
                retained_bytes = retained_bytes.checked_add(option.as_bytes().len()).ok_or(
                    UiPacketError::AutocompleteTooLarge {
                        bytes: usize::MAX,
                        max: MAX_CHAT_AUTOCOMPLETE_BYTES,
                    },
                )?;
                if retained_bytes > MAX_CHAT_AUTOCOMPLETE_BYTES {
                    return Err(UiPacketError::AutocompleteTooLarge {
                        bytes: retained_bytes,
                        max: MAX_CHAT_AUTOCOMPLETE_BYTES,
                    });
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

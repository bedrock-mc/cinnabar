use std::{collections::BTreeMap, sync::Arc};

use thiserror::Error;
use valentine::bedrock::borrowed::BorrowedStr;
use valentine::bedrock::version::v1_26_40::{
    BorrowedMcpePacketData, BossEventPacket, BossEventPacketColor, BossEventPacketEventType,
    BossEventPacketOverlay, CommandOriginData, CommandOutputPacket, CommandRequestPacket,
    LevelEventPacket, ModalFormRequestPacket, PlayStatusPacket, PlayStatusPacketStatus,
    RemoveObjectivePacket, SetDisplayObjectivePacket, SetHealthPacket, SetScorePacket,
    SetScorePacketScoreInfoItem, TextPacket, TextPacketBody, TextPacketPayloadAuthorAndMessage,
    TextPacketPayloadAuthorAndMessageMessageType, ToastRequestPacket, UpdateSoftEnumPacket,
    UpdateSoftEnumPacketUpdateType,
};

mod text;

pub use text::{RawTextEvent, TextCategory, TextEvent, TextKind, TitleAction, TitleEvent};
pub(crate) use text::{normalize_text, normalize_title};

pub const MAX_UI_TEXT_BYTES: usize = 16_384;
pub const MAX_CHAT_PARAMETERS: usize = 128;
pub const MAX_COMMAND_OUTPUT_MESSAGES: usize = 128;
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

fn validate_outbound_chat(
    source_name: &str,
    xuid: &str,
    message: &str,
) -> Result<(), ChatPacketError> {
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
    Ok(())
}

pub fn chat_text_packet(
    source_name: &str,
    xuid: &str,
    message: &str,
) -> Result<crate::Packet, ChatPacketError> {
    validate_outbound_chat(source_name, xuid, message)?;
    // gophertunnel derives the union tag from the message type
    // (`minecraft/protocol/packet/text.go`, `Text.Marshal`): TextTypeChat is
    // written under TextCategoryAuthoredMessage, which is the AuthorAndMessage
    // payload here.
    Ok(TextPacket {
        localize: false,
        body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
            message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
            player_name: source_name.to_owned(),
            message: message.to_owned(),
        }),
        senders_xuid: xuid.to_owned(),
        platform_id: String::new(),
        filtered_message: None,
    }
    .into())
}

/// Builds the vanilla outbound packet for a chat-editor submission.
///
/// Slash-prefixed input is a vanilla command request. Other input retains the
/// authored chat packet shape used by [`chat_text_packet`].
pub fn chat_input_packet(
    source_name: &str,
    xuid: &str,
    message: &str,
) -> Result<crate::Packet, ChatPacketError> {
    validate_outbound_chat(source_name, xuid, message)?;
    if !message.starts_with('/') {
        return chat_text_packet(source_name, xuid, message);
    }

    // The origin discriminant is a lowercase name string on this wire, not an
    // integer: gophertunnel's `commandOriginToString` maps
    // `CommandOriginPlayer` to exactly "player"
    // (`minecraft/protocol/command.go`).
    Ok(CommandRequestPacket {
        command: message.to_owned(),
        origin: CommandOriginData {
            type_: "player".to_owned(),
            uuid: uuid::Uuid::new_v4(),
            request_id: String::new(),
            player_id: 0,
        },
        is_internal: false,
        version: "latest".to_owned(),
    }
    .into())
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    Text(TextEvent),
    CommandOutput(CommandOutputEvent),
    RawText(RawTextEvent),
    Title(TitleEvent),
    Hud(HudEvent),
    Objective(ObjectiveEvent),
    Score(ScoreEvent),
    Boss(BossEvent),
    Form(FormRequestEvent),
    ChatAutocomplete(ChatAutocompleteEvent),
    GameMode(GameModeEvent),
    /// SetDefaultGameType: the level's default mode changed; players whose
    /// mode is bound to the default follow it.
    DefaultGameMode(GameModeEvent),
}

/// One wire game-mode value, retained without guessing.
///
/// `WorldDefault` is the explicit level-default sentinel (`GameMode 5`); the
/// receiver resolves it against its retained world default. `Unknown` keeps
/// the raw value so the receiver can count the skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameModeUpdate {
    Explicit(crate::PlayerGameMode),
    WorldDefault,
    Unknown(i32),
}

/// A runtime SetPlayerGameType / SetDefaultGameType change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameModeEvent {
    pub update: GameModeUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutputMessage {
    pub message_id: Arc<str>,
    pub success: bool,
    pub parameters: Arc<[Arc<str>]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutputEvent {
    pub output_type: Arc<str>,
    pub success_count: u32,
    pub messages: Arc<[CommandOutputMessage]>,
    pub data: Option<Arc<str>>,
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

/// One scoreboard line update.
///
/// 1.26.40 moved the add/remove verb out of the packet and into each entry:
/// gophertunnel's `ScoreboardEntry.Marshal` (`minecraft/protocol/scoreboard.go`)
/// writes a per-entry variant of "remove", "changeplayer", "changeentity" or
/// "changefakeplayer", so a single packet may mix removals with changes. A
/// `Remove` entry carries only an optional objective name on the wire, so
/// `score` reads 0 and `identity` reads `ScoreIdentity::None` for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreEntry {
    pub action: ScoreAction,
    pub scoreboard_id: i64,
    pub objective_name: Arc<str>,
    pub score: i32,
    pub identity: ScoreIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreEvent {
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
    #[error("command output has {count} messages, exceeding the {max}-message limit")]
    TooManyCommandOutputMessages { count: usize, max: usize },
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

pub(crate) fn normalize_command_output(
    packet: CommandOutputPacket,
) -> Result<UiEvent, UiPacketError> {
    // 1.26.40 groups the output-type, success count, messages and data set into
    // a nested `CommandOutput`; the wire order is unchanged
    // (gophertunnel `minecraft/protocol/packet/command_output.go`).
    let output = packet.output;
    if output.output_messages.len() > MAX_COMMAND_OUTPUT_MESSAGES {
        return Err(UiPacketError::TooManyCommandOutputMessages {
            count: output.output_messages.len(),
            max: MAX_COMMAND_OUTPUT_MESSAGES,
        });
    }
    let messages = output
        .output_messages
        .into_iter()
        .map(|message| {
            if message.parameters.len() > MAX_CHAT_PARAMETERS {
                return Err(UiPacketError::TooManyChatParameters {
                    count: message.parameters.len(),
                    max: MAX_CHAT_PARAMETERS,
                });
            }
            Ok(CommandOutputMessage {
                message_id: bounded_text(message.message_id)?,
                success: message.successful,
                parameters: Arc::from(
                    message
                        .parameters
                        .into_iter()
                        .map(bounded_text)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(UiEvent::CommandOutput(CommandOutputEvent {
        output_type: bounded_text(output.output_type)?,
        success_count: output.success_count,
        messages: Arc::from(messages),
        data: output.data_set.map(bounded_text).transpose()?,
    }))
}

pub(crate) fn normalize_toast(packet: ToastRequestPacket) -> Result<UiEvent, UiPacketError> {
    Ok(UiEvent::Hud(HudEvent::Toast {
        title: bounded_text(packet.title)?,
        message: bounded_text(packet.content)?,
    }))
}

pub(crate) fn normalize_health(packet: SetHealthPacket) -> UiEvent {
    UiEvent::Hud(HudEvent::Health {
        health: packet.health,
    })
}

pub(crate) fn normalize_player_status(packet: PlayStatusPacket) -> Result<UiEvent, UiPacketError> {
    // 1.26.40 renamed every failure variant to the Mojang spelling. The mapping
    // below is by wire value against gophertunnel's `PlayStatus*` constants
    // (`minecraft/protocol/packet/play_status.go`): 1 = client outdated,
    // 2 = server outdated (this crate's historical `FailedSpawn` name),
    // 5 = vanilla client -> education server, 6 = education client -> vanilla
    // server, 8 = editor client -> vanilla server, 9 = vanilla -> editor.
    let status = match packet.status {
        PlayStatusPacketStatus::LoginSuccess => PlayerStatus::LoginSuccess,
        PlayStatusPacketStatus::LoginFailedClientOld => PlayerStatus::FailedClient,
        PlayStatusPacketStatus::LoginFailedServerOld => PlayerStatus::FailedSpawn,
        PlayStatusPacketStatus::PlayerSpawn => PlayerStatus::PlayerSpawn,
        PlayStatusPacketStatus::LoginFailedInvalidTenant => PlayerStatus::FailedInvalidTenant,
        PlayStatusPacketStatus::LoginFailedEditionMismatchEduToVanilla => {
            PlayerStatus::FailedVanillaEducation
        }
        PlayStatusPacketStatus::LoginFailedEditionMismatchVanillaToEdu => {
            PlayerStatus::FailedEducationVanilla
        }
        PlayStatusPacketStatus::LoginFailedServerFullSubClient => PlayerStatus::FailedServerFull,
        PlayStatusPacketStatus::LoginFailedEditorMismatchEditorToVanilla => {
            PlayerStatus::FailedEditorVanillaMismatch
        }
        PlayStatusPacketStatus::LoginFailedEditorMismatchVanillaToEditor => {
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
        display_slot: bounded_text(packet.display_slot_name)?,
        objective_name: bounded_text(packet.objective_name)?,
        display_name: bounded_text(packet.objective_display_name)?,
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

/// Normalizes SetScore, whose verb is now carried per entry.
///
/// The unknown-identity arm the protocol-1001 shape needed is gone: the entry
/// union only decodes the four variants gophertunnel writes, and an out-of-range
/// variant is rejected by the generated decoder before it reaches here.
pub(crate) fn normalize_score(packet: SetScorePacket) -> Result<UiEvent, UiPacketError> {
    if packet.score_info.len() > MAX_SCORE_ENTRIES_PER_PACKET {
        return Err(UiPacketError::TooManyScores {
            count: packet.score_info.len(),
            max: MAX_SCORE_ENTRIES_PER_PACKET,
        });
    }
    let entries = packet
        .score_info
        .into_iter()
        .map(|entry| match entry {
            SetScorePacketScoreInfoItem::RemoveScore(entry) => Ok(ScoreEntry {
                action: ScoreAction::Remove,
                scoreboard_id: entry.scoreboard_id.scoreboard_id,
                // A removal carries an optional objective name and nothing
                // else, so there is no score or identity to report.
                objective_name: bounded_text(entry.objective_name.unwrap_or_default())?,
                score: 0,
                identity: ScoreIdentity::None,
            }),
            SetScorePacketScoreInfoItem::ChangePlayerScore(entry) => Ok(ScoreEntry {
                action: ScoreAction::Change,
                scoreboard_id: entry.scoreboard_id.scoreboard_id,
                objective_name: bounded_text(entry.objective_name)?,
                score: entry.score_value,
                identity: ScoreIdentity::Player(entry.player_unique_id.player_unique_id),
            }),
            SetScorePacketScoreInfoItem::ChangeEntityScore(entry) => Ok(ScoreEntry {
                action: ScoreAction::Change,
                scoreboard_id: entry.scoreboard_id.scoreboard_id,
                objective_name: bounded_text(entry.objective_name)?,
                score: entry.score_value,
                identity: ScoreIdentity::Entity(entry.actor_id.actor_unique_id),
            }),
            SetScorePacketScoreInfoItem::ChangeFakePlayerScore(entry) => Ok(ScoreEntry {
                action: ScoreAction::Change,
                scoreboard_id: entry.scoreboard_id.scoreboard_id,
                objective_name: bounded_text(entry.objective_name)?,
                score: entry.score_value,
                identity: ScoreIdentity::FakePlayer(bounded_text(entry.fake_player_name)?),
            }),
        })
        .collect::<Result<Vec<_>, UiPacketError>>()?;
    Ok(UiEvent::Score(ScoreEvent {
        entries: Arc::from(entries),
    }))
}

pub(crate) fn normalize_boss(packet: BossEventPacket) -> Result<UiEvent, UiPacketError> {
    if !packet.health_percent.is_finite() {
        return Err(UiPacketError::NonFiniteBossProgress {
            bits: packet.health_percent.to_bits(),
        });
    }
    // 1.26.40 renamed the event verbs to the Mojang spellings; the wire values
    // are unchanged and still line up 0..=8 with gophertunnel's `BossEvent*`
    // constants (`minecraft/protocol/packet/boss_event.go`), so `UpdateStyle`
    // is value 7, the one gophertunnel calls `BossEventTexture`.
    let action = match packet.event_type {
        BossEventPacketEventType::Add => BossAction::Show,
        BossEventPacketEventType::PlayerAdded => BossAction::RegisterPlayer,
        BossEventPacketEventType::Remove => BossAction::Hide,
        BossEventPacketEventType::PlayerRemoved => BossAction::UnregisterPlayer,
        BossEventPacketEventType::UpdatePercent => BossAction::SetProgress,
        BossEventPacketEventType::UpdateName => BossAction::SetTitle,
        BossEventPacketEventType::UpdateProperties => BossAction::UpdateProperties,
        BossEventPacketEventType::UpdateStyle => BossAction::Texture,
        BossEventPacketEventType::Query => BossAction::Query,
        BossEventPacketEventType::Unknown(value) => {
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
        target_entity_id: packet.target_actor_id.actor_unique_id,
        player_id: packet.player_id.actor_unique_id,
        action,
        title: bounded_text(packet.name)?,
        filtered_title: bounded_text(packet.filtered_name)?,
        progress: packet.health_percent,
        style: BossStyle {
            color,
            overlay,
            // The 1.26.40 BossEvent wire has no sky-darkening or fog fields
            // (gophertunnel `BossEvent.Marshal` writes only the two ids, the
            // event type, both titles, the health percentage, the colour and
            // the overlay). Preserve that absence instead of inventing a
            // vanilla style value.
            darken_sky: None,
            create_world_fog: None,
        },
    }))
}

pub(crate) fn normalize_form(packet: ModalFormRequestPacket) -> Result<UiEvent, UiPacketError> {
    Ok(UiEvent::Form(FormRequestEvent {
        form_id: packet.form_id,
        json: bounded_form(packet.form_uijson)?,
    }))
}

/// Normalizes the server soft-enum delta used by local command completion.
/// Request identities belong to the local editor; this wire packet carries no request ID.
pub(crate) fn normalize_soft_enum(packet: UpdateSoftEnumPacket) -> Result<UiEvent, UiPacketError> {
    if packet.values.len() > MAX_CHAT_AUTOCOMPLETE {
        return Err(UiPacketError::TooManyAutocompleteSuggestions {
            count: packet.values.len(),
            max: MAX_CHAT_AUTOCOMPLETE,
        });
    }
    let enum_name = bounded_text(packet.enum_name)?;
    let mut retained_bytes = enum_name.len();
    let mut suggestions = Vec::with_capacity(packet.values.len());
    for option in packet.values {
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
    // `Replace` is wire value 2, gophertunnel's `SoftEnumActionSet`
    // (`minecraft/protocol/packet/update_soft_enum.go`) — the same value the
    // protocol-1001 shape spelled `Update`.
    let action = match packet.update_type {
        UpdateSoftEnumPacketUpdateType::Add => ChatAutocompleteAction::Add,
        UpdateSoftEnumPacketUpdateType::Remove => ChatAutocompleteAction::Remove,
        UpdateSoftEnumPacketUpdateType::Replace => ChatAutocompleteAction::Replace,
        UpdateSoftEnumPacketUpdateType::Unknown(value) => {
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

/// Block-cracking LevelEvent ids.
///
/// 1.26.40 carries LevelEvent as a raw `event_id` instead of a named enum, so
/// the three ids this module cares about are pinned here against gophertunnel's
/// `LevelEventStartBlockCracking` / `LevelEventStopBlockCracking` /
/// `LevelEventUpdateBlockCracking` (`minecraft/protocol/packet/level_event.go`).
pub(crate) const LEVEL_EVENT_START_BLOCK_CRACKING: i32 = 3600;
pub(crate) const LEVEL_EVENT_STOP_BLOCK_CRACKING: i32 = 3601;
pub(crate) const LEVEL_EVENT_UPDATE_BLOCK_CRACKING: i32 = 3602;

/// Whether a LevelEvent id is one of the three block-cracking events.
#[must_use]
pub(crate) const fn is_block_crack_event(event_id: i32) -> bool {
    matches!(
        event_id,
        LEVEL_EVENT_START_BLOCK_CRACKING
            | LEVEL_EVENT_STOP_BLOCK_CRACKING
            | LEVEL_EVENT_UPDATE_BLOCK_CRACKING
    )
}

/// Normalizes block cracking without inventing a stage or actor ID.
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
    let action = match packet.event_id {
        LEVEL_EVENT_STOP_BLOCK_CRACKING => BlockCrackAction::Stop,
        LEVEL_EVENT_START_BLOCK_CRACKING | LEVEL_EVENT_UPDATE_BLOCK_CRACKING => {
            let progress_per_tick = u16::try_from(packet.data)
                .ok()
                .filter(|value| *value != 0)
                .ok_or(UiPacketError::InvalidBlockCrackSpeed { value: packet.data })?;
            if packet.event_id == LEVEL_EVENT_START_BLOCK_CRACKING {
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

/// Length-only check for a string the borrowed view already materialized.
fn bounded_borrowed_text(value: &str) -> Result<(), UiPacketError> {
    if value.len() > MAX_UI_TEXT_BYTES {
        return Err(UiPacketError::TextTooLong {
            bytes: value.len(),
            max: MAX_UI_TEXT_BYTES,
        });
    }
    Ok(())
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
        BorrowedMcpePacketData::TextPacket(packet) => {
            // valentine does not emit a borrowed view for the `TextPacketBody`
            // union, so `TextPacketView::body` is the owned `TextPacketBody`:
            // its strings are already materialized (and therefore already valid
            // UTF-8) by the time this runs. `codec::validate_raw_text_packet`
            // is what rejects invalid UTF-8 and oversized parameter lists
            // straight off the raw frame; this arm re-checks the retained byte
            // and parameter budgets.
            match &packet.body {
                TextPacketBody::MessageOnly(payload) => {
                    bounded_borrowed_text(&payload.message)?;
                }
                TextPacketBody::AuthorAndMessage(payload) => {
                    bounded_borrowed_text(&payload.player_name)?;
                    bounded_borrowed_text(&payload.message)?;
                }
                TextPacketBody::MessageAndParams(payload) => {
                    bounded_borrowed_text(&payload.message)?;
                    if payload.parameter_list.len() > MAX_CHAT_PARAMETERS {
                        return Err(UiPacketError::TooManyChatParameters {
                            count: payload.parameter_list.len(),
                            max: MAX_CHAT_PARAMETERS,
                        });
                    }
                    for parameter in &payload.parameter_list {
                        bounded_borrowed_text(parameter)?;
                    }
                }
            }
            validate_utf8(&packet.senders_xuid, "text.xuid")?;
            validate_utf8(&packet.platform_id, "text.platform_chat_id")?;
            if let Some(filtered_message) = &packet.filtered_message {
                validate_utf8(filtered_message, "text.filtered_message")?;
            }
            Ok(())
        }
        BorrowedMcpePacketData::ModalFormRequestPacket(packet) => {
            if packet.form_uijson.as_bytes().len() > MAX_FORM_JSON_BYTES {
                return Err(UiPacketError::FormTooLarge {
                    bytes: packet.form_uijson.as_bytes().len(),
                    max: MAX_FORM_JSON_BYTES,
                });
            }
            packet
                .form_uijson
                .as_str()
                .map(|_| ())
                .map_err(|_| UiPacketError::InvalidUtf8 {
                    field: "modal_form.data",
                })
        }
        BorrowedMcpePacketData::SetTitlePacket(packet) => {
            validate_utf8(&packet.title_text, "set_title.text")?;
            validate_utf8(&packet.xuid, "set_title.xuid")?;
            validate_utf8(&packet.platform_online_id, "set_title.platform_online_id")?;
            validate_utf8(&packet.filtered_title_message, "set_title.filtered_message")
        }
        BorrowedMcpePacketData::BossEventPacket(packet) => {
            validate_utf8(&packet.name, "boss.title")?;
            validate_utf8(&packet.filtered_name, "boss.filtered_title")
        }
        BorrowedMcpePacketData::ToastRequestPacket(packet) => {
            validate_utf8(&packet.title, "toast.title")?;
            validate_utf8(&packet.content, "toast.message")
        }
        BorrowedMcpePacketData::RemoveObjectivePacket(packet) => {
            validate_utf8(&packet.objective_name, "objective.name")
        }
        BorrowedMcpePacketData::SetDisplayObjectivePacket(packet) => {
            validate_utf8(&packet.display_slot_name, "objective.display_slot")?;
            validate_utf8(&packet.objective_name, "objective.name")?;
            validate_utf8(&packet.objective_display_name, "objective.display_name")?;
            validate_utf8(&packet.criteria_name, "objective.criteria_name")
        }
        BorrowedMcpePacketData::UpdateSoftEnumPacket(packet) => {
            if packet.values.len() > MAX_CHAT_AUTOCOMPLETE {
                return Err(UiPacketError::TooManyAutocompleteSuggestions {
                    count: packet.values.len(),
                    max: MAX_CHAT_AUTOCOMPLETE,
                });
            }
            validate_utf8(&packet.enum_name, "soft_enum.name")?;
            let mut retained_bytes = packet.enum_name.as_bytes().len();
            for option in &packet.values {
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

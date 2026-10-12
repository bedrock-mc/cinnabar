use std::sync::Arc;

use bedrock_protocol_1_26_51::enums::{
    BossBarColor, BossBarOverlay, BossEventUpdateType, PlayStatusType, SoftEnumUpdateType,
    TextPacketType,
};
use bedrock_protocol_1_26_51::packets::{
    BossEvent as BossEventPacket, CommandOutput, CommandRequest, LevelEvent, PlayStatus,
    RemoveObjective, SetDisplayObjective, SetHealth, SetScore, Text, ToastRequest, UpdateSoftEnum,
};
use bedrock_protocol_1_26_51::types::{CommandOriginData, SetScoreInfoItem, TextData};
use bedrock_protocol_1_26_51::wire;
use thiserror::Error;

mod commands;
mod death;
mod forms;
mod text;
pub(crate) mod validation;

pub(crate) use commands::normalize_available_commands;
pub use commands::{
    ChatAutocompleteCatalog, ChatAutocompleteCatalogError, ChatAutocompleteCompletion,
    CommandParam, CommandParamKind, CommandSpec, CommandTreeEvent, CompletionContext,
};
pub(crate) use death::normalize_death_info;
pub use death::{DeathInfoEvent, DeathRules};

pub use forms::{
    CustomForm, CustomFormElement, CustomFormValue, ElementMenuForm, FormButtonImage, FormKind,
    FormNumber, FormRequestEvent, FormText, MAX_CUSTOM_FORM_ITEMS, MAX_FORM_JSON_DEPTH,
    MenuElement, ModalDialogForm, ModalFormResponseSelection, NPC_DIALOGUE_FORM_ID, NpcButton,
    NpcDialogueForm, NpcRequestKind, ServerFormModel, TextMenuForm, UnsupportedForm,
    custom_form_submit_response, modal_form_busy_response, modal_form_cancel_response,
    modal_form_submit_response, npc_request_packet, server_settings_request_packet,
};
pub(crate) use forms::{normalize_form, normalize_npc_dialogue, normalize_server_settings};
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
/// The codec's bounds on an authored Text packet's player name and sender XUID, which a test pins
/// to the generated encoder.
const MAX_CHAT_SOURCE_NAME_BYTES: usize = 256;
const MAX_CHAT_XUID_BYTES: usize = 64;

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
    for (field, value, max) in [
        ("source_name", source_name, MAX_CHAT_SOURCE_NAME_BYTES),
        ("xuid", xuid, MAX_CHAT_XUID_BYTES),
    ] {
        if value.len() > max {
            return Err(ChatPacketError::IdentityTooLong {
                field,
                bytes: value.len(),
                max,
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
    Ok(crate::Packet::new(Text {
        localize: false,
        body: TextData::AuthorAndMessage {
            message_type: TextPacketType::Chat,
            player_name: source_name.into(),
            message: message.into(),
        },
        sender_xuid: xuid.into(),
        platform_id: wire::Str::default(),
        filtered_message: None,
    }))
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

    Ok(command_request_packet(message))
}

/// A vanilla player-origin command request for already validated text.
pub fn command_request_packet(command: &str) -> crate::Packet {
    // The origin discriminant is a lowercase name string on this wire, not an
    // integer: gophertunnel's `commandOriginToString` maps
    // `CommandOriginPlayer` to exactly "player"
    // (`minecraft/protocol/command.go`).
    crate::Packet::new(CommandRequest {
        command: command.into(),
        origin: CommandOriginData {
            type_: "player".into(),
            uuid: uuid::Uuid::new_v4(),
            request_id: wire::Str::default(),
            player_id: wire::I64LE(0),
        },
        is_internal: false,
        version: "latest".into(),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    DeathInfo(DeathInfoEvent),
    ShowCredits(crate::ShowCreditsEvent),
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
    AvailableCommands(CommandTreeEvent),
    GameMode(GameModeEvent),
    /// UpdatePlayerGameType targets an actor's unique ID, not its runtime ID.
    /// The ordered world stream admits only its local player's update to the UI.
    PlayerGameMode {
        actor_unique_id: i64,
        tick: u64,
        event: GameModeEvent,
    },
    /// SetDefaultGameType: the level's default mode changed; players whose
    /// mode is bound to the default follow it.
    DefaultGameMode(GameModeEvent),
    GameRules {
        hud: HudRules,
        death: DeathRules,
    },
    /// The world's sleep status (LevelEventGeneric `SleepingPlayers`).
    SleepStatus(SleepStatusEvent),
}

/// The `SleepingPlayers` level event's NetworkLittleEndian NBT compound
/// (`sleepingPlayerCount`, `overworldPlayerCount`, `ableToSleep`), which the
/// receiver decodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepStatusEvent {
    pub nbt: std::sync::Arc<[u8]>,
}

/// The world rules that raise HUD text; `None` leaves a rule as it was.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HudRules {
    pub show_coordinates: Option<bool>,
    pub show_days_played: Option<bool>,
}

impl HudRules {
    pub const fn is_empty(self) -> bool {
        self.show_coordinates.is_none() && self.show_days_played.is_none()
    }
}

/// One wire game-mode value, retained without guessing.
///
/// `WorldDefault` is the explicit level-default sentinel (`GameMode 5`); the
/// receiver resolves it against its retained world default. `Unknown` keeps
/// the raw value so the receiver can count the skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameModeUpdate {
    Explicit(crate::PlayerGameMode),
    /// Legacy viewer modes share HUD capabilities but are not native spectators.
    LegacyViewer,
    WorldDefault,
    Unknown(i32),
}

impl GameModeUpdate {
    /// Resolves explicit HUD capabilities while preserving default and unknown sentinels.
    #[must_use]
    pub fn hud_mode(self) -> Option<crate::PlayerGameMode> {
        match self {
            Self::Explicit(mode) => Some(mode),
            Self::LegacyViewer => Some(crate::PlayerGameMode::Spectator),
            Self::WorldDefault | Self::Unknown(_) => None,
        }
    }
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
    UnsupportedEdition,
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
    /// A removal without an objective: the entry leaves every objective.
    RemoveFromAll,
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
    pub action: BossAction,
    pub title: Arc<str>,
    pub filtered_title: Arc<str>,
    pub progress: f32,
    pub style: BossStyle,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockCrackAction {
    Start { progress_per_tick: i32 },
    UpdateSpeed { progress_per_tick: i32 },
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
    #[error("server form JSON is malformed or is not a top-level object")]
    InvalidFormJson,
    #[error("server form JSON nests {depth} levels, exceeding the maximum depth {max}")]
    FormJsonDepthExceeded { depth: usize, max: usize },
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
    #[error("UI packet value is outside its protocol schema: {reason}")]
    SchemaViolation { reason: &'static str },
}

/// Retains a wire string as text, decoding invalid UTF-8 lossily, within [`MAX_UI_TEXT_BYTES`].
fn bounded_text(value: wire::Str) -> Result<Arc<str>, UiPacketError> {
    let value = value.to_string_lossy();
    if value.len() > MAX_UI_TEXT_BYTES {
        return Err(UiPacketError::TextTooLong {
            bytes: value.len(),
            max: MAX_UI_TEXT_BYTES,
        });
    }
    Ok(Arc::from(value))
}

pub(crate) fn normalize_command_output(packet: CommandOutput) -> Result<UiEvent, UiPacketError> {
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
        success_count: output.success_count.0,
        messages: Arc::from(messages),
        data: output.data_set.map(bounded_text).transpose()?,
    }))
}

pub(crate) fn normalize_toast(packet: ToastRequest) -> Result<UiEvent, UiPacketError> {
    Ok(UiEvent::Hud(HudEvent::Toast {
        title: bounded_text(packet.title)?,
        message: bounded_text(packet.content)?,
    }))
}

pub(crate) fn normalize_health(packet: SetHealth) -> UiEvent {
    UiEvent::Hud(HudEvent::Health {
        health: packet.health.0,
    })
}

pub(crate) fn normalize_player_status(packet: PlayStatus) -> Result<UiEvent, UiPacketError> {
    // Map the reserved edition-mismatch statuses to one neutral local state.
    let status = match packet.status {
        PlayStatusType::Loginsuccess => PlayerStatus::LoginSuccess,
        PlayStatusType::LoginfailedClientold => PlayerStatus::FailedClient,
        PlayStatusType::LoginfailedServerold => PlayerStatus::FailedSpawn,
        PlayStatusType::Playerspawn => PlayerStatus::PlayerSpawn,
        PlayStatusType::LoginfailedInvalidtenant
        | PlayStatusType::LoginfailedEditionmismatchedutovanilla
        | PlayStatusType::LoginfailedEditionmismatchvanillatoedu => {
            PlayerStatus::UnsupportedEdition
        }
        PlayStatusType::LoginfailedServerfullsubclient => PlayerStatus::FailedServerFull,
        PlayStatusType::LoginfailedEditormismatcheditortovanilla => {
            PlayerStatus::FailedEditorVanillaMismatch
        }
        PlayStatusType::LoginfailedEditormismatchvanillatoeditor => {
            PlayerStatus::FailedVanillaEditorMismatch
        }
        PlayStatusType::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "player status",
                value: i64::from(value),
            });
        }
    };
    Ok(UiEvent::Hud(HudEvent::PlayerStatus(status)))
}

pub(crate) fn normalize_display_objective(
    packet: SetDisplayObjective,
) -> Result<UiEvent, UiPacketError> {
    Ok(UiEvent::Objective(ObjectiveEvent::Display {
        display_slot: bounded_text(packet.display_slot_name)?,
        objective_name: bounded_text(packet.objective_name)?,
        display_name: bounded_text(packet.objective_display_name)?,
        criteria_name: bounded_text(packet.criteria_name)?,
        sort_order: packet.sort_order.0,
    }))
}

pub(crate) fn normalize_remove_objective(
    packet: RemoveObjective,
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
pub(crate) fn normalize_score(packet: SetScore) -> Result<UiEvent, UiPacketError> {
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
            SetScoreInfoItem::RemoveScore {
                scoreboard_id,
                objective_name,
                ..
            } => {
                // A removal carries an optional objective name and nothing
                // else, so there is no score or identity to report.
                Ok(ScoreEntry {
                    action: match objective_name {
                        Some(_) => ScoreAction::Remove,
                        None => ScoreAction::RemoveFromAll,
                    },
                    scoreboard_id: scoreboard_id.0,
                    objective_name: bounded_text(objective_name.unwrap_or_default())?,
                    score: 0,
                    identity: ScoreIdentity::None,
                })
            }
            SetScoreInfoItem::ChangePlayerScore {
                scoreboard_id,
                objective_name,
                score_value,
                player_unique_id,
                ..
            } => Ok(ScoreEntry {
                action: ScoreAction::Change,
                scoreboard_id: scoreboard_id.0,
                objective_name: bounded_text(objective_name)?,
                score: score_value.0,
                identity: ScoreIdentity::Player(player_unique_id.0),
            }),
            SetScoreInfoItem::ChangeEntityScore {
                scoreboard_id,
                objective_name,
                score_value,
                actor_id,
                ..
            } => Ok(ScoreEntry {
                action: ScoreAction::Change,
                scoreboard_id: scoreboard_id.0,
                objective_name: bounded_text(objective_name)?,
                score: score_value.0,
                identity: ScoreIdentity::Entity(actor_id.0),
            }),
            SetScoreInfoItem::ChangeFakePlayerScore {
                scoreboard_id,
                objective_name,
                score_value,
                fake_player_name,
                ..
            } => Ok(ScoreEntry {
                action: ScoreAction::Change,
                scoreboard_id: scoreboard_id.0,
                objective_name: bounded_text(objective_name)?,
                score: score_value.0,
                identity: ScoreIdentity::FakePlayer(bounded_text(fake_player_name)?),
            }),
        })
        .collect::<Result<Vec<_>, UiPacketError>>()?;
    Ok(UiEvent::Score(ScoreEvent {
        entries: Arc::from(entries),
    }))
}

pub(crate) fn normalize_boss(packet: BossEventPacket) -> Result<UiEvent, UiPacketError> {
    // Removal consumes only identity. Unused presentation fields must not
    // prevent the client from retiring a tracked boss.
    if packet.event_type == BossEventUpdateType::Remove {
        return Ok(UiEvent::Boss(BossEvent {
            target_entity_id: packet.target_actor_id.0,
            action: BossAction::Hide,
            title: Arc::from(""),
            filtered_title: Arc::from(""),
            progress: 0.0,
            style: BossStyle {
                color: BossColor::Pink,
                overlay: BossOverlay::Progress,
                darken_sky: None,
                create_world_fog: None,
            },
        }));
    }
    let health_percent = packet.health_percent.0;
    if !health_percent.is_finite() {
        return Err(UiPacketError::NonFiniteBossProgress {
            bits: health_percent.to_bits(),
        });
    }
    // 1.26.40 renamed the event verbs to the Mojang spellings; the wire values
    // are unchanged and still line up 0..=8 with gophertunnel's `BossEvent*`
    // constants (`minecraft/protocol/packet/boss_event.go`), so `UpdateStyle`
    // is value 7, the one gophertunnel calls `BossEventTexture`.
    let action = match packet.event_type {
        BossEventUpdateType::Add => BossAction::Show,
        BossEventUpdateType::Playeradded => BossAction::RegisterPlayer,
        BossEventUpdateType::Remove => BossAction::Hide,
        BossEventUpdateType::Playerremoved => BossAction::UnregisterPlayer,
        BossEventUpdateType::UpdatePercent => BossAction::SetProgress,
        BossEventUpdateType::UpdateName => BossAction::SetTitle,
        BossEventUpdateType::UpdateProperties => BossAction::UpdateProperties,
        BossEventUpdateType::UpdateStyle => BossAction::Texture,
        BossEventUpdateType::Query => BossAction::Query,
        BossEventUpdateType::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "boss action",
                value: i64::from(value),
            });
        }
    };
    let color = match packet.color {
        BossBarColor::Pink => BossColor::Pink,
        BossBarColor::Blue => BossColor::Blue,
        BossBarColor::Red => BossColor::Red,
        BossBarColor::Green => BossColor::Green,
        BossBarColor::Yellow => BossColor::Yellow,
        BossBarColor::Purple => BossColor::Purple,
        BossBarColor::RebeccaPurple => BossColor::RebeccaPurple,
        BossBarColor::White => BossColor::White,
        BossBarColor::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "boss color",
                value: i64::from(value),
            });
        }
    };
    let overlay = match packet.overlay {
        BossBarOverlay::Progress => BossOverlay::Progress,
        BossBarOverlay::Notched6 => BossOverlay::Notched6,
        BossBarOverlay::Notched10 => BossOverlay::Notched10,
        BossBarOverlay::Notched12 => BossOverlay::Notched12,
        BossBarOverlay::Notched20 => BossOverlay::Notched20,
        BossBarOverlay::Unknown(value) => {
            return Err(UiPacketError::UnknownEnum {
                kind: "boss overlay",
                value: i64::from(value),
            });
        }
    };
    Ok(UiEvent::Boss(BossEvent {
        target_entity_id: packet.target_actor_id.0,
        action,
        title: bounded_text(packet.name)?,
        filtered_title: bounded_text(packet.filtered_name)?,
        progress: health_percent,
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

/// Normalizes the server soft-enum delta used by local command completion.
/// Request identities belong to the local editor; this wire packet carries no request ID.
pub(crate) fn normalize_soft_enum(packet: UpdateSoftEnum) -> Result<UiEvent, UiPacketError> {
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
        SoftEnumUpdateType::Add => ChatAutocompleteAction::Add,
        SoftEnumUpdateType::Remove => ChatAutocompleteAction::Remove,
        SoftEnumUpdateType::Replace => ChatAutocompleteAction::Replace,
        SoftEnumUpdateType::Unknown(value) => {
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

/// Normalizes block cracking without inventing a stage or actor ID.
///
/// The wire `data` field is the server-authored progress rate (`65535 / break_ticks`).
/// Presentation advances progress on rendered frames; normalization preserves
/// the signed wire rate.
pub(crate) fn normalize_block_crack(packet: LevelEvent) -> Result<BlockCrackEvent, UiPacketError> {
    let position = [
        floored_block_coordinate(packet.position.x, "x")?,
        floored_block_coordinate(packet.position.y, "y")?,
        floored_block_coordinate(packet.position.z, "z")?,
    ];
    let event_id = packet.event_id.0;
    let action = match event_id {
        LEVEL_EVENT_STOP_BLOCK_CRACKING => BlockCrackAction::Stop,
        LEVEL_EVENT_START_BLOCK_CRACKING | LEVEL_EVENT_UPDATE_BLOCK_CRACKING => {
            let progress_per_tick = packet.data.0;
            if event_id == LEVEL_EVENT_START_BLOCK_CRACKING {
                BlockCrackAction::Start { progress_per_tick }
            } else {
                BlockCrackAction::UpdateSpeed { progress_per_tick }
            }
        }
        _ => unreachable!("caller only dispatches block crack level events"),
    };
    Ok(BlockCrackEvent { position, action })
}

/// Floors a coordinate as vanilla's level-event handling does.
fn floored_block_coordinate(value: f32, field: &'static str) -> Result<i32, UiPacketError> {
    let error = UiPacketError::InvalidBlockCrackPosition {
        field,
        bits: value.to_bits(),
    };
    if !value.is_finite() {
        return Err(error);
    }
    i32::try_from(value.floor() as i64).map_err(|_| error)
}

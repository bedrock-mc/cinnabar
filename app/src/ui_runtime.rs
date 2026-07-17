//! App-owned conversion boundary between retained UI output and render POD.

pub mod presentation;
pub mod render_adapter;

use std::{collections::VecDeque, sync::Arc};

use bevy::prelude::Resource;
use protocol::{
    ActorAttribute, BlockCrackEvent, HudEvent, PlayerStatus, TextEvent, TextKind, TitleAction,
    TitleEvent, UiEvent,
};
use semantic_input::InputContext;
use ui::{
    BoundedStat, ChatApplyResult, ChatMessage, ChatMessageKind, ChatStore, HudPlayerStatus,
    HudStore, TitleDurations, Toast,
};

pub const MAX_PENDING_BLOCK_CRACK_EVENTS: usize = 1_024;

#[derive(Clone, Debug)]
pub struct SequencedUiEvent {
    pub session_id: u64,
    pub fifo_sequence: u64,
    pub local_millis: u64,
    pub server_tick: Option<u64>,
    pub event: UiEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequencedBlockCrackEvent {
    pub session_id: u64,
    pub fifo_sequence: u64,
    pub dimension: i32,
    pub event: BlockCrackEvent,
}

#[derive(Clone, Debug)]
pub struct SequencedLocalAttributes {
    pub session_id: u64,
    pub fifo_sequence: u64,
    pub local_millis: u64,
    pub server_tick: u64,
    pub attributes: Arc<[ActorAttribute]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiApplyOutcome {
    Applied,
    IgnoredByReceiveStore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiRuntimeError {
    WrongSession { expected: u64, actual: u64 },
    StaleFifoSequence { previous: u64, actual: u64 },
    StaleBlockCrackSequence { previous: u64, actual: u64 },
    BlockCrackQueueFull { maximum: usize },
    NonMonotonicLocalTime { previous: u64, actual: u64 },
    NonMonotonicServerTick { previous: u64, actual: u64 },
    InvalidTitleDurations,
    InvalidHealth(i32),
    InvalidLocalAttribute { field: &'static str },
    ChatRejected(ChatApplyResult),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiAuthorityTransition {
    consumes_text: bool,
    requested_context: InputContext,
}

impl UiAuthorityTransition {
    pub const fn ui_consumed_text(self) -> bool {
        self.consumes_text
    }

    pub const fn requested_input_context(self) -> InputContext {
        self.requested_context
    }
}

#[derive(Clone, Debug, Resource)]
pub struct UiRuntime {
    session_id: u64,
    last_fifo_sequence: Option<u64>,
    last_block_crack_sequence: Option<u64>,
    last_local_millis: Option<u64>,
    last_server_tick: Option<u64>,
    chat_focused: bool,
    hud: HudStore,
    chat: ChatStore,
    pending_block_cracks: VecDeque<SequencedBlockCrackEvent>,
}

impl UiRuntime {
    pub fn new(session_id: u64) -> Self {
        Self {
            session_id,
            last_fifo_sequence: None,
            last_block_crack_sequence: None,
            last_local_millis: None,
            last_server_tick: None,
            chat_focused: false,
            hud: HudStore::default(),
            chat: ChatStore::default(),
            pending_block_cracks: VecDeque::with_capacity(MAX_PENDING_BLOCK_CRACK_EVENTS),
        }
    }

    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    pub const fn hud(&self) -> &HudStore {
        &self.hud
    }

    pub const fn chat(&self) -> &ChatStore {
        &self.chat
    }

    pub const fn chat_focused(&self) -> bool {
        self.chat_focused
    }

    pub const fn pending_block_cracks(&self) -> &VecDeque<SequencedBlockCrackEvent> {
        &self.pending_block_cracks
    }

    pub fn take_block_cracks(&mut self) -> Vec<SequencedBlockCrackEvent> {
        self.pending_block_cracks.drain(..).collect()
    }

    pub fn begin_session(&mut self, session_id: u64) {
        if self.session_id == session_id {
            return;
        }
        self.session_id = session_id;
        self.last_fifo_sequence = None;
        self.last_block_crack_sequence = None;
        self.last_local_millis = None;
        self.last_server_tick = None;
        self.chat_focused = false;
        self.hud.clear();
        self.chat.clear();
        self.pending_block_cracks.clear();
    }

    pub fn open_chat(&mut self) -> UiAuthorityTransition {
        self.chat_focused = true;
        UiAuthorityTransition {
            consumes_text: true,
            requested_context: InputContext::UiFocused,
        }
    }

    pub fn close_chat(&mut self) -> UiAuthorityTransition {
        self.chat_focused = false;
        UiAuthorityTransition {
            consumes_text: false,
            requested_context: InputContext::Gameplay,
        }
    }

    pub fn apply(&mut self, envelope: SequencedUiEvent) -> Result<UiApplyOutcome, UiRuntimeError> {
        self.validate_identity(
            envelope.session_id,
            envelope.fifo_sequence,
            envelope.local_millis,
            envelope.server_tick,
        )?;
        let event_millis = envelope
            .server_tick
            .map_or(envelope.local_millis, |tick| tick.saturating_mul(50));
        let outcome = match envelope.event {
            UiEvent::Text(event) => self.apply_text(event, envelope.fifo_sequence, event_millis)?,
            UiEvent::Title(event) => {
                self.apply_title(event, envelope.fifo_sequence, event_millis)?;
                UiApplyOutcome::Applied
            }
            UiEvent::Hud(event) => {
                self.apply_hud(event, envelope.fifo_sequence, event_millis)?;
                UiApplyOutcome::Applied
            }
            UiEvent::Objective(_)
            | UiEvent::Score(_)
            | UiEvent::Boss(_)
            | UiEvent::Form(_)
            | UiEvent::ChatAutocomplete(_) => UiApplyOutcome::IgnoredByReceiveStore,
        };
        self.last_fifo_sequence = Some(envelope.fifo_sequence);
        self.last_local_millis = Some(envelope.local_millis);
        if let Some(server_tick) = envelope.server_tick {
            self.last_server_tick = Some(server_tick);
        }
        Ok(outcome)
    }

    pub fn retain_block_crack(
        &mut self,
        envelope: SequencedBlockCrackEvent,
    ) -> Result<(), UiRuntimeError> {
        if envelope.session_id != self.session_id {
            return Err(UiRuntimeError::WrongSession {
                expected: self.session_id,
                actual: envelope.session_id,
            });
        }
        if let Some(previous) = self.last_block_crack_sequence
            && envelope.fifo_sequence <= previous
        {
            return Err(UiRuntimeError::StaleBlockCrackSequence {
                previous,
                actual: envelope.fifo_sequence,
            });
        }
        if self.pending_block_cracks.len() >= MAX_PENDING_BLOCK_CRACK_EVENTS {
            return Err(UiRuntimeError::BlockCrackQueueFull {
                maximum: MAX_PENDING_BLOCK_CRACK_EVENTS,
            });
        }
        self.last_block_crack_sequence = Some(envelope.fifo_sequence);
        self.pending_block_cracks.push_back(envelope);
        Ok(())
    }

    pub fn apply_local_attributes(
        &mut self,
        envelope: SequencedLocalAttributes,
    ) -> Result<(), UiRuntimeError> {
        self.validate_identity(
            envelope.session_id,
            envelope.fifo_sequence,
            envelope.local_millis,
            Some(envelope.server_tick),
        )?;
        let mut health = self.hud.health();
        let mut hunger = self.hud.hunger();
        for attribute in envelope.attributes.iter() {
            match attribute.name.as_ref() {
                "minecraft:health" => {
                    health = Some(
                        attribute_stat(attribute)
                            .ok_or(UiRuntimeError::InvalidLocalAttribute { field: "health" })?,
                    );
                }
                "minecraft:player.hunger" => {
                    hunger = Some(
                        attribute_stat(attribute)
                            .ok_or(UiRuntimeError::InvalidLocalAttribute { field: "hunger" })?,
                    );
                }
                _ => {}
            }
        }
        self.hud
            .set_stats(health, hunger, self.hud.armor(), self.hud.air());
        self.last_fifo_sequence = Some(envelope.fifo_sequence);
        self.last_local_millis = Some(envelope.local_millis);
        self.last_server_tick = Some(envelope.server_tick);
        Ok(())
    }

    fn validate_identity(
        &self,
        session_id: u64,
        fifo_sequence: u64,
        local_millis: u64,
        server_tick: Option<u64>,
    ) -> Result<(), UiRuntimeError> {
        if session_id != self.session_id {
            return Err(UiRuntimeError::WrongSession {
                expected: self.session_id,
                actual: session_id,
            });
        }
        if let Some(previous) = self.last_fifo_sequence
            && fifo_sequence <= previous
        {
            return Err(UiRuntimeError::StaleFifoSequence {
                previous,
                actual: fifo_sequence,
            });
        }
        if let Some(previous) = self.last_local_millis
            && local_millis < previous
        {
            return Err(UiRuntimeError::NonMonotonicLocalTime {
                previous,
                actual: local_millis,
            });
        }
        if let (Some(previous), Some(actual)) = (self.last_server_tick, server_tick)
            && actual < previous
        {
            return Err(UiRuntimeError::NonMonotonicServerTick { previous, actual });
        }
        Ok(())
    }

    fn apply_text(
        &mut self,
        event: TextEvent,
        fifo_sequence: u64,
        event_millis: u64,
    ) -> Result<UiApplyOutcome, UiRuntimeError> {
        if matches!(
            event.kind,
            TextKind::Popup | TextKind::JukeboxPopup | TextKind::Tip
        ) {
            self.hud
                .set_actionbar(event.message, fifo_sequence, event_millis);
            return Ok(UiApplyOutcome::Applied);
        }
        let kind = match event.kind {
            TextKind::Chat => ChatMessageKind::Chat,
            TextKind::Whisper | TextKind::JsonWhisper => ChatMessageKind::Whisper,
            TextKind::Announcement | TextKind::JsonAnnouncement => ChatMessageKind::Announcement,
            TextKind::Translation => ChatMessageKind::Translation,
            TextKind::Raw | TextKind::System | TextKind::Json => ChatMessageKind::System,
            TextKind::Popup | TextKind::JukeboxPopup | TextKind::Tip => unreachable!(),
        };
        match self.chat.push(ChatMessage {
            fifo_sequence,
            received_millis: event_millis,
            kind,
            source: event.source,
            message: event.message,
            parameters: event.parameters,
        }) {
            ChatApplyResult::Applied { .. } => Ok(UiApplyOutcome::Applied),
            result => Err(UiRuntimeError::ChatRejected(result)),
        }
    }

    fn apply_title(
        &mut self,
        event: TitleEvent,
        fifo_sequence: u64,
        event_millis: u64,
    ) -> Result<(), UiRuntimeError> {
        match event.action {
            TitleAction::Clear => self.hud.clear_titles(),
            TitleAction::Reset => self.hud.reset_titles(),
            TitleAction::SetTitle | TitleAction::SetTitleJson => {
                self.hud.set_title(event.text, fifo_sequence, event_millis);
            }
            TitleAction::SetSubtitle | TitleAction::SetSubtitleJson => {
                self.hud
                    .set_subtitle(event.text, fifo_sequence, event_millis);
            }
            TitleAction::ActionBar | TitleAction::ActionBarJson => {
                self.hud
                    .set_actionbar(event.text, fifo_sequence, event_millis);
            }
            TitleAction::SetDurations => {
                let durations = TitleDurations::from_wire(
                    event.fade_in_ticks,
                    event.stay_ticks,
                    event.fade_out_ticks,
                )
                .ok_or(UiRuntimeError::InvalidTitleDurations)?;
                self.hud.set_durations(durations);
            }
        }
        Ok(())
    }

    fn apply_hud(
        &mut self,
        event: HudEvent,
        fifo_sequence: u64,
        event_millis: u64,
    ) -> Result<(), UiRuntimeError> {
        match event {
            HudEvent::Toast { title, message } => {
                self.hud.push_toast(Toast {
                    title,
                    message,
                    fifo_sequence,
                    received_millis: event_millis,
                });
            }
            HudEvent::Health { health } => {
                let health =
                    u16::try_from(health).map_err(|_| UiRuntimeError::InvalidHealth(health))?;
                let maximum = health.max(20);
                self.hud.set_health(BoundedStat::new(health, maximum));
            }
            HudEvent::PlayerStatus(status) => {
                self.hud.set_player_status(map_player_status(status));
            }
        }
        Ok(())
    }
}

fn attribute_stat(attribute: &ActorAttribute) -> Option<BoundedStat> {
    if !attribute.current.is_finite()
        || !attribute.max.is_finite()
        || attribute.max <= 0.0
        || attribute.current < 0.0
        || attribute.current > attribute.max
    {
        return None;
    }
    let scale = if attribute.max <= u16::MAX as f32 / 100.0 {
        100.0
    } else {
        1.0
    };
    let maximum = u16::try_from((attribute.max * scale).round() as u32).ok()?;
    let current = u16::try_from((attribute.current * scale).round() as u32).ok()?;
    BoundedStat::new_scaled(current, maximum, scale as u16)
}

fn map_player_status(status: PlayerStatus) -> HudPlayerStatus {
    match status {
        PlayerStatus::LoginSuccess => HudPlayerStatus::LoginSuccess,
        PlayerStatus::FailedClient => HudPlayerStatus::FailedClient,
        PlayerStatus::FailedSpawn => HudPlayerStatus::FailedSpawn,
        PlayerStatus::PlayerSpawn => HudPlayerStatus::PlayerSpawn,
        PlayerStatus::FailedInvalidTenant => HudPlayerStatus::FailedInvalidTenant,
        PlayerStatus::FailedVanillaEducation => HudPlayerStatus::FailedVanillaEducation,
        PlayerStatus::FailedEducationVanilla => HudPlayerStatus::FailedEducationVanilla,
        PlayerStatus::FailedServerFull => HudPlayerStatus::FailedServerFull,
        PlayerStatus::FailedEditorVanillaMismatch => HudPlayerStatus::FailedEditorVanillaMismatch,
        PlayerStatus::FailedVanillaEditorMismatch => HudPlayerStatus::FailedVanillaEditorMismatch,
    }
}

#[cfg(test)]
mod tests;

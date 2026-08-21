//! Application of decoded text, title, command-output, and HUD events.

use std::sync::Arc;

use protocol::{CommandOutputEvent, HudEvent, TextEvent, TextKind, TitleAction, TitleEvent};
use ui::{BoundedStat, ChatApplyResult, ChatMessage, ChatMessageKind, TitleDurations, Toast};

use super::{UiApplyOutcome, UiRuntime, UiRuntimeError, hud_adapter};

impl UiRuntime {
    pub(super) fn apply_text(
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
            // An oversized server row is odd data, not a wire fault: skip the
            // whole row, count it, keep the session alive.
            ChatApplyResult::RejectedTooLarge => {
                self.gameplay_hud.note_oversized_chat_row();
                Ok(UiApplyOutcome::IgnoredByReceiveStore)
            }
            result => Err(UiRuntimeError::ChatRejected(result)),
        }
    }

    pub(super) fn apply_command_output(
        &mut self,
        event: CommandOutputEvent,
        fifo_sequence: u64,
        event_millis: u64,
    ) -> Result<UiApplyOutcome, UiRuntimeError> {
        let messages = event
            .messages
            .iter()
            .map(|message| ChatMessage {
                fifo_sequence,
                received_millis: event_millis,
                kind: ChatMessageKind::Translation,
                source: None,
                message: Arc::clone(&message.message_id),
                parameters: Arc::clone(&message.parameters),
            })
            .collect();
        match self.chat.push_batch(messages) {
            ChatApplyResult::Applied { .. } => Ok(UiApplyOutcome::Applied),
            ChatApplyResult::RejectedTooLarge => {
                self.gameplay_hud.note_oversized_chat_row();
                Ok(UiApplyOutcome::IgnoredByReceiveStore)
            }
            result => Err(UiRuntimeError::ChatRejected(result)),
        }
    }

    pub(super) fn apply_title(
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
                // Negative tick counts are semantically odd but well-formed:
                // keep the previous durations and count the skip.
                match TitleDurations::from_wire(
                    event.fade_in_ticks,
                    event.stay_ticks,
                    event.fade_out_ticks,
                ) {
                    Some(durations) => self.hud.set_durations(durations),
                    None => self.gameplay_hud.note_odd_hud_packet(),
                }
            }
        }
        Ok(())
    }

    pub(super) fn apply_hud(
        &mut self,
        event: HudEvent,
        fifo_sequence: u64,
        event_millis: u64,
    ) -> Result<(), UiRuntimeError> {
        match event {
            HudEvent::Toast { title, message } => {
                self.hud
                    .push_toast(Toast::new(title, message, fifo_sequence, event_millis));
            }
            HudEvent::Health { health } => {
                // A negative or overflowing SetHealth is semantically odd but
                // well-formed wire: skip it, count it, keep the session alive.
                match u16::try_from(health) {
                    Ok(health) => {
                        let maximum = health.max(20);
                        self.hud.set_health(BoundedStat::new(health, maximum));
                    }
                    Err(_) => self.gameplay_hud.note_odd_hud_packet(),
                }
            }
            HudEvent::PlayerStatus(status) => {
                self.hud
                    .set_player_status(hud_adapter::player_status(status));
            }
        }
        Ok(())
    }
}

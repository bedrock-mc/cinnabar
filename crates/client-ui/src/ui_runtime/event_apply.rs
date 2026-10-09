//! Application of decoded text, title, command-output, and HUD events.

use std::sync::Arc;

use protocol::{
    CommandOutputEvent, HudEvent, TextEvent, TextKind, TitleAction, TitleEvent, UiEvent,
};
use ui::{BoundedStat, ChatApplyResult, ChatMessage, ChatMessageKind, TitleDurations, Toast};

use super::{
    SequencedUiEvent, UiApplyOutcome, UiRuntime, UiRuntimeError, hud_adapter, scoreboard_adapter,
};

impl UiRuntime {
    /// Applies one session- and sequence-validated UI event.
    pub fn apply(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        envelope: SequencedUiEvent,
    ) -> Result<UiApplyOutcome, UiRuntimeError> {
        self.validate_identity(
            envelope.session_id,
            envelope.fifo_sequence,
            envelope.local_millis,
            envelope.server_tick,
        )?;
        let timed_event = matches!(
            envelope.event,
            UiEvent::Text(_)
                | UiEvent::CommandOutput(_)
                | UiEvent::RawText(_)
                | UiEvent::Title(_)
                | UiEvent::Hud(_)
        );
        if timed_event && envelope.server_tick.is_some() {
            return Err(UiRuntimeError::TimedEventRequiresLocalClock {
                fifo_sequence: envelope.fifo_sequence,
            });
        }
        let event_millis = envelope.local_millis;
        let outcome = match envelope.event {
            UiEvent::DeathInfo(event) => {
                self.apply_death_info(event);
                UiApplyOutcome::Applied
            }
            UiEvent::Text(event) => self.apply_text(event, envelope.fifo_sequence, event_millis)?,
            UiEvent::CommandOutput(event) => {
                self.apply_command_output(event, envelope.fifo_sequence, event_millis)?
            }
            UiEvent::RawText(event) if event.document.has_unresolved_components() => {
                // Score/selector/translation components resolve against the
                // retained authoritative state; every degradation is counted
                // and presented per the vanilla rules, never as JSON.
                let resolved = self.resolve_raw_text(&event.document);
                let mut text = event.text;
                text.message = Arc::from(resolved.text);
                self.apply_resolved_text(text, envelope.fifo_sequence, event_millis)?
            }
            UiEvent::RawText(event) => {
                self.apply_resolved_text(event.text, envelope.fifo_sequence, event_millis)?
            }
            UiEvent::Title(mut event)
                if event
                    .document
                    .as_ref()
                    .is_some_and(|document| document.has_unresolved_components()) =>
            {
                let document = event.document.clone().expect("guard checked the document");
                let resolved = self.resolve_raw_text(&document);
                event.text = Arc::from(resolved.text);
                self.apply_title(event, envelope.fifo_sequence, event_millis)?;
                UiApplyOutcome::Applied
            }
            UiEvent::Title(event) => {
                self.apply_title(event, envelope.fifo_sequence, event_millis)?;
                UiApplyOutcome::Applied
            }
            UiEvent::Hud(event) => {
                self.apply_hud(event, envelope.fifo_sequence, event_millis)?;
                UiApplyOutcome::Applied
            }
            UiEvent::ChatAutocomplete(event) => {
                self.chat_autocomplete_catalog
                    .apply(event)
                    .map_err(UiRuntimeError::ChatAutocompleteCatalog)?;
                UiApplyOutcome::Applied
            }
            UiEvent::AvailableCommands(event) => {
                self.chat_autocomplete_catalog.apply_commands(event);
                UiApplyOutcome::Applied
            }
            UiEvent::Objective(event) => scoreboard_adapter::apply_outcome(
                self.scoreboards
                    .apply(envelope.fifo_sequence, scoreboard_adapter::objective(event))
                    .map_err(UiRuntimeError::RetainedUiSequence)?,
            ),
            UiEvent::Score(event) => scoreboard_adapter::apply_outcome(
                self.scoreboards
                    .apply(envelope.fifo_sequence, scoreboard_adapter::score(event))
                    .map_err(UiRuntimeError::RetainedUiSequence)?,
            ),
            UiEvent::Boss(event) => self.apply_boss(envelope.fifo_sequence, event)?,
            UiEvent::GameMode(event) => self.apply_game_mode_update(player_runtime, event.update),
            // Targeted mode updates must pass the world stream's local-unique-ID
            // admission first; a direct UI injection cannot establish that identity.
            UiEvent::PlayerGameMode { .. } => {
                self.gameplay_hud.note_odd_hud_packet();
                UiApplyOutcome::IgnoredByReceiveStore
            }
            UiEvent::DefaultGameMode(event) => {
                self.apply_default_game_mode_update(player_runtime, event.update)
            }
            UiEvent::GameRules { hud, death } => {
                self.apply_hud_rules(hud);
                self.apply_death_rules(death);
                UiApplyOutcome::Applied
            }
            UiEvent::SleepStatus(event) => self.apply_sleep_status(&event),
            UiEvent::ShowCredits(event) => {
                self.credits
                    .open(event.runtime_id, envelope.fifo_sequence, event_millis);
                UiApplyOutcome::Applied
            }
            UiEvent::Form(event) => {
                bevy::log::info!(
                    target: "server_form",
                    form_id = event.form_id,
                    kind = ?event.kind,
                    title = ?event.title,
                    "form received"
                );
                self.forms.admit(
                    event,
                    envelope.fifo_sequence,
                    self.session_id,
                    self.chat_focused || self.inventory_open,
                );
                UiApplyOutcome::Applied
            }
        };
        self.last_fifo_sequence = Some(envelope.fifo_sequence);
        self.last_local_millis = Some(envelope.local_millis);
        if let Some(server_tick) = envelope.server_tick {
            self.last_server_tick = Some(server_tick);
            self.observe_presentation_tick(server_tick, envelope.local_millis);
        }
        Ok(outcome)
    }
    pub(super) fn apply_text(
        &mut self,
        mut event: TextEvent,
        fifo_sequence: u64,
        event_millis: u64,
    ) -> Result<UiApplyOutcome, UiRuntimeError> {
        if event.needs_translation && event.kind != TextKind::Translation {
            let translate = |key: &str| self.translation(key);
            let template = json_ui::localize_text(&event.message, &translate);
            let parameters = event
                .parameters
                .iter()
                .map(|parameter| {
                    protocol::localize_parameter_prefix(parameter, &translate, usize::MAX)
                        .into_owned()
                })
                .collect::<Vec<_>>();
            event.message = Arc::from(protocol::format_translation(&template, &parameters));
            event.parameters = Arc::from([]);
        }
        self.apply_resolved_text(event, fifo_sequence, event_millis)
    }

    /// Routes resolved component text without repeating packet localization.
    pub(super) fn apply_resolved_text(
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

    /// Adds a client-generated system line that consumes no server sequence.
    pub fn push_local_chat_line(&mut self, message: Arc<str>, now_millis: u64) {
        let _ = self.chat.push_local(ChatMessage {
            fifo_sequence: 0,
            received_millis: now_millis,
            kind: ChatMessageKind::System,
            source: None,
            message,
            parameters: Arc::from([]),
        });
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
                let mut toast = Toast::new(title, message, fifo_sequence, event_millis);
                toast.expires_millis = event_millis
                    .saturating_add(self.toast_display_millis)
                    .saturating_add(ui::TOAST_SLIDE_OUT_MILLIS);
                self.hud.push_toast(toast);
            }
            HudEvent::Health { health } => {
                // A negative or overflowing SetHealth is semantically odd but
                // well-formed wire: skip it, count it, keep the session alive.
                match u16::try_from(health) {
                    Ok(health) => {
                        let maximum = health.max(20);
                        self.publish_local_player_alive(health > 0);
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

    /// Stands a client toast that stays while its cause lasts.
    pub fn stand_toast(&mut self, toast: ui::StandingToast) {
        self.hud.stand_toast(toast);
    }

    /// Slides the standing toast out from `now_millis`.
    pub fn retire_standing_toast(&mut self, now_millis: u64) {
        self.hud.retire_standing_toast(now_millis);
    }
}

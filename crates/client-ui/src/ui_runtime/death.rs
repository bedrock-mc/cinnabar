//! Retains the localized local-player reason until authoritative recovery.

use std::sync::Arc;

use super::{UiRuntime, UiRuntimeError};

impl UiRuntime {
    /// The latest server-authored reason; packets may precede or follow zero health.
    pub fn death_reason(&self) -> &str {
        if self
            .death_rules
            .show_messages
            .unwrap_or(protocol::DeathRules::DEFAULT_SHOW_MESSAGES)
        {
            &self.death_reason
        } else {
            ""
        }
    }

    /// Whether the server requests respawn without showing the death controls.
    pub fn immediate_respawn(&self) -> bool {
        self.death_rules
            .immediate_respawn
            .unwrap_or(protocol::DeathRules::DEFAULT_IMMEDIATE_RESPAWN)
    }

    /// Applies only supplied values, preserving unrelated incremental updates.
    pub fn apply_death_rules(&mut self, rules: protocol::DeathRules) {
        if rules.show_messages.is_some() {
            self.death_rules.show_messages = rules.show_messages;
        }
        if rules.immediate_respawn.is_some() {
            self.death_rules.immediate_respawn = rules.immediate_respawn;
        }
    }

    /// Resolves the same marked parameters used by translated server messages.
    pub(super) fn apply_death_info(&mut self, event: protocol::DeathInfoEvent) {
        let translate = |key: &str| self.translation(key);
        let template = json_ui::localize_text_chars(&event.message, &translate);
        let parameters = event
            .parameters
            .iter()
            .map(|parameter| {
                protocol::localize_parameter_prefix(parameter, &translate, usize::MAX).into_owned()
            })
            .collect::<Vec<_>>();
        self.death_reason = Arc::from(protocol::format_translation_chars(template, &parameters));
    }

    /// Authoritative health availability and life state, independent of HUD rounding.
    pub const fn local_player_alive(&self) -> Option<bool> {
        if self.local_player_health_available {
            self.last_known_local_player_alive
        } else {
            None
        }
    }

    /// Whether readiness has acknowledged the current death, independently of health.
    pub const fn local_player_respawn_ready(&self) -> bool {
        self.local_player_respawn_ready
    }

    /// Distinguishes deaths even when recovery and a later death share one UI drain.
    pub const fn local_player_death_generation(&self) -> u64 {
        self.local_player_death_generation
    }

    /// Acknowledges the current death at its committed UI FIFO position.
    pub fn apply_respawn_ready(
        &mut self,
        session_id: u64,
        fifo_sequence: u64,
        local_millis: u64,
    ) -> Result<(), UiRuntimeError> {
        self.validate_identity(session_id, fifo_sequence, local_millis, None)?;
        self.local_player_respawn_ready = true;
        self.last_fifo_sequence = Some(fifo_sequence);
        self.last_local_millis = Some(local_millis);
        Ok(())
    }

    /// Recovery retires a reason once, preserving a new packet arriving before death.
    pub(super) fn publish_local_player_alive(&mut self, next: bool) {
        if !next && self.last_known_local_player_alive != Some(false) {
            self.local_player_death_generation = self.local_player_death_generation.wrapping_add(1);
            if self.last_known_local_player_alive == Some(true) {
                self.local_player_respawn_ready = false;
            }
            // Vanilla dismisses a data-driven screen when its player dies.
            self.close_data_driven_screen(
                protocol::ddui::DataDrivenScreenCloseReason::ClientCanceled,
            );
        }
        if self.last_known_local_player_alive == Some(false) && next {
            self.death_reason = Arc::from("");
        }
        self.last_known_local_player_alive = Some(next);
        self.local_player_health_available = true;
    }
}

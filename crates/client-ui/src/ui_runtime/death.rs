//! Retains the localized local-player reason until authoritative recovery.

use std::sync::Arc;

use super::UiRuntime;

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
        let template = json_ui::localize_text(&event.message, &translate);
        let parameters = event
            .parameters
            .iter()
            .map(|parameter| {
                protocol::localize_parameter_prefix(parameter, &translate, usize::MAX).into_owned()
            })
            .collect::<Vec<_>>();
        self.death_reason = Arc::from(protocol::format_translation(&template, &parameters));
    }

    /// Authoritative health availability and life state, independent of HUD rounding.
    pub const fn local_player_alive(&self) -> Option<bool> {
        if self.local_player_health_available {
            self.last_known_local_player_alive
        } else {
            None
        }
    }

    /// Recovery retires a reason once, preserving a new packet arriving before death.
    pub(super) fn publish_local_player_alive(&mut self, next: bool) {
        if self.last_known_local_player_alive == Some(false) && next {
            self.death_reason = Arc::from("");
        }
        self.last_known_local_player_alive = Some(next);
        self.local_player_health_available = true;
    }
}

//! Passive session-bound evidence, never an effective permission resolver.
use protocol::AbilitiesUpdate;

use super::UiRuntime;

impl UiRuntime {
    /// Retires evidence and its admission binding. A drain cannot re-arm it.
    pub(crate) fn clear_local_abilities(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
    ) {
        player_runtime.facts.clear_local_abilities()
    }

    /// Accepted fatal-free bootstrap tail only; no received evidence is invented.
    pub(crate) fn bind_local_abilities(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        session: u64,
        stream: u64,
        actor_unique_id: i64,
        setup_succeeded: bool,
    ) {
        player_runtime
            .facts
            .bind_local_abilities(session, stream, actor_unique_id, setup_succeeded)
    }

    /// Missing, failed or replaced streams retire evidence, without minting a new binding.
    pub(crate) fn synchronize_local_abilities(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        session: u64,
        stream: Option<u64>,
    ) {
        player_runtime
            .facts
            .synchronize_local_abilities(session, stream)
    }

    /// Applies admitted ability evidence to the shared player owner.
    pub(crate) fn apply_local_abilities(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        session: u64,
        stream: u64,
        sequence: u64,
        update: AbilitiesUpdate,
    ) {
        player_runtime
            .facts
            .apply_local_abilities(session, stream, sequence, update)
    }

    /// None means unknown. Received-empty and unavailable remain distinct evidence.
    pub(crate) fn local_abilities<'a>(
        &self,
        player_runtime: &'a crate::player_runtime::PlayerRuntime,
    ) -> Option<&'a AbilitiesUpdate> {
        player_runtime.facts.local_abilities()
    }
}

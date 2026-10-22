//! Player death changes presentation flags while retaining the network actor.

use super::{ActorSnapshot, DEATH_DURATION_TICKS};
use protocol::{ActorKind, ActorMetadataValue, GameModeUpdate, PlayerGameMode};
use protocol_ids::{ActorDataKey, ActorFlag};

impl ActorSnapshot {
    /// Resolves a player's explicit or level-default spectator mode.
    pub(super) fn is_spectator(&self, world_default: Option<PlayerGameMode>) -> bool {
        matches!(
            self.player_game_mode,
            Some(GameModeUpdate::Explicit(PlayerGameMode::Spectator))
        ) || (self.player_game_mode == Some(GameModeUpdate::WorldDefault)
            && world_default == Some(PlayerGameMode::Spectator))
    }

    /// Hides a completed player death once; a later primary metadata word replaces these flags.
    pub(super) fn finish_player_death(&mut self, world_default: Option<PlayerGameMode>) {
        if !matches!(self.kind, ActorKind::Player { .. })
            || !self.status.dead
            || self.status.player_death_completed
            || self.status.native_death_ticks <= i16::from(DEATH_DURATION_TICKS)
            || self.is_spectator(world_default)
        {
            return;
        }
        self.status.player_death_completed = true;
        if self.is_invisible() {
            return;
        }
        let flags = self.local_presentation_flags.unwrap_or_else(|| {
            match self.metadata.get(&ActorDataKey::Flags.wire_value()) {
                Some(
                    ActorMetadataValue::Flags(flags) | ActorMetadataValue::FlagsExtended(flags),
                ) => *flags,
                _ => 0,
            }
        });
        self.local_presentation_flags = Some(
            (flags | (1_u64 << ActorFlag::Invisible.wire_value()))
                & !(1_u64 << ActorFlag::ShowName.wire_value()),
        );
    }
}

#[cfg(test)]
mod tests;

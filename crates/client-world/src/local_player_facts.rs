//! Authoritative local-player facts shared by gameplay and UI projections.
//!
//! Callers apply committed game modes, hunger and mount changes only after their
//! shared session/FIFO guard accepts the event. Abilities retain their separate
//! bootstrap binding and sequence fence. Equipment stays in the inventory ledger.

use protocol::{ActorAttribute, GameModeUpdate, PlayerGameMode};

use crate::game_mode_capabilities::GameModeCapabilities;

mod abilities;
mod stat;

pub use stat::LocalPlayerStat;

/// Session state consumed synchronously by movement and presentation.
#[derive(Clone, Debug, Default)]
pub struct LocalPlayerFacts {
    session_id: u64,
    local_abilities: abilities::LocalAbilities,
    player_game_mode: Option<PlayerGameMode>,
    world_default_game_mode: Option<PlayerGameMode>,
    player_mode_from_default: bool,
    server_authoritative_block_breaking: Option<bool>,
    hunger: Option<LocalPlayerStat>,
    mount_unique_id: Option<i64>,
    immobile: bool,
}

impl LocalPlayerFacts {
    /// Starts a session with unknown facts and no accepted ability binding.
    #[must_use]
    pub fn new(session_id: u64) -> Self {
        Self {
            session_id,
            ..Self::default()
        }
    }

    /// Clears all facts on a new session; repeating the current session is a no-op.
    pub fn begin_session(&mut self, session_id: u64) {
        if self.session_id != session_id {
            *self = Self::new(session_id);
        }
    }

    /// Returns the session whose committed facts this owner retains.
    #[must_use]
    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    /// Retires the block-breaking negotiation without changing other player facts.
    pub fn clear_block_breaking_mode(&mut self) {
        self.server_authoritative_block_breaking = None;
    }

    /// Installs the bootstrap negotiation only for this session after successful setup.
    pub fn install_block_breaking_mode(
        &mut self,
        session_generation: u64,
        mode: bool,
        setup_succeeded: bool,
    ) {
        if self.session_id() == session_generation && setup_succeeded {
            self.server_authoritative_block_breaking = Some(mode);
        }
    }

    /// Returns retained negotiation; it does not authorize a mining request by itself.
    #[must_use]
    pub const fn server_authoritative_block_breaking(&self) -> Option<bool> {
        self.server_authoritative_block_breaking
    }

    /// Installs an explicit mode without clearing or inventing attribute values.
    pub fn publish_player_game_mode(&mut self, game_mode: PlayerGameMode) {
        self.player_game_mode = Some(game_mode);
        self.player_mode_from_default = false;
    }

    /// Installs StartGame's resolved mode and its relationship to the world default.
    pub fn publish_bootstrap_game_modes(
        &mut self,
        player: PlayerGameMode,
        world_default: PlayerGameMode,
        player_uses_world_default: bool,
    ) {
        self.player_game_mode = Some(player);
        self.world_default_game_mode = Some(world_default);
        self.player_mode_from_default = player_uses_world_default;
    }

    /// Applies an accepted mode change; false leaves state intact for an odd packet.
    pub fn apply_game_mode_update(&mut self, update: GameModeUpdate) -> bool {
        match update {
            GameModeUpdate::Explicit(mode) => {
                self.player_game_mode = Some(mode);
                self.player_mode_from_default = false;
                true
            }
            GameModeUpdate::LegacyViewer => {
                self.apply_game_mode_update(GameModeUpdate::Explicit(PlayerGameMode::Spectator))
            }
            GameModeUpdate::WorldDefault => match self.world_default_game_mode {
                Some(default) => {
                    self.player_game_mode = Some(default);
                    self.player_mode_from_default = true;
                    true
                }
                None => false,
            },
            GameModeUpdate::Unknown(_) => false,
        }
    }

    /// Updates the world default and players bound to it; false marks an odd packet.
    pub fn apply_default_game_mode_update(&mut self, update: GameModeUpdate) -> bool {
        match update {
            GameModeUpdate::Explicit(mode) => {
                self.world_default_game_mode = Some(mode);
                if self.player_mode_from_default {
                    self.player_game_mode = Some(mode);
                }
                true
            }
            GameModeUpdate::LegacyViewer => self.apply_default_game_mode_update(
                GameModeUpdate::Explicit(PlayerGameMode::Spectator),
            ),
            GameModeUpdate::WorldDefault | GameModeUpdate::Unknown(_) => false,
        }
    }

    /// Returns the retained resolved mode, or None before it is known.
    #[must_use]
    pub const fn player_game_mode(&self) -> Option<PlayerGameMode> {
        self.player_game_mode
    }

    /// Resolves known mode defaults against the latest accepted ability evidence.
    #[must_use]
    pub fn game_mode_capabilities(&self) -> Option<GameModeCapabilities> {
        self.player_game_mode
            .map(|mode| GameModeCapabilities::resolve(mode, self.local_abilities()))
    }

    /// Keeps the existing survival-stat visibility default while the mode is unknown.
    #[must_use]
    pub const fn survival_stats_visible(&self) -> bool {
        match self.player_game_mode {
            Some(mode) => mode.shows_survival_stats(),
            None => true,
        }
    }

    /// Retains accepted hunger using the existing quantization; false preserves prior state.
    pub fn apply_hunger_attribute(&mut self, attribute: &ActorAttribute) -> bool {
        let Some(hunger) = LocalPlayerStat::from_attribute(attribute) else {
            return false;
        };
        self.hunger = Some(hunger);
        true
    }

    /// Returns the hunger attribute in the same units used by the existing HUD adapter.
    #[must_use]
    pub const fn hunger(&self) -> Option<LocalPlayerStat> {
        self.hunger
    }

    /// Applies a mount change after the caller accepts its committed identity.
    pub fn set_mount(&mut self, ridden_unique_id: Option<i64>) {
        self.mount_unique_id = ridden_unique_id;
    }

    /// Returns the committed mount, or None when the player is not riding.
    #[must_use]
    pub const fn mount_unique_id(&self) -> Option<i64> {
        self.mount_unique_id
    }

    /// Applies FIFO-committed local flags for this session; omitted fields retain their state.
    pub fn apply_local_movement_flags(
        &mut self,
        session_id: u64,
        flags: crate::MovementFlagUpdate,
    ) -> bool {
        if self.session_id != session_id {
            return false;
        }
        if let Some(immobile) = flags.immobile {
            self.immobile = immobile;
        }
        true
    }

    /// Server immobility prevents travel without locking camera or raw input.
    #[must_use]
    pub const fn is_immobile(&self) -> bool {
        self.immobile
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod abilities_tests;

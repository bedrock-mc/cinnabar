use jolyne::GameData;

use crate::ActorGameMode;

/// StartGame's local-player game mode reduced to the HUD distinctions Cinnabar owns.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PlayerGameMode {
    Survival,
    Creative,
    Adventure,
    Spectator,
    Unknown,
}

impl PlayerGameMode {
    #[must_use]
    pub fn from_game_data(game_data: &GameData) -> Self {
        let start_game = &game_data.start_game;
        Self::from_actor_game_modes(
            start_game.player_gamemode.into(),
            start_game.world_gamemode.into(),
        )
    }

    #[must_use]
    pub const fn from_actor_game_modes(player: ActorGameMode, world: ActorGameMode) -> Self {
        let effective = player.resolve_fallback(world);
        match effective {
            ActorGameMode::Survival => Self::Survival,
            ActorGameMode::Creative => Self::Creative,
            ActorGameMode::Adventure => Self::Adventure,
            ActorGameMode::SurvivalSpectator
            | ActorGameMode::CreativeSpectator
            | ActorGameMode::Spectator => Self::Spectator,
            ActorGameMode::Fallback | ActorGameMode::Unknown(_) => Self::Unknown,
        }
    }

    #[must_use]
    pub const fn shows_hotbar(self) -> bool {
        matches!(self, Self::Survival | Self::Creative | Self::Adventure)
    }

    #[must_use]
    pub const fn shows_survival_stats(self) -> bool {
        matches!(self, Self::Survival | Self::Adventure)
    }
}

/// Session-scoped StartGame identity and game-mode authority for the local player.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct LocalPlayerGameModeAuthority {
    unique_id: i64,
    game_mode: ActorGameMode,
    default_game_mode: ActorGameMode,
}

impl LocalPlayerGameModeAuthority {
    #[must_use]
    pub fn from_game_data(game_data: &GameData) -> Self {
        Self {
            unique_id: game_data.start_game.entity_id,
            game_mode: game_data.start_game.player_gamemode.into(),
            default_game_mode: game_data.start_game.world_gamemode.into(),
        }
    }

    #[must_use]
    pub const fn new(
        unique_id: i64,
        game_mode: ActorGameMode,
        default_game_mode: ActorGameMode,
    ) -> Self {
        Self {
            unique_id,
            game_mode,
            default_game_mode,
        }
    }

    #[must_use]
    pub const fn unique_id(self) -> i64 {
        self.unique_id
    }

    #[must_use]
    pub const fn raw_game_mode(self) -> ActorGameMode {
        self.game_mode
    }

    #[must_use]
    pub const fn default_game_mode(self) -> ActorGameMode {
        self.default_game_mode
    }

    #[must_use]
    pub const fn resolved_game_mode(self) -> ActorGameMode {
        self.game_mode.resolve_fallback(self.default_game_mode)
    }

    #[must_use]
    pub const fn player_game_mode(self) -> PlayerGameMode {
        PlayerGameMode::from_actor_game_modes(self.game_mode, self.default_game_mode)
    }

    #[must_use]
    pub const fn is_render_eligible(self) -> bool {
        !self.resolved_game_mode().is_spectator()
    }

    /// Applies a per-player authority update only when its StartGame identity matches.
    /// Returns the new effective mode only when player-visible authority changed.
    pub fn update_player(
        &mut self,
        unique_id: i64,
        game_mode: ActorGameMode,
    ) -> Option<PlayerGameMode> {
        if unique_id != self.unique_id {
            return None;
        }
        let previous = self.player_game_mode();
        self.game_mode = game_mode;
        let current = self.player_game_mode();
        (current != previous).then_some(current)
    }

    /// Applies world-default authority and reports only an effective-mode change.
    pub fn update_default(&mut self, default_game_mode: ActorGameMode) -> Option<PlayerGameMode> {
        let previous = self.player_game_mode();
        self.default_game_mode = default_game_mode;
        let current = self.player_game_mode();
        (current != previous).then_some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_game_fallback_uses_the_authoritative_world_mode() {
        assert_eq!(
            PlayerGameMode::from_actor_game_modes(ActorGameMode::Fallback, ActorGameMode::Creative,),
            PlayerGameMode::Creative
        );
        assert_eq!(
            PlayerGameMode::from_actor_game_modes(ActorGameMode::Fallback, ActorGameMode::Survival,),
            PlayerGameMode::Survival
        );
        assert_eq!(
            PlayerGameMode::from_actor_game_modes(
                ActorGameMode::Unknown(77),
                ActorGameMode::Creative,
            ),
            PlayerGameMode::Unknown
        );
    }

    #[test]
    fn local_authority_correlates_exact_identity_and_re_resolves_fallback() {
        let mut authority =
            LocalPlayerGameModeAuthority::new(-9, ActorGameMode::Survival, ActorGameMode::Survival);
        assert_eq!(authority.update_player(8, ActorGameMode::Spectator), None);
        assert!(authority.is_render_eligible());
        assert_eq!(authority.update_player(-9, ActorGameMode::Fallback), None);
        assert_eq!(
            authority.update_default(ActorGameMode::Spectator),
            Some(PlayerGameMode::Spectator)
        );
        assert!(!authority.is_render_eligible());
    }
}

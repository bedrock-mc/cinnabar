//! Local-player game-mode reduction shared by StartGame and runtime updates.

use valentine::bedrock::version::v1_26_30::GameMode;

use jolyne::GameData;

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
        Self::from_game_modes(start_game.player_gamemode, start_game.world_gamemode)
    }

    fn from_game_modes(player: GameMode, world: GameMode) -> Self {
        let effective = if player == GameMode::Fallback {
            world
        } else {
            player
        };
        match effective {
            GameMode::Survival => Self::Survival,
            GameMode::Creative => Self::Creative,
            GameMode::Adventure => Self::Adventure,
            GameMode::SurvivalSpectator | GameMode::CreativeSpectator | GameMode::Spectator => {
                Self::Spectator
            }
            GameMode::Fallback | GameMode::Unknown(_) => Self::Unknown,
        }
    }

    /// Maps a runtime SetPlayerGameType value without a world-mode fallback.
    ///
    /// The level-default sentinel and unknown values return `None`: a runtime
    /// change cannot be resolved against StartGame's world mode here, so the
    /// caller keeps its current authoritative mode rather than guessing.
    #[must_use]
    pub fn from_explicit_game_mode(mode: GameMode) -> Option<Self> {
        match mode {
            GameMode::Survival => Some(Self::Survival),
            GameMode::Creative => Some(Self::Creative),
            GameMode::Adventure => Some(Self::Adventure),
            GameMode::SurvivalSpectator | GameMode::CreativeSpectator | GameMode::Spectator => {
                Some(Self::Spectator)
            }
            GameMode::Fallback | GameMode::Unknown(_) => None,
        }
    }

    /// StartGame's world default mode, retained so a later level-default
    /// sentinel (SetPlayerGameType 5) or SetDefaultGameType can resolve.
    #[must_use]
    pub fn world_default_from_game_data(game_data: &GameData) -> Self {
        Self::from_explicit_game_mode(game_data.start_game.world_gamemode).unwrap_or(Self::Unknown)
    }

    /// Whether StartGame bound the player to the level default rather than an
    /// explicit personal mode.
    #[must_use]
    pub fn bootstrap_uses_world_default(game_data: &GameData) -> bool {
        game_data.start_game.player_gamemode == GameMode::Fallback
    }

    /// Typed wire value for runtime game-mode packets.
    #[must_use]
    pub fn update_from_game_mode(mode: GameMode) -> crate::GameModeUpdate {
        match Self::from_explicit_game_mode(mode) {
            Some(resolved) => crate::GameModeUpdate::Explicit(resolved),
            None if mode == GameMode::Fallback => crate::GameModeUpdate::WorldDefault,
            None => crate::GameModeUpdate::Unknown(match mode {
                GameMode::Unknown(value) => value,
                _ => -1,
            }),
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

#[cfg(test)]
mod player_game_mode_tests {
    use super::{GameMode, PlayerGameMode};

    #[test]
    fn start_game_fallback_uses_the_authoritative_world_mode() {
        assert_eq!(
            PlayerGameMode::from_game_modes(GameMode::Fallback, GameMode::Creative),
            PlayerGameMode::Creative
        );
        assert_eq!(
            PlayerGameMode::from_game_modes(GameMode::Fallback, GameMode::Survival),
            PlayerGameMode::Survival
        );
        assert_eq!(
            PlayerGameMode::from_game_modes(GameMode::Unknown(77), GameMode::Creative),
            PlayerGameMode::Unknown
        );
    }
}

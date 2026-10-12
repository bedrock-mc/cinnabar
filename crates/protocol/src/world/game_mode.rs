//! Local-player game-mode reduction shared by StartGame and runtime updates.

use bedrock_protocol_1_26_51::enums::GameType;
use bedrock_protocol_1_26_51::packets::{StartGame, UpdatePlayerGameType};

pub(super) fn targeted_update(packet: UpdatePlayerGameType) -> crate::UiEvent {
    crate::UiEvent::PlayerGameMode {
        actor_unique_id: packet.target_player.0,
        tick: packet.tick.0,
        event: crate::GameModeEvent {
            update: PlayerGameMode::update_from_game_mode(packet.player_game_type),
        },
    }
}

/// One Bedrock `GameType` wire value reduced to the distinctions the HUD needs.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum GameTypeValue {
    Survival,
    Creative,
    Adventure,
    /// The level-default sentinel, wire value 5.
    Default,
    /// Wire value 6.
    Spectator,
    /// Any value the generated enum does not name, including `Undefined` (-1).
    Other(i32),
}

impl From<GameType> for GameTypeValue {
    fn from(mode: GameType) -> Self {
        match mode {
            GameType::Survival => Self::Survival,
            GameType::Creative => Self::Creative,
            GameType::Adventure => Self::Adventure,
            GameType::Default => Self::Default,
            GameType::Spectator => Self::Spectator,
            GameType::Undefined => Self::Other(-1),
            GameType::Unknown(value) => Self::Other(value),
        }
    }
}

/// Wire value of vanilla's legacy `SurvivalViewer` game type.
///
/// The generated enum names only -1/0/1/2/5/6, so the two legacy viewer modes arrive as
/// `Unknown`; they are still spectator-like game types.
const SURVIVAL_VIEWER_GAME_TYPE: i32 = 3;
/// Wire value of vanilla's legacy `CreativeViewer` game type.
const CREATIVE_VIEWER_GAME_TYPE: i32 = 4;

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
    pub(crate) fn from_start_game(start_game: &StartGame) -> Self {
        // The world default moved into StartGame's nested LevelSettings block
        // in 1.26.40; the personal mode stays on the packet itself.
        Self::from_game_modes(
            start_game.game_type.into(),
            start_game.settings.game_type.into(),
        )
    }

    fn from_game_modes(player: GameTypeValue, world: GameTypeValue) -> Self {
        let effective = if player == GameTypeValue::Default {
            world
        } else {
            player
        };
        Self::from_game_type(effective).unwrap_or(Self::Unknown)
    }

    fn from_game_type(mode: GameTypeValue) -> Option<Self> {
        match mode {
            GameTypeValue::Survival => Some(Self::Survival),
            GameTypeValue::Creative => Some(Self::Creative),
            GameTypeValue::Adventure => Some(Self::Adventure),
            GameTypeValue::Spectator => Some(Self::Spectator),
            GameTypeValue::Other(SURVIVAL_VIEWER_GAME_TYPE | CREATIVE_VIEWER_GAME_TYPE) => {
                Some(Self::Spectator)
            }
            GameTypeValue::Default | GameTypeValue::Other(_) => None,
        }
    }

    /// StartGame's world default mode, retained so a later level-default
    /// sentinel (SetPlayerGameType 5) or SetDefaultGameType can resolve.
    pub(crate) fn world_default_update_from_start_game(
        start_game: &StartGame,
    ) -> crate::GameModeUpdate {
        Self::update_from_game_type(start_game.settings.game_type.into())
    }

    /// Whether StartGame bound the player to the level default rather than an
    /// explicit personal mode.
    pub(crate) fn bootstrap_uses_world_default(start_game: &StartGame) -> bool {
        GameTypeValue::from(start_game.game_type) == GameTypeValue::Default
    }

    /// Typed wire value for a runtime SetPlayerGameType or SetDefaultGameType packet.
    #[must_use]
    pub(crate) fn update_from_game_mode(mode: GameType) -> crate::GameModeUpdate {
        Self::update_from_game_type(mode.into())
    }

    fn update_from_game_type(mode: GameTypeValue) -> crate::GameModeUpdate {
        if matches!(
            mode,
            GameTypeValue::Other(SURVIVAL_VIEWER_GAME_TYPE | CREATIVE_VIEWER_GAME_TYPE)
        ) {
            return crate::GameModeUpdate::LegacyViewer;
        }
        match Self::from_game_type(mode) {
            Some(resolved) => crate::GameModeUpdate::Explicit(resolved),
            None => match mode {
                GameTypeValue::Default => crate::GameModeUpdate::WorldDefault,
                GameTypeValue::Other(value) => crate::GameModeUpdate::Unknown(value),
                // `from_game_type` resolves every other arm.
                _ => crate::GameModeUpdate::Unknown(-1),
            },
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
    use super::{GameTypeValue, PlayerGameMode};

    #[test]
    fn start_game_fallback_uses_the_authoritative_world_mode() {
        assert_eq!(
            PlayerGameMode::from_game_modes(GameTypeValue::Default, GameTypeValue::Creative),
            PlayerGameMode::Creative
        );
        assert_eq!(
            PlayerGameMode::from_game_modes(GameTypeValue::Default, GameTypeValue::Survival),
            PlayerGameMode::Survival
        );
        assert_eq!(
            PlayerGameMode::from_game_modes(GameTypeValue::Other(77), GameTypeValue::Creative),
            PlayerGameMode::Unknown
        );
    }

    #[test]
    fn legacy_viewer_game_types_stay_spectator_after_the_enum_lost_their_names() {
        for value in [3, 4] {
            assert_eq!(
                PlayerGameMode::from_game_type(GameTypeValue::Other(value)),
                Some(PlayerGameMode::Spectator)
            );
            let update = PlayerGameMode::update_from_game_mode(
                bedrock_protocol_1_26_51::enums::GameType::Unknown(value),
            );
            assert_eq!(update, crate::GameModeUpdate::LegacyViewer);
            assert_eq!(update.hud_mode(), Some(PlayerGameMode::Spectator));
        }
    }
}

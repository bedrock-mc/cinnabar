//! Local-player game-mode reduction shared by StartGame and runtime updates.

use valentine::bedrock::version::v1_26_40::{
    EnumsGameType, SetDefaultGameTypePacketDefaultGameType,
};

use jolyne::GameData;

/// One Bedrock `GameType` wire value, independent of which packet carried it.
///
/// 1.26.30 exposed a single `GameMode` enum. 1.26.40 generates one structurally
/// identical enum per field (`StartGamePacketGameType`, `LevelSettingsGameType`,
/// `SetPlayerGameTypePacketPlayerGameType`,
/// `SetDefaultGameTypePacketDefaultGameType`), so the shared reduction below
/// funnels all four through this local value instead of being written out four
/// times.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum GameTypeValue {
    Survival,
    Creative,
    Adventure,
    /// The level-default sentinel, wire value 5 (1.26.30 named this `Fallback`).
    Default,
    /// Wire value 6.
    Spectator,
    /// Any value the generated enums do not name, including `Undefined` (-1).
    Other(i32),
}

macro_rules! game_type_value_from {
    ($($generated:path),+ $(,)?) => {
        $(
            impl From<$generated> for GameTypeValue {
                fn from(mode: $generated) -> Self {
                    use $generated as Generated;
                    match mode {
                        Generated::Survival => Self::Survival,
                        Generated::Creative => Self::Creative,
                        Generated::Adventure => Self::Adventure,
                        Generated::Default => Self::Default,
                        Generated::Spectator => Self::Spectator,
                        // `Undefined` is the generated name for wire value -1.
                        Generated::Undefined => Self::Other(-1),
                        Generated::Unknown(value) => Self::Other(value),
                    }
                }
            }
        )+
    };
}

game_type_value_from!(EnumsGameType);

impl From<SetDefaultGameTypePacketDefaultGameType> for GameTypeValue {
    fn from(mode: SetDefaultGameTypePacketDefaultGameType) -> Self {
        use SetDefaultGameTypePacketDefaultGameType as Generated;
        match mode {
            Generated::Survival => Self::Survival,
            Generated::Creative => Self::Creative,
            Generated::Adventure => Self::Adventure,
            Generated::Default => Self::Default,
            Generated::Spectator => Self::Spectator,
            Generated::Unknown(value) => Self::Other(value),
        }
    }
}

/// Wire value of vanilla's legacy `SurvivalViewer` game type.
///
/// The 1.26.40 generated enums name only -1/0/1/2/5/6, so the two legacy viewer
/// modes arrive as `Unknown`. They are still spectator-like game types in
/// vanilla and are reduced the same way 1.26.30 reduced `SurvivalSpectator` and
/// `CreativeSpectator`.
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
    #[must_use]
    pub fn from_game_data(game_data: &GameData) -> Self {
        let start_game = &game_data.start_game;
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

    /// Maps a runtime SetPlayerGameType value without a world-mode fallback.
    ///
    /// The level-default sentinel and unknown values return `None`: a runtime
    /// change cannot be resolved against StartGame's world mode here, so the
    /// caller keeps its current authoritative mode rather than guessing.
    #[must_use]
    pub fn from_explicit_game_mode(mode: EnumsGameType) -> Option<Self> {
        Self::from_game_type(mode.into())
    }

    /// StartGame's world default mode, retained so a later level-default
    /// sentinel (SetPlayerGameType 5) or SetDefaultGameType can resolve.
    #[must_use]
    pub fn world_default_from_game_data(game_data: &GameData) -> Self {
        Self::from_game_type(game_data.start_game.settings.game_type.into())
            .unwrap_or(Self::Unknown)
    }

    /// Whether StartGame bound the player to the level default rather than an
    /// explicit personal mode.
    #[must_use]
    pub fn bootstrap_uses_world_default(game_data: &GameData) -> bool {
        GameTypeValue::from(game_data.start_game.game_type) == GameTypeValue::Default
    }

    /// Typed wire value for a runtime SetPlayerGameType packet.
    #[must_use]
    pub fn update_from_game_mode(mode: EnumsGameType) -> crate::GameModeUpdate {
        Self::update_from_game_type(mode.into())
    }

    /// Typed wire value for a runtime SetDefaultGameType packet.
    ///
    /// 1.26.40 gives SetDefaultGameType its own generated enum, so this is the
    /// sibling of [`Self::update_from_game_mode`] rather than a second call
    /// into it.
    #[must_use]
    pub fn update_from_default_game_mode(
        mode: SetDefaultGameTypePacketDefaultGameType,
    ) -> crate::GameModeUpdate {
        Self::update_from_game_type(mode.into())
    }

    fn update_from_game_type(mode: GameTypeValue) -> crate::GameModeUpdate {
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
        }
    }
}

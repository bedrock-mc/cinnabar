//! Xbox activity follows committed world state independently of Discord activity.
use protocol::{PlayerGameMode, bridge::XboxPresenceState};

/// Reports menus until the world default is known, with generic featured/experience activity.
pub(super) fn state(
    in_world: bool,
    mode: Option<PlayerGameMode>,
    address: Option<&str>,
    featured: bool,
) -> XboxPresenceState {
    if !in_world || mode.is_none() {
        return XboxPresenceState::default();
    }
    let address = address.unwrap_or_default();
    XboxPresenceState {
        in_world: true,
        game_mode: match mode {
            Some(PlayerGameMode::Creative) => 1,
            Some(PlayerGameMode::Adventure) => 2,
            _ => 0,
        },
        realm: address.starts_with("realm_id/"),
        experience: featured || address.starts_with(launcher::menu::EXPERIENCE_ADDRESS_PREFIX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_world::LocalPlayerFacts;

    #[test]
    fn personal_game_mode_does_not_change_world_activity() {
        let mut facts = LocalPlayerFacts::new(1);
        facts.publish_bootstrap_game_modes(
            PlayerGameMode::Creative,
            PlayerGameMode::Survival,
            false,
        );
        assert_eq!(
            state(true, facts.world_default_game_mode(), None, false).game_mode,
            0
        );
        facts.apply_default_game_mode_update(protocol::GameModeUpdate::Explicit(
            PlayerGameMode::Adventure,
        ));
        assert_eq!(
            state(
                true,
                facts.world_default_game_mode(),
                Some("realm_id/42"),
                false
            ),
            XboxPresenceState {
                in_world: true,
                game_mode: 2,
                realm: true,
                experience: false
            }
        );
        assert_eq!(facts.player_game_mode(), Some(PlayerGameMode::Creative));
    }

    #[test]
    fn leaving_or_loading_reports_menus_and_clears_experience() {
        assert_eq!(
            state(
                false,
                Some(PlayerGameMode::Creative),
                Some("gathering/id"),
                true
            ),
            XboxPresenceState::default()
        );
        assert_eq!(
            state(true, None, Some("realm_id/42"), false),
            XboxPresenceState::default()
        );
        let playing = state(
            true,
            Some(PlayerGameMode::Survival),
            Some("gathering/id"),
            false,
        );
        assert!(playing.experience);
        let featured = state(
            true,
            Some(PlayerGameMode::Adventure),
            Some("example.invalid"),
            true,
        );
        assert!(featured.experience);
    }
}

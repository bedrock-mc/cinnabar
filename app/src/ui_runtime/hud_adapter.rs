use protocol::{ActorAttribute, PlayerStatus};
use ui::{BoundedStat, HudPlayerStatus};

pub(super) fn attribute_stat(attribute: &ActorAttribute) -> Option<BoundedStat> {
    let stat = client_world::LocalPlayerStat::from_attribute(attribute)?;
    ui::BoundedStat::new_scaled(stat.current(), stat.maximum(), stat.scale())
}

pub(super) fn player_status(status: PlayerStatus) -> HudPlayerStatus {
    match status {
        PlayerStatus::LoginSuccess => HudPlayerStatus::LoginSuccess,
        PlayerStatus::FailedClient => HudPlayerStatus::FailedClient,
        PlayerStatus::FailedSpawn => HudPlayerStatus::FailedSpawn,
        PlayerStatus::PlayerSpawn => HudPlayerStatus::PlayerSpawn,
        PlayerStatus::UnsupportedEdition => HudPlayerStatus::UnsupportedEdition,
        PlayerStatus::FailedServerFull => HudPlayerStatus::FailedServerFull,
        PlayerStatus::FailedEditorVanillaMismatch => HudPlayerStatus::FailedEditorVanillaMismatch,
        PlayerStatus::FailedVanillaEditorMismatch => HudPlayerStatus::FailedVanillaEditorMismatch,
    }
}

#[cfg(test)]
mod tests {
    use super::player_status;
    use protocol::PlayerStatus;
    use ui::HudPlayerStatus;

    #[test]
    fn unsupported_edition_status_stays_semantic_at_the_hud_boundary() {
        assert_eq!(
            player_status(PlayerStatus::UnsupportedEdition),
            HudPlayerStatus::UnsupportedEdition
        );
    }
}

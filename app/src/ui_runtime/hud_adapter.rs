use protocol::{ActorAttribute, PlayerStatus};
use ui::{BoundedStat, HudPlayerStatus};

pub(super) fn attribute_stat(attribute: &ActorAttribute) -> Option<BoundedStat> {
    if !attribute.current.is_finite()
        || !attribute.max.is_finite()
        || attribute.max <= 0.0
        || attribute.current < 0.0
        || attribute.current > attribute.max
    {
        return None;
    }
    let scale = if attribute.max <= u16::MAX as f32 / 100.0 {
        100.0
    } else {
        1.0
    };
    let maximum = u16::try_from((attribute.max * scale).round() as u32).ok()?;
    let current = u16::try_from((attribute.current * scale).round() as u32).ok()?;
    BoundedStat::new_scaled(current, maximum, scale as u16)
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

use protocol::{ActorAttribute, PlayerStatus};
use ui::{BoundedStat, HudPlayerStatus};

pub(super) fn attribute_stat(attribute: &ActorAttribute) -> Option<BoundedStat> {
    if attribute.name.as_ref() == "minecraft:absorption" {
        return BoundedStat::from_absorption_points(attribute.current);
    }
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

#[cfg(test)]
mod absorption_tests {
    use super::attribute_stat;
    use std::sync::Arc;

    #[test]
    fn absorption_with_an_unbounded_maximum_reaches_the_hud() {
        let attribute = protocol::ActorAttribute {
            name: Arc::from("minecraft:absorption"),
            min: 0.0,
            max: f32::MAX,
            current: 4.0,
            default: Some(0.0),
            modifiers: Arc::from([]),
        };
        for max in [f32::MAX, 0.0, 20.0] {
            let attribute = protocol::ActorAttribute {
                max,
                ..attribute.clone()
            };
            assert_eq!(attribute_stat(&attribute), ui::BoundedStat::new(4, 4));
        }
    }
}

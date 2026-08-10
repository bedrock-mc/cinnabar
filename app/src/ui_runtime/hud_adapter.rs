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
    let ordinal = status as u8;
    match status {
        PlayerStatus::LoginSuccess => HudPlayerStatus::LoginSuccess,
        PlayerStatus::FailedClient => HudPlayerStatus::FailedClient,
        PlayerStatus::FailedSpawn => HudPlayerStatus::FailedSpawn,
        PlayerStatus::PlayerSpawn => HudPlayerStatus::PlayerSpawn,
        PlayerStatus::FailedServerFull => HudPlayerStatus::FailedServerFull,
        PlayerStatus::FailedEditorVanillaMismatch => HudPlayerStatus::FailedEditorVanillaMismatch,
        PlayerStatus::FailedVanillaEditorMismatch => HudPlayerStatus::FailedVanillaEditorMismatch,
        _ => match ordinal {
            4 => HudPlayerStatus::Reserved4,
            5 => HudPlayerStatus::Reserved5,
            6 => HudPlayerStatus::Reserved6,
            _ => unreachable!("all public player-status variants are matched"),
        },
    }
}

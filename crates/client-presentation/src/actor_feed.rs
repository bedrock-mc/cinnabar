//! Converts borrowed local physics and skin observations into the actor animation feed.
use client_world::{LocalItemUse, LocalPlayerFeed};
/// Builds this frame's client-authored local-player feed from the predicted physics state and
/// the look pose. The yaw/pitch come from the look input (`LocalViewPose`), never the boomed
/// third-person camera. Returns `None` before physics or on any non-finite value.
pub fn build_local_player_feed(
    physics: &dyn crate::observations::PhysicsObservation,
    look: bevy::math::Quat,
    first_person: bool,
    view_bobbing: bool,
    local_uuid: [u8; 16],
    skin: impl FnOnce() -> protocol::PlayerSkin,
    item_use: LocalItemUse,
) -> Option<LocalPlayerFeed> {
    let state = physics.state()?;
    let (yaw, pitch, _) = look.to_euler(bevy::math::EulerRot::YXZ);
    let yaw_degrees = (180.0 - yaw.to_degrees()).rem_euclid(360.0);
    let pitch_degrees = -pitch.to_degrees();
    let position = [
        state.position.x as f32,
        state.position.y as f32,
        state.position.z as f32,
    ];
    let velocity = [
        state.velocity.x as f32,
        state.velocity.y as f32,
        state.velocity.z as f32,
    ];
    if !position
        .iter()
        .chain(&velocity)
        .chain(&[yaw_degrees, pitch_degrees])
        .all(|value| value.is_finite())
    {
        return None;
    }
    let (sneaking, sprinting) = physics.latest_sneak_sprint().unwrap_or_default();
    Some(LocalPlayerFeed {
        // A real player-list echo overrides this; without one, the stream backs the local body
        // with the client's own uploaded skin under this stable local uuid.
        uuid: local_uuid,
        username: std::sync::Arc::from(""),
        skin: skin(),
        position,
        velocity,
        on_ground: state.on_ground,
        yaw: yaw_degrees,
        head_yaw: yaw_degrees,
        pitch: pitch_degrees,
        main_hand: None,
        off_hand: None,
        teleported: false,
        first_person,
        view_bobbing,
        sneaking,
        sprinting,
        item_use,
    })
}

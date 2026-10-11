//! Action-facing overrides shared by melee and item use.
use bevy::prelude::*;
use client_presentation::{
    aim_assist::AimAssistFrame, camera::ServerCameraView, local_player::LocalViewPose,
};

/// Applies the native action override to player facing and the same unsent movement tick.
pub fn rotate_for_action(
    frame: &AimAssistFrame,
    camera: &ServerCameraView,
    view: &mut LocalViewPose,
    movement: &mut crate::movement::MovementTicker,
    tick: u64,
) {
    if let Some(rotation) = action_rotation(frame, camera) {
        apply_action_rotation(rotation, view, movement, tick);
    }
}

/// The facing an attack or release turns the player to, when the camera and scheme allow it.
pub fn action_rotation(frame: &AimAssistFrame, camera: &ServerCameraView) -> Option<Quat> {
    use client_presentation::aim_assist::{AimAssistControlScheme, rotates_player_on_projectile};
    let direction = frame.interaction_direction()?;
    let scheme = AimAssistControlScheme::from_wire(camera.active_control_scheme().unwrap_or(0))?;
    camera
        .active_base_preset_name()
        .is_some_and(|camera| rotates_player_on_projectile(camera, scheme))
        .then(|| client_presentation::camera::look_at_target(Vec3::ZERO, direction))
}

/// Writes an action facing to the view and to `tick`'s unsent movement input.
pub fn apply_action_rotation(
    rotation: Quat,
    view: &mut LocalViewPose,
    movement: &mut crate::movement::MovementTicker,
    tick: u64,
) {
    let (yaw, pitch, _) = rotation.to_euler(EulerRot::YXZ);
    if movement.override_action_rotation(
        tick,
        -pitch.to_degrees(),
        gameplay::movement::wire_yaw(180.0 - yaw.to_degrees()),
    ) {
        view.set_rotation(rotation);
    }
}

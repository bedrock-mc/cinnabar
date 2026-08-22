use bevy::log::info;
use client_world::CommittedControlEvent;

use crate::camera::CameraSettingsAuthority;
use crate::local_player::LocalViewPose;
use crate::runtime::telemetry::bedrock_camera_rotation;

/// Applies one committed local-player control event to the view pose, camera
/// authority, and pending spawn anchor.
///
/// Position-bearing events rotate and translate the view; environment-only and
/// impulse events carry no resolved position and return unchanged.
pub(crate) fn apply_committed_control(
    control: CommittedControlEvent,
    view: &mut LocalViewPose,
    camera_settings: &mut CameraSettingsAuthority,
    pending_surface_spawn: &mut Option<[i32; 2]>,
) {
    let resolved = match control {
        CommittedControlEvent::MovePlayer {
            movement, resolved, ..
        } => {
            info!(
                runtime_id = movement.runtime_id,
                position = ?movement.position,
                "applying committed local MovePlayer"
            );
            if movement.yaw.is_finite() && movement.pitch.is_finite() {
                view.set_rotation(bedrock_camera_rotation(movement.yaw, movement.pitch));
            }
            resolved
        }
        CommittedControlEvent::PlayerMovementCorrection {
            correction,
            resolved,
            ..
        } => {
            info!(
                tick = correction.tick,
                position = ?correction.position,
                "applying committed server-authoritative movement correction"
            );
            resolved
        }
        CommittedControlEvent::ChangeDimension { resolved, .. } => {
            camera_settings.reset_perspective();
            resolved
        }
        CommittedControlEvent::Respawn {
            respawn, resolved, ..
        } => {
            info!(
                state = respawn.state,
                runtime_entity_id = respawn.runtime_entity_id,
                position = ?respawn.position,
                "applying committed Respawn"
            );
            resolved
        }
        CommittedControlEvent::SetTime { .. }
        | CommittedControlEvent::DaylightCycle { .. }
        | CommittedControlEvent::Weather { .. }
        | CommittedControlEvent::LocalMovementEffect { .. }
        | CommittedControlEvent::LocalMovementSpeed { .. }
        | CommittedControlEvent::LocalActorMotion { .. } => return,
    };
    view.set_eye_translation(bevy::prelude::Vec3::from_array(resolved.position));
    *pending_surface_spawn = resolved.surface_anchor;
}

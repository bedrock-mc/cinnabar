use bevy::log::info;
use client_world::CommittedControlEvent;

use crate::camera::CameraSettingsAuthority;
use crate::local_player::LocalViewPose;
use crate::runtime::telemetry::bedrock_camera_rotation;

/// Applies one committed local-player control event to the view pose, camera
/// authority, and pending spawn anchor.
///
/// MovePlayer owns rotation; prediction corrections only reconcile movement.
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
            view.set_freelook(false);
            resolved
        }
        CommittedControlEvent::Respawn {
            respawn, resolved, ..
        } => {
            log_respawn(respawn);
            if !respawn.ready_to_spawn() {
                return;
            }
            resolved
        }
        CommittedControlEvent::SetTime { .. }
        | CommittedControlEvent::DimensionChangeAck { .. }
        | CommittedControlEvent::WorldClocks { .. }
        | CommittedControlEvent::DaylightCycle { .. }
        | CommittedControlEvent::WeatherCycle { .. }
        | CommittedControlEvent::Weather { .. }
        | CommittedControlEvent::LocalMovementEffect { .. }
        | CommittedControlEvent::LocalMovementSpeed { .. }
        | CommittedControlEvent::LocalMovementFlags { .. }
        | CommittedControlEvent::NetworkStackLatency { .. }
        | CommittedControlEvent::LocalActorMotion { .. }
        | CommittedControlEvent::LocalMovementBoost { .. }
        | CommittedControlEvent::LocalHurt { .. }
        | CommittedControlEvent::PlayerListChanged { .. } => return,
    };
    view.set_eye_translation(bevy::prelude::Vec3::from_array(resolved.position));
    *pending_surface_spawn = resolved.surface_anchor;
}

pub(super) fn log_respawn(respawn: protocol::RespawnEvent) {
    info!(
        state = respawn.state,
        runtime_entity_id = respawn.runtime_entity_id,
        position = ?respawn.position,
        "applying committed Respawn"
    );
}

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
            let preserve_rotation = camera_settings.preserves_teleport_rotation()
                && movement.mode == protocol::MovePlayerMode::Teleport;
            if !preserve_rotation && movement.yaw.is_finite() && movement.pitch.is_finite() {
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
    view.reanchor_camera();
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::{EulerRot, Quat, Vec3};

    #[test]
    fn teleport_policy_preserves_only_teleport_aim_and_always_reconciles_position() {
        use protocol::MovePlayerMode::*;
        let aim = Quat::from_euler(EulerRot::YXZ, 0.35, -0.2, 0.0);
        let server_aim = bedrock_camera_rotation(90.0, 15.0);
        for enabled in [false, true] {
            for mode in [Normal, Reset, Teleport, Rotation, Unknown(9)] {
                let mut view = LocalViewPose::new(Vec3::ZERO, aim);
                let mut settings = CameraSettingsAuthority::default();
                settings.set_preserve_teleport_rotation(enabled);
                let previous_epoch = view.camera_reanchor_epoch();
                let mut anchor = None;
                apply_committed_control(
                    CommittedControlEvent::MovePlayer {
                        sequence: 1,
                        source_cohort: None,
                        movement: protocol::MovePlayerEvent {
                            yaw: 90.0,
                            pitch: 15.0,
                            mode,
                            ..Default::default()
                        },
                        resolved: client_world::ResolvedServerPosition {
                            position: [3.0, 64.0, 8.0],
                            surface_anchor: Some([3, 8]),
                        },
                    },
                    &mut view,
                    &mut settings,
                    &mut anchor,
                );
                assert_ne!(view.camera_reanchor_epoch(), previous_epoch);
                assert_eq!(view.eye_translation(), Vec3::new(3.0, 64.0, 8.0));
                assert_eq!(anchor, Some([3, 8]));
                let expected = if enabled && mode == Teleport {
                    aim
                } else {
                    server_aim
                };
                assert!(view.rotation().abs_diff_eq(expected, 0.0001));
            }
        }
    }

    #[test]
    fn camera_context_reset_revokes_teleport_policy() {
        let mut settings = CameraSettingsAuthority::default();
        assert!(!settings.preserves_teleport_rotation());
        settings.set_preserve_teleport_rotation(true);
        let mut view = LocalViewPose::default();
        let previous_epoch = view.camera_reanchor_epoch();
        let mut anchor = None;
        apply_committed_control(
            CommittedControlEvent::ChangeDimension {
                sequence: 1,
                change: protocol::ChangeDimensionEvent {
                    dimension: 1,
                    ..Default::default()
                },
                resolved: client_world::ResolvedServerPosition {
                    position: [3.0, 64.0, 8.0],
                    surface_anchor: Some([3, 8]),
                },
            },
            &mut view,
            &mut settings,
            &mut anchor,
        );
        assert_ne!(view.camera_reanchor_epoch(), previous_epoch);
        assert_eq!(view.eye_translation(), Vec3::new(3.0, 64.0, 8.0));
        assert_eq!(anchor, Some([3, 8]));
        assert!(!settings.preserves_teleport_rotation());
    }
    #[test]
    fn only_ready_respawn_reanchors_camera_motion_history() {
        let mut settings = CameraSettingsAuthority::default();
        let mut view = LocalViewPose::default();
        let mut anchor = None;
        for state in [0, 1] {
            let previous_epoch = view.camera_reanchor_epoch();
            apply_committed_control(
                CommittedControlEvent::Respawn {
                    sequence: u64::from(state) + 1,
                    respawn: protocol::RespawnEvent {
                        state,
                        position: [3.0, 64.0, 8.0],
                        runtime_entity_id: 1,
                    },
                    resolved: client_world::ResolvedServerPosition {
                        position: [3.0, 64.0, 8.0],
                        surface_anchor: None,
                    },
                },
                &mut view,
                &mut settings,
                &mut anchor,
            );
            assert_eq!(view.camera_reanchor_epoch() != previous_epoch, state == 1);
        }
    }
}

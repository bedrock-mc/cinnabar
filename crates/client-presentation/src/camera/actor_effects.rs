//! Actor-clock damage rotation composed onto the rendered player camera.

use bevy::prelude::{Mat4, Quat, Query, Res, SystemSet, Transform, With};

use super::{CameraSettingsAuthority, FirstPersonHandMotion, FlyCamera, ServerCameraView};
use crate::observations::WorldObservation;
use semantic_input::PerspectiveMode;

/// Completes actor camera effects before an explicit rendered-pose override.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActorCameraEffects;

/// Applies completed actor counters before the accepted hand bob, without changing gameplay aim.
pub fn apply_actor_damage_camera_rotation(
    settings: Res<CameraSettingsAuthority>,
    hand: Res<FirstPersonHandMotion>,
    server: Res<ServerCameraView>,
    world: Option<WorldObservation<'_>>,
    partial_tick: f32,
    mut cameras: Query<&mut Transform, With<FlyCamera>>,
) {
    if !server.renders_first_person(settings.perspective() == PerspectiveMode::FirstPerson) {
        return;
    }
    let alpha = if partial_tick.is_finite() {
        partial_tick.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let damage = world
        .and_then(|world| world.stream)
        .and_then(|stream| stream.authority().actor(stream.local_player_runtime_id()))
        .map_or(Quat::IDENTITY, |actor| {
            let death_ticks = actor
                .attributes
                .get("minecraft:health")
                .filter(|health| health.current.ceil() < 1.0)
                .map(|_| f32::from(actor.status.native_death_ticks()) + alpha);
            let hurt_progress = if actor.status.hurt_time > 0 {
                (f32::from(actor.status.hurt_time) - alpha)
                    / f32::from(client_world::HURT_DURATION_TICKS)
            } else {
                0.0
            };
            damage_rotation(
                death_ticks,
                hurt_progress,
                actor.status.hurt_direction.unwrap_or(0.0).to_radians(),
                settings.feel().damage_bob,
            )
        });
    let effect = Mat4::from_quat(damage) * hand.bob.matrix().inverse();
    if effect == Mat4::IDENTITY || !effect.is_finite() {
        return;
    }
    for mut camera in &mut cameras {
        *camera = Transform::from_matrix(camera.to_matrix() * effect);
    }
}

/// Returns native camera-local damage rotation from sampled actor state.
fn damage_rotation(
    death_ticks: Option<f32>,
    hurt_progress: f32,
    hurt_angle: f32,
    strength: f32,
) -> Quat {
    let mut rotation = Quat::IDENTITY;
    if let Some(ticks) = death_ticks.filter(|ticks| *ticks > 0.0) {
        rotation = Quat::from_rotation_z(-super::hurt::death_roll_degrees(ticks).to_radians());
    }
    if hurt_progress > 0.0 {
        let phase =
            hurt_progress * hurt_progress * hurt_progress * hurt_progress * std::f32::consts::PI;
        let shake = sim::minecraft_sin(f64::from(phase)) as f32;
        let angle = (shake * super::hurt::HURT_TILT_DEGREES * strength).to_radians();
        rotation *= Quat::from_rotation_y(hurt_angle)
            * Quat::from_rotation_z(angle)
            * Quat::from_rotation_y(-hurt_angle);
    }
    rotation
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Vec3;

    #[test]
    fn actor_half_hurt_rolls_positive_z_and_direction_rotates_its_axis() {
        let right = damage_rotation(None, 0.5, 0.0, 1.0) * Vec3::X;
        assert!(right.y > 0.047 && right.y < 0.048);
        assert!(right.z.abs() < 1e-6);
        let up = damage_rotation(None, 0.5, std::f32::consts::FRAC_PI_2, 1.0) * Vec3::Y;
        assert!(up.z > 0.047 && up.z < 0.048);
        assert!(up.x.abs() < 1e-6);
    }

    #[test]
    fn native_death_counter_continues_beyond_the_body_clock() {
        let right = damage_rotation(Some(200.0), 0.0, 0.0, 1.0) * Vec3::X;
        assert!((right.x - 20.0_f32.to_radians().cos()).abs() < 1e-6);
        assert!((right.y + 20.0_f32.to_radians().sin()).abs() < 1e-6);
        assert_eq!(damage_rotation(Some(-1.0), 0.0, 0.0, 1.0), Quat::IDENTITY);
    }

    #[test]
    fn damage_bob_scale_does_not_disable_the_independent_death_rotation() {
        assert_eq!(damage_rotation(None, 0.5, 0.0, 0.0), Quat::IDENTITY);
        assert_eq!(
            damage_rotation(Some(200.0), 0.5, 0.0, 0.0),
            damage_rotation(Some(200.0), 0.0, 0.0, 1.0)
        );
    }
}

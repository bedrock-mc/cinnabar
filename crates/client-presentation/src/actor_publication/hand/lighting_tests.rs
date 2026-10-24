use super::*;
use bevy::math::{EulerRot, Quat, Vec3};

#[test]
fn java_fixed_lights_follow_world_look_and_bob_but_not_arm_sway() {
    let mut motion = crate::camera::FirstPersonHandMotion {
        hurt: Mat4::from_rotation_z(0.2),
        sway_pitch_radians: 0.7,
        sway_yaw_radians: -0.3,
        ..Default::default()
    };
    motion.bob.pitch_radians = 0.1;
    let look = Quat::from_euler(EulerRot::YXZ, 2.0, -0.4, 0.0);
    let lights = java_light_matrix(Some(&motion), look);
    let expected = motion.hurt
        * motion.bob.matrix()
        * Mat4::from_rotation_x(0.4)
        * Mat4::from_rotation_y(std::f32::consts::PI - 2.0);
    assert!(lights.abs_diff_eq(expected, 1e-6));
    motion.sway_pitch_radians = -0.8;
    motion.sway_yaw_radians = 0.9;
    assert_eq!(lights, java_light_matrix(Some(&motion), look));
    let forward = java_light_matrix(None, Quat::from_rotation_y(std::f32::consts::PI));
    assert!(
        forward
            .transform_vector3(Vec3::Z)
            .abs_diff_eq(Vec3::Z, 1e-6)
    );
}

#[test]
fn legacy_sample_replaces_separate_bob_and_hurt_before_hand_sway() {
    let effect = render_model::legacy_combat::legacy_hurt_matrix(5, 90.0, None, 0.5, 1.0)
        * render_model::legacy_combat::LegacyCameraEndpoints {
            walked: [0.6, 0.72],
            bob: [0.04, 0.064],
            fall_degrees: [0.0, 1.2],
        }
        .bob_matrix(0.5);
    let motion = crate::camera::FirstPersonHandMotion {
        legacy_effect: Some(effect),
        hurt: Mat4::from_rotation_z(0.9),
        bob: crate::camera::ViewEffect {
            translation: Vec3::X,
            roll_radians: 0.7,
            pitch_radians: 0.2,
        },
        sway_pitch_radians: 0.04,
        sway_yaw_radians: -0.06,
    };
    let expected = effect
        * Mat4::from_rotation_x(motion.sway_pitch_radians)
        * Mat4::from_rotation_y(motion.sway_yaw_radians);
    assert_eq!(hand_motion_matrix(&motion), expected);
    assert!(
        !hand_motion_matrix(&motion).abs_diff_eq(effect * motion.hurt * motion.bob.matrix(), 1e-3)
    );
}

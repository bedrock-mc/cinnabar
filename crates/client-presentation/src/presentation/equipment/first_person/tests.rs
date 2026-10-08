use super::*;
use bevy::math::Quat;

#[test]
fn block_idle_keeps_centered_cube_geometry_and_native_camera_yaw() {
    let idle = block_pose(ItemAnimationState::default()).unwrap();
    assert_eq!(idle.axis_scale, render_model::UNIT_AXIS_SCALE);
    assert!((idle.translation_scale[3] - FIRST_PERSON_ITEM_SCALE).abs() < 1e-6);
    let center = Vec3::from_array(idle.translation_scale[..3].try_into().unwrap());
    assert!(center.abs_diff_eq(CAMERA_ANCHOR, 1e-6));
    let rotation = bevy::math::Quat::from_array(idle.rotation);
    let native_yaw = bevy::math::Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
    let half_extent = FIRST_PERSON_ITEM_SCALE * 0.5;
    // All eight centered native cube corners must retain their handedness and orientation.
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-1.0, 1.0] {
                let corner = Vec3::new(x, y, z) * half_extent;
                let actual = center + rotation * corner;
                let expected = CAMERA_ANCHOR + native_yaw * corner;
                assert!(actual.abs_diff_eq(expected, 1e-6));
            }
        }
    }
}

#[test]
fn sprite_idle_uses_a_camera_pose_not_the_third_person_grip() {
    let idle = sprite_pose(ItemAnimationState::default()).unwrap();
    // Native legacy icon scale: camera scale times default item scale. Not the grip scale.
    assert!((idle.translation_scale[3] - 0.6).abs() < 1e-6);
    assert_eq!(idle.axis_scale, render_model::UNIT_AXIS_SCALE);
    assert!(idle.translation_scale[0] > 0.4);
    assert!(idle.translation_scale[1] < 0.0);
    assert!(idle.translation_scale[2] < -0.4);
    assert_ne!(
        idle.rotation,
        render_model::equipment::held_sprite_display(false)
            .rotation
            .to_array()
    );
}

#[test]
fn camera_item_keeps_swing_and_equip_animation() {
    for pose in [sprite_pose as fn(_) -> _, block_pose] {
        let idle = pose(ItemAnimationState::default()).unwrap();
        let lowered = pose(ItemAnimationState {
            arm_height: 0.0,
            ..Default::default()
        })
        .unwrap();
        assert!((lowered.translation_scale[1] - idle.translation_scale[1] + 0.6).abs() < 1e-6);
        assert_eq!(idle.rotation, lowered.rotation);
        for attack_time in [0.1, 0.25, 0.5, 0.75] {
            let swinging = pose(ItemAnimationState {
                attack_time,
                ..Default::default()
            })
            .unwrap();
            assert!(swinging.is_finite());
            assert_ne!(swinging.rotation, idle.rotation);
            assert_ne!(swinging.translation_scale, idle.translation_scale);
        }
    }
}

#[test]
fn nonfinite_item_observations_do_not_reach_the_renderer() {
    for pose in [sprite_pose as fn(_) -> _, block_pose] {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(
                pose(ItemAnimationState {
                    attack_time: invalid,
                    ..Default::default()
                })
                .is_none()
            );
            assert!(
                pose(ItemAnimationState {
                    arm_height: invalid,
                    ..Default::default()
                })
                .is_none()
            );
        }
    }
}

fn apply(bone: RenderBoneTransform, point: Vec3) -> Vec3 {
    Vec3::from_array(bone.translation_scale[..3].try_into().unwrap())
        + Quat::from_array(bone.rotation) * (point * bone.translation_scale[3])
}

#[test]
fn offhand_flat_sprite_matches_native_uv_labelled_pixel_corners() {
    for [width, height] in [[16, 16], [32, 16], [8, 16], [32, 32]] {
        let long = f32::from(width.max(height));
        let bone = offhand_sprite_pose(false, [width, height]).unwrap();
        assert!((bone.translation_scale[3] - 1.0).abs() < 1e-6);
        let native = Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2)
            * Mat4::from_translation(Vec3::new(-1.25, -1.125, 0.0))
            * Mat4::from_rotation_z(-80.0_f32.to_radians())
            * Mat4::from_rotation_y(-20.0_f32.to_radians())
            * Mat4::from_translation(Vec3::new(-0.3125, 0.25, -0.03125));
        // Read actual front/back vertices and UV-labelled texel corners from the
        // production mesh. Native front depth is 0; its back is 1 texel.
        let pixels = vec![255; usize::from(width) * usize::from(height) * 4];
        let mesh = render_model::held_sprite_vertices(
            usize::from(width),
            usize::from(height),
            &pixels,
            [0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        for vertex in mesh.iter().filter(|vertex| vertex.normal[2] != 0.0) {
            let depth = if vertex.normal[2] < 0.0 { 1.0 } else { 0.0 };
            let native_pixel = Vec3::new(
                vertex.uv[0] * f32::from(width),
                depth,
                vertex.uv[1] * f32::from(height),
            ) / long;
            assert!(
                apply(bone, Vec3::from_array(vertex.position))
                    .abs_diff_eq(native.transform_point3(native_pixel), 2e-6)
            );
        }
    }
}

#[test]
fn offhand_hand_equipped_retains_native_pixel_size_and_rotation_order() {
    let bone = offhand_sprite_pose(true, [32, 16]).unwrap();
    assert!((bone.translation_scale[3] - 2.0).abs() < 1e-6);
    let native = Mat4::from_translation(Vec3::new(-0.6875, -0.125, -1.53125))
        * Mat4::from_rotation_y(-10.0_f32.to_radians())
        * Mat4::from_rotation_x(70.0_f32.to_radians())
        * Mat4::from_rotation_z(80.0_f32.to_radians());
    for column in [0.0, 11.0, 32.0] {
        for row in [0.0, 7.0, 16.0] {
            for depth in [0.0, 1.0] {
                let held = Vec3::new(-column, 16.0 - row, -depth) / 32.0;
                let native_pixel =
                    Vec3::new(column, depth, row) / f32::from(NATIVE_ICON_TEXELS_PER_UNIT);
                assert!(apply(bone, held).abs_diff_eq(native.transform_point3(native_pixel), 2e-6));
            }
        }
    }
}

#[test]
fn offhand_block_uses_left_presentation_default_without_sprite_correction() {
    let bone = offhand_pose(false, true).unwrap();
    let anchor = Vec3::new(-0.56, -0.52, -0.72);
    let rotation = Quat::from_rotation_y(135.0_f32.to_radians());
    for x in [-0.5, 0.5] {
        for y in [-0.5, 0.5] {
            for z in [-0.5, 0.5] {
                let corner = Vec3::new(x, y, z);
                assert!(apply(bone, corner).abs_diff_eq(anchor + rotation * (corner * 0.4), 1e-6));
            }
        }
    }
    assert_eq!(offhand_pose(true, true), Some(bone));
    assert_eq!(
        offhand_pose(false, false),
        offhand_sprite_pose(false, [NATIVE_ICON_TEXELS_PER_UNIT; 2])
    );
    for size in [[0, 16], [16, 0], [0, 0]] {
        assert!(offhand_sprite_pose(false, size).is_none());
        assert!(offhand_sprite_pose(true, size).is_none());
    }
}

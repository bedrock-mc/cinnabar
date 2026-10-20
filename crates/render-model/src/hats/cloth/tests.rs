//! Synthetic cloth behavior regressions, independent of private catalog art.
use super::*;
use crate::{ActorRigSurface, ActorRigVertex, EntityRigId};

#[test]
fn admitted_cloth_expands_all_pose_channels_and_runs_through_the_native_rig() {
    let item = crate::hats::fixture::cloth();
    let model = rig(item);
    assert!(model.has_cloth());
    let root = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [0.0, 1.5, 0.0, 1.0],
        axis_scale: UNIT_AXIS_SCALE,
    };
    let base = model.pose(root, super::super::Motion::default());
    assert_eq!(base.len(), 11);
    let mut state = ClothState::default();
    for _ in 0..30 {
        let (_, pose) = state
            .pose(
                item,
                [base.clone(), base.clone()],
                [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
                (1.0 / 20.0, 0.5),
                &[],
            )
            .unwrap();
        assert!(pose.iter().all(|bone| bone.is_finite()));
    }
}

#[test]
fn scaled_rest_lengths_and_frame_poses_preserve_attachment_endpoints() {
    let item = crate::hats::fixture::cloth();
    let model = rig(item);
    let cloth = model.cloth.as_ref().unwrap();
    let mut state = ClothState::default();
    for scale in [Vec3::splat(0.5), Vec3::splat(2.0), Vec3::new(0.7, 1.3, 0.9)] {
        let root = RenderBoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 1.5, 0.0, 1.0],
            axis_scale: [scale.x, scale.y, scale.z, 1.0],
        };
        let previous = model.pose(root, super::super::Motion::default());
        let mut current = previous.to_vec();
        for bone in &mut current {
            bone.translation_scale[0] += 0.1;
        }
        let current: Arc<[RenderBoneTransform]> = current.into();
        let actor = [
            [1.2, 0.0, 0.0, 2.0],
            [0.0, 0.8, 0.0, 0.0],
            [0.0, 0.0, 1.1, 0.0],
        ];
        for frame in 0..120 {
            let alpha = (frame % 3) as f32 * 0.4;
            let (a, b) = state
                .pose(
                    item,
                    [previous.clone(), current.clone()],
                    actor,
                    (1.0 / 120.0, alpha),
                    &[],
                )
                .unwrap();
            assert_eq!(&a[..cloth.first], &previous[..cloth.first]);
            assert_eq!(&b[..cloth.first], &current[..cloth.first]);
            assert_eq!(&a[cloth.first..], &b[cloth.first..]);
            let pivot = Vec3::from_array(model.geometry.bone_pivots[cloth.source]);
            let mut offset = cloth.rest[0] - pivot;
            offset.x *= 0.89;
            offset.z += REST_CLEARANCE;
            let pin = crate::actor::sampled_bone_offset(
                offset.to_array(),
                previous[cloth.source],
                current[cloth.source],
                actor,
                alpha,
            )
            .unwrap();
            assert!(state.points[0].abs_diff_eq(Vec3::from_array(pin.position), 1e-5));
            for c in &state.constraints {
                let expected =
                    ((cloth.rest[c.b] - cloth.rest[c.a]) * scale * Vec3::new(1.2, 0.8, 1.1))
                        .length();
                assert!((c.length - expected).abs() < 1e-5);
            }
        }
        let stretch = state
            .constraints
            .iter()
            .filter(|c| c.compliance < 0.001)
            .map(|c| state.points[c.a].distance(state.points[c.b]) / c.length - 1.0)
            .fold(0.0_f32, f32::max);
        assert!(
            stretch < 0.12,
            "scaled fabric lost its dimensions: {stretch}"
        );
    }
}

/// A rectangular fabric fixture whose neighbouring triangles repeat their shared corners.
fn fixture() -> (ClothRig, ActorRigGeometry) {
    let (nx, ny) = (6, 12);
    let points: Vec<_> = (0..=ny)
        .flat_map(|y| {
            (0..=nx).map(move |x| {
                [
                    -0.3125 + x as f32 * 0.625 / nx as f32,
                    1.4875 - y as f32 * 1.0625 / ny as f32,
                    0.2,
                ]
            })
        })
        .collect();
    let mut vertices = Vec::new();
    for y in 0..ny {
        for x in 0..nx {
            let a = y * (nx + 1) + x;
            for index in [a, a + nx + 1, a + 1, a + 1, a + nx + 1, a + nx + 2] {
                vertices.push(ActorRigVertex {
                    position: points[index],
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.0; 2],
                    back_uv: [0.0; 2],
                    bone_index: 1,
                    surface: ActorRigSurface::SINGLE_FACE,
                    glint_origin: [0.0; 2],
                });
            }
        }
    }
    let mut geometry = ActorRigGeometry::new(
        EntityRigId(0),
        vertices,
        vec![[0.0, 1.5, 0.0], [0.0, 1.4875, 0.2]],
    )
    .unwrap();
    let cloth = ClothRig::build(
        &Definition {
            bone: "fabric".into(),
            columns: nx,
            rows: ny,
        },
        &["body", "fabric"],
        &mut geometry,
        &mut vec![None, Some(0)],
    )
    .unwrap();
    (cloth, geometry)
}

#[test]
fn adjacent_triangles_share_the_same_simulated_particle() {
    let (cloth, geometry) = fixture();
    assert_eq!(cloth.rest.len(), 91);
    for pair in geometry.vertices.as_chunks::<6>().0 {
        assert_eq!(pair[1].bone_index, pair[4].bone_index);
        assert_eq!(pair[2].bone_index, pair[3].bone_index);
    }
}

#[test]
fn fabric_trails_motion_and_survives_run_stop_turn_jump() {
    let (cloth, _) = fixture();
    let mut state = ClothState {
        points: cloth.rest.clone(),
        old: cloth.rest.clone(),
        normals: vec![Vec3::Z; cloth.rest.len()],
        constraints: cloth.constraints.clone(),
        ..Default::default()
    };
    let mut center = Vec3::ZERO;
    let mut trail = 0.0_f32;
    for frame in 0..1800 {
        let t = frame as f32 * STEP;
        let speed = if (1.0..5.0).contains(&t) { 5.6 } else { 0.0 };
        center.z += speed * STEP;
        let yaw = if t > 7.0 {
            (t - 7.0).min(1.0) * std::f32::consts::PI
        } else {
            0.0
        };
        let rotation = Quat::from_rotation_y(yaw);
        let jump = if (9.0..10.0).contains(&t) {
            ((t - 9.0) * std::f32::consts::PI).sin() * 0.6
        } else {
            0.0
        };
        let pins: Vec<_> = cloth.rest[..=cloth.columns]
            .iter()
            .map(|p| center + rotation * (*p) + Vec3::Y * jump)
            .collect();
        let anchor_velocity = (pins[0] - state.points[0]) / STEP;
        state.step(&cloth, &pins, &[], Vec3::ZERO, anchor_velocity);
        assert!(state.points.iter().all(|p| p.is_finite()));
        assert_eq!(&state.points[..=cloth.columns], pins.as_slice());
        if (3.0..5.0).contains(&t) {
            trail = trail.max(pins[0].z - state.points.last().unwrap().z);
        }
    }
    assert!(
        trail > 0.25,
        "moving attachments must leave the hem trailing: {trail}"
    );
    let max_stretch = cloth
        .constraints
        .iter()
        .filter(|c| c.compliance < 0.001)
        .map(|c| state.points[c.a].distance(state.points[c.b]) / c.length - 1.0)
        .fold(0.0_f32, f32::max);
    assert!(
        max_stretch < 0.12,
        "the settled fabric must retain its dimensions: {max_stretch}"
    );
}

#[test]
fn torso_contact_exits_through_the_back_after_rotation() {
    let bone = RenderBoneTransform {
        rotation: Quat::from_rotation_y(1.2).to_array(),
        translation_scale: [0.0, 1.5, 0.0, 1.0],
        axis_scale: UNIT_AXIS_SCALE,
    };
    let collider = Collider::biped("body", bone, Mat4::IDENTITY).unwrap();
    let mut point = collider.world.transform_point3(Vec3::new(0.0, -0.4, 0.0));
    collider.project(&mut point);
    assert!(collider.inverse.transform_point3(point).z > collider.max.z);
}

#[test]
fn idle_fabric_clears_standard_chest_armor() {
    let item = crate::hats::fixture::cloth();
    let model = rig(item);
    let root = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [0.0, 1.5, 0.0, 1.0],
        axis_scale: UNIT_AXIS_SCALE,
    };
    let base = model.pose(root, super::super::Motion::default());
    let collider = Collider::biped("body", root, Mat4::IDENTITY).unwrap();
    let actor = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let mut state = ClothState::default();
    for _ in 0..480 {
        state
            .pose(
                item,
                [base.clone(), base.clone()],
                actor,
                (STEP, 1.0),
                std::slice::from_ref(&collider),
            )
            .unwrap();
    }
    // Standard chest armor ends three pixels behind the torso, including its inflation.
    for point in state.points.iter().filter(|p| p.y > 0.75 && p.y < 1.5) {
        assert!(
            point.z > 3.0 / 16.0 + 0.015,
            "fabric entered the armor: {point}"
        );
    }
}

#[test]
fn jumping_and_landing_do_not_throw_the_hem_up_the_players_back() {
    let item = crate::hats::fixture::dense_cloth();
    let model = rig(item);
    let root = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [0.0, 1.5, 0.0, 1.0],
        axis_scale: UNIT_AXIS_SCALE,
    };
    let base = model.pose(root, super::super::Motion::default());
    let mut state = ClothState::default();
    let mut peak = 0.0_f32;
    for frame in 0..480 {
        let t = frame as f32 * STEP - 1.0;
        let height = if (0.0..0.5).contains(&t) {
            8.4 * t - 16.8 * t * t
        } else {
            0.0
        };
        let actor = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, height],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let collider = Collider::biped("body", root, placement(actor)).unwrap();
        state
            .pose(
                item,
                [base.clone(), base.clone()],
                actor,
                (STEP, 1.0),
                &[collider],
            )
            .unwrap();
        let hem = state.points.last().unwrap();
        peak = peak.max(hem.y - height);
        assert!(state.points.iter().all(|point| point.is_finite()));
    }
    println!("Peak hem height relative to feet: {peak}");
    assert!(
        peak < 0.75,
        "jump/landing folded the hem above the waist: {peak}"
    );
    assert!(
        (state.points.last().unwrap().y - 0.425).abs() < 0.10,
        "fabric did not settle after landing"
    );
}

#[test]
fn head_contacts_leave_the_neck_strip_at_its_rest_length() {
    let item = crate::hats::fixture::dense_cloth();
    let model = rig(item);
    let cloth = model.cloth.as_ref().unwrap();
    let root = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [0.0, 1.5, 0.0, 1.0],
        axis_scale: UNIT_AXIS_SCALE,
    };
    let base = model.pose(root, super::super::Motion::default());
    let actor = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let colliders = [
        Collider::biped("head", root, Mat4::IDENTITY).unwrap(),
        Collider::biped("body", root, Mat4::IDENTITY).unwrap(),
    ];
    let mut state = ClothState::default();
    for _ in 0..240 {
        state
            .pose(
                item,
                [base.clone(), base.clone()],
                actor,
                (STEP, 1.0),
                &colliders,
            )
            .unwrap();
        for edge in state.constraints.iter().filter(|edge| {
            edge.a <= cloth.columns && edge.b > cloth.columns && edge.b <= 2 * cloth.columns + 1
        }) {
            let stretch = state.points[edge.a].distance(state.points[edge.b]) / edge.length;
            assert!(
                stretch < 1.15,
                "head contact stretched the collar strip: {stretch}"
            );
        }
    }
}

#[test]
fn view_pose_transfer_preserves_drape_and_velocity() {
    let item = crate::hats::fixture::dense_cloth();
    let model = rig(item);
    let root = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [0.0, 1.5, 0.0, 1.0],
        axis_scale: UNIT_AXIS_SCALE,
    };
    let base = model.pose(root, super::super::Motion::default());
    let mut state = ClothState::default();
    let mut actor = [
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 3.0],
    ];
    for frame in 0..120 {
        actor[2][3] = 3.0 + frame as f32 / 60.0;
        state
            .pose(item, [base.clone(), base.clone()], actor, (STEP, 1.0), &[])
            .unwrap();
    }
    let points = state.points.clone();
    let velocities: Vec<_> = state
        .points
        .iter()
        .zip(&state.old)
        .map(|(a, b)| *a - *b)
        .collect();
    let old_frame = state.previous_frame.unwrap();
    let target_root = RenderBoneTransform {
        rotation: (Quat::from_rotation_y(0.9) * Quat::from_rotation_x(0.45)).to_array(),
        translation_scale: [0.0, 1.38, -0.06, 1.0],
        ..root
    };
    let target = model.pose(target_root, super::super::Motion::default());
    state.reanchor();
    state
        .pose(item, [target.clone(), target], actor, (0.0, 1.0), &[])
        .unwrap();
    let transfer = state.previous_frame.unwrap() * old_frame.inverse();
    for (i, point) in state.points.iter().enumerate() {
        assert!(
            point.abs_diff_eq(transfer.transform_point3(points[i]), 1e-5),
            "view change reset or left behind point {i}"
        );
        assert!(
            (*point - state.old[i]).abs_diff_eq(transfer.transform_vector3(velocities[i]), 1e-5),
            "view change injected velocity at point {i}"
        );
    }
}

#[test]
fn sprinting_keeps_a_bounded_drape_and_settles_after_stopping() {
    let item = crate::hats::fixture::dense_cloth();
    let model = rig(item);
    let root = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [0.0, 1.5, 0.0, 1.0],
        axis_scale: UNIT_AXIS_SCALE,
    };
    let base = model.pose(root, super::super::Motion::default());
    let mut state = ClothState::default();
    let mut peak_lift = 0.0_f32;
    let mut trail = 0.0_f32;
    for frame in 0..720 {
        let z = -5.6 * (frame as f32 * STEP).min(4.0);
        let actor = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, z],
        ];
        let colliders = [
            Collider::biped("body", root, placement(actor)).unwrap(),
            Collider::biped("head", root, placement(actor)).unwrap(),
        ];
        state
            .pose(
                item,
                [base.clone(), base.clone()],
                actor,
                (STEP, 1.0),
                &colliders,
            )
            .unwrap();
        let hem = state.points.last().unwrap();
        peak_lift = peak_lift.max(hem.y - 0.425);
        trail = trail.max(hem.z - z - 0.2625);
    }
    println!("Sprint peak hem lift: {peak_lift}; trail: {trail}");
    assert!(trail > 0.1, "the fabric must still respond to running");
    assert!(
        peak_lift < 0.65,
        "running folded the cape too high: {peak_lift}"
    );
    assert!(
        (state.points.last().unwrap().y - 0.425).abs() < 0.1,
        "fabric did not settle after stopping"
    );
}

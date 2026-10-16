use super::*;

/// A standard head pivot in block coordinates, with an arbitrary evaluated orientation.
fn head(rotation: Quat) -> RenderBoneTransform {
    RenderBoneTransform {
        rotation: rotation.to_array(),
        translation_scale: [0.0, 1.5, 0.0, 1.0],
        axis_scale: crate::UNIT_AXIS_SCALE,
    }
}

#[test]
fn original_model_has_a_flexible_continuous_surface_and_valid_art() {
    let model = rig(Hat::BunnyEars);
    assert!(model.flex.iter().filter(|weight| **weight > 0.0).count() >= 16);
    assert!(model.texture.is_valid());
    assert_eq!(model.pixels.len(), crate::STANDARD_SKIN_SIDE.pow(2) * 4);
    assert!(
        model
            .geometry
            .vertices
            .iter()
            .all(|vertex| { vertex.uv.into_iter().all(|uv| (0.0..=1.0).contains(&uv)) })
    );
    assert!(model.rest_vertices.iter().any(|v| v.position[1] > 2.5));
    assert!(
        model.rest_vertices.as_chunks::<3>().0.iter().all(|face| {
            face.iter().all(|v| v.position[0] > 0.0) || face.iter().all(|v| v.position[0] < 0.0)
        }),
        "no headband bridges the two ear roots"
    );
    let bottom = model
        .rest_vertices
        .iter()
        .map(|v| v.position[1])
        .fold(f32::INFINITY, f32::min);
    assert!(
        (bottom - 2.0).abs() < 1e-5,
        "ears sit directly on the head without a connecting band"
    );
}

#[test]
fn forward_wind_folds_only_the_upper_half_behind_the_head_at_every_orientation() {
    let model = rig(Hat::BunnyEars);
    for rotation in [
        Quat::IDENTITY,
        Quat::from_rotation_y(1.3),
        Quat::from_rotation_x(-0.4),
    ] {
        let rest = model.pose(head(rotation), 0.0);
        let folded = model.pose(head(rotation), 1.0);
        for (index, weight) in model.flex.iter().enumerate() {
            if *weight == 0.0 {
                assert_eq!(rest[index], folded[index]);
                continue;
            }
            let local_up = rotation.inverse() * Quat::from_array(folded[index].rotation) * Vec3::Y;
            assert!(
                local_up.z > 0.0,
                "upper rings bend behind, not toward the face"
            );
            assert!(local_up.y > 0.0, "tip never inverts");
        }
    }
}

#[test]
fn forward_speed_ignores_strafing_and_vertical_motion_and_tracks_look_yaw() {
    assert!((forward_speed([0.0, 7.0, 0.25], 0.0) - 5.0).abs() < 1e-5);
    assert!((forward_speed([-0.25, 0.0, 0.0], 90.0) - 5.0).abs() < 1e-5);
    assert!(forward_speed([0.25, 0.0, 0.0], 0.0).abs() < 1e-5);
    assert!(forward_speed([0.0, 0.0, -0.25], 0.0) < 0.0);
}

#[test]
fn wind_scales_with_forward_speed_eases_back_and_is_frame_rate_independent() {
    let walk = EarWind::default().advance(3.0, 1.0);
    let run = EarWind::default().advance(6.0, 1.0);
    assert!(run > walk && run <= 65.0_f32.to_radians());
    let mut slow = EarWind::default();
    let mut fast = EarWind::default();
    for _ in 0..30 {
        slow.advance(5.0, 1.0 / 30.0);
    }
    for _ in 0..120 {
        fast.advance(5.0, 1.0 / 120.0);
    }
    assert!((slow.bend - fast.bend).abs() < 1e-4);
    let before = fast.bend;
    let after = fast.advance(-3.0, 0.1);
    assert!(after > 0.0 && after < before);
    for _ in 0..120 {
        fast.advance(0.0, 1.0 / 30.0);
    }
    assert_eq!(fast.bend, 0.0);
    assert_eq!(fast.advance(f32::NAN, f32::INFINITY), 0.0);
}

#[test]
fn settled_headwear_reuses_its_pose_and_retains_head_scale() {
    let mut cache = HatPoseCache::default();
    let mut head = head(Quat::IDENTITY);
    head.translation_scale[3] = 0.5;
    let first = cache.pose(Hat::BunnyEars, head, 0.0);
    let second = cache.pose(Hat::BunnyEars, head, 0.0);
    assert!(Arc::ptr_eq(&first, &second));
    for bone in &*first {
        assert_eq!(bone.translation_scale[3], 0.5);
    }
    let changed = cache.pose(Hat::BunnyEars, head, 0.5);
    assert!(!Arc::ptr_eq(&first, &changed));
}

#[test]
fn shared_surface_edges_stay_welded_and_joint_angles_form_a_curve() {
    let model = rig(Hat::BunnyEars);
    for bend in [0.0, 0.5, 1.0] {
        let pose = model.pose(head(Quat::IDENTITY), bend);
        let mut shared = std::collections::HashMap::<[u32; 3], Vec3>::new();
        let mut duplicate_corners = 0;
        for vertex in &*model.geometry.vertices {
            let index = vertex.bone_index as usize;
            let bone = pose[index];
            let position = Vec3::from_slice(&bone.translation_scale[..3])
                + Quat::from_array(bone.rotation)
                    * (Vec3::from_array(vertex.position)
                        - Vec3::from_array(model.geometry.bone_pivots[index]));
            if let Some(previous) = shared.insert(vertex.position.map(f32::to_bits), position) {
                assert!(
                    previous.distance(position) < 1e-6,
                    "a shared surface edge must never split at a joint"
                );
                duplicate_corners += 1;
            }
        }
        assert!(duplicate_corners > 100);
        for (index, parent) in model.parents.iter().enumerate() {
            if let Some(parent) = parent {
                let difference = Quat::from_array(pose[index].rotation)
                    .angle_between(Quat::from_array(pose[*parent].rotation));
                assert!(
                    difference < 0.15,
                    "wind curvature is distributed across small joint angles"
                );
            }
        }
    }
}

#[test]
fn blockbench_project_and_runtime_export_share_the_same_surface_and_joint_pivots() {
    for &hat in Hat::ALL {
        let source = model::source(hat);
        let project: serde_json::Value = serde_json::from_str(source.project).unwrap();
        let exported: serde_json::Value = serde_json::from_str(source.geometry).unwrap();
        assert_eq!(
            project["model_identifier"],
            format!("geometry.cinnabar.{}", hat.id())
        );
        let geometry = &exported["minecraft:geometry"][0];
        assert_eq!(
            project["model_identifier"],
            geometry["description"]["identifier"]
        );
        for group in project["groups"].as_array().unwrap() {
            assert!(
                group["rotation"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|n| n.as_f64() == Some(0.0)),
                "headwear group transforms must be baked before export"
            );
            let bone = geometry["bones"]
                .as_array()
                .unwrap()
                .iter()
                .find(|bone| bone["name"] == group["name"])
                .expect("exported project joint");
            assert_eq!(bone["pivot"], group["origin"]);
        }
        for mesh in project["elements"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|element| element["type"] == "mesh")
        {
            for field in ["origin", "rotation"] {
                assert!(
                    mesh[field]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|n| n.as_f64() == Some(0.0)),
                    "headwear mesh transforms must be baked before export"
                );
            }
            let surface = geometry["bones"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|bone| bone.get("poly_mesh"))
                .find(|surface| {
                    surface["positions"].as_array().unwrap().len()
                        == mesh["vertices"].as_object().unwrap().len()
                        && mesh["vertices"]
                            .as_object()
                            .unwrap()
                            .values()
                            .all(|position| {
                                surface["positions"].as_array().unwrap().contains(position)
                            })
                })
                .expect("runtime surface matches the editable Blockbench mesh");
            let positions = surface["positions"].as_array().unwrap();
            assert!(positions.len() > 100);
            assert_eq!(
                surface["polys"].as_array().unwrap().len(),
                mesh["faces"].as_object().unwrap().len()
            );
            for face in mesh["faces"].as_object().unwrap().values() {
                assert!(
                    surface["polys"].as_array().unwrap().iter().any(|polygon| {
                        let corners = polygon.as_array().unwrap();
                        let authored = face["vertices"].as_array().unwrap();
                        authored.len() == corners.len()
                            && authored.iter().zip(corners).all(|(key, corner)| {
                                let key = key.as_str().unwrap();
                                let indices = corner.as_array().unwrap();
                                mesh["vertices"][key]
                                    == surface["positions"][indices[0].as_u64().unwrap() as usize]
                                    && face["uv"][key]
                                        == surface["uvs"][indices[2].as_u64().unwrap() as usize]
                            })
                    }),
                    "Blockbench faces and UVs must be exported together"
                );
            }
        }
    }
}

#[test]
fn crown_has_its_own_art_and_keeps_its_shape_in_wind() {
    let crown = rig(Hat::Crown);
    let ears = rig(Hat::BunnyEars);
    assert_ne!(crown.geometry.id, ears.geometry.id);
    assert!(crown.texture.is_valid());
    let rest = crown.pose(head(Quat::IDENTITY), 0.0);
    assert_eq!(rest, crown.pose(head(Quat::IDENTITY), 1.0));
    let bottom = crown
        .rest_vertices
        .iter()
        .map(|v| v.position[1])
        .fold(f32::INFINITY, f32::min);
    assert!(
        (bottom - 2.0).abs() < 1e-5,
        "crown rests on the top of the head"
    );
    assert!(
        crown
            .rest_vertices
            .iter()
            .all(|vertex| { vertex.position[0].abs() <= 0.25 && vertex.position[2].abs() <= 0.25 }),
        "the complete crown, including jewels, fits the head width and depth"
    );
    assert!(crown.rest_vertices.iter().any(|v| v.position[1] > 2.0));
}

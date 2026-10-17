use super::*;

/// Applies the same immutable vertex and bone transform used by world and HUD draw calls.
fn point(model: &HatRig, poses: &[RenderBoneTransform], vertex: &crate::ActorRigVertex) -> Vec3 {
    let index = vertex.bone_index as usize;
    let pose = poses[index];
    Vec3::from_slice(&pose.translation_scale[..3])
        + Quat::from_array(pose.rotation)
            * (Vec3::from_array(vertex.position)
                - Vec3::from_array(model.geometry.bone_pivots[index]))
}

#[test]
fn wings_loop_mirror_and_keep_shared_edges_welded_through_the_flap() {
    let model = rig(fixture::dragon_wings());
    let length = model.playback.as_ref().unwrap().length;
    let root = model.geometry.bone_pivots[model.head];
    let attachment = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [root[0], root[1], root[2], 1.0],
        axis_scale: crate::UNIT_AXIS_SCALE,
    };
    let mut tips = Vec::new();
    for seconds in [0.0, length * 0.25, length * 0.5, length * 0.75, length] {
        let pose = model.pose(
            attachment,
            Motion {
                seconds,
                ..Default::default()
            },
        );
        let mut shared = std::collections::HashMap::new();
        for vertex in &*model.geometry.vertices {
            let p = point(model, &pose, vertex);
            assert!(p.is_finite());
            if let Some(previous) = shared.insert(vertex.position.map(f32::to_bits), p) {
                assert!(
                    p.distance(previous) < 1e-6,
                    "shared membrane and rib vertices cannot separate"
                );
            }
        }
        for (key, p) in &shared {
            let mirror = [(-f32::from_bits(key[0])).to_bits(), key[1], key[2]];
            if let Some(q) = shared.get(&mirror) {
                assert!(
                    (p.x + q.x).abs() < 1e-5
                        && (p.y - q.y).abs() < 1e-5
                        && (p.z - q.z).abs() < 1e-5
                );
            }
        }
        let vertex = model
            .geometry
            .vertices
            .iter()
            .max_by(|a, b| a.position[0].total_cmp(&b.position[0]))
            .unwrap();
        tips.push(point(model, &pose, vertex));
    }
    assert!(
        tips[0].distance(tips[4]) < 1e-6,
        "loop closes without a jump"
    );
    assert!(
        tips[1].distance(tips[3]) > 0.2,
        "tips move visibly through a flap"
    );
    assert!(model.geometry.bone_pivots.len() < crate::MAX_RENDER_BONES_PER_ACTOR);
    let back = fixture::dragon_wings();
    let head = fixture::devil_horns();
    let slots = [
        ("head".into(), Some(head.id().into())),
        ("back".into(), Some(back.id().into())),
    ]
    .into();
    let resolved = cosmetics::resolve(&slots, |_| false);
    assert!(resolved.contains(&head) && resolved.contains(&back));
}

#[test]
fn cosmetic_motion_wraps_and_rigid_items_reuse_poses_when_time_changes() {
    let wings = fixture::dragon_wings();
    let mut motion = Motion::default();
    motion.advance(wings, 0.7);
    motion.advance(wings, 2.8);
    assert!((motion.seconds - 0.7).abs() < 1e-6);
    for invalid in [f32::NAN, f32::INFINITY, -1.0] {
        motion.advance(wings, invalid);
    }
    assert!((motion.seconds - 0.7).abs() < 1e-6);
    let attachment = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [0., 1.5, 0., 1.],
        axis_scale: crate::UNIT_AXIS_SCALE,
    };
    let mut cache = HatPoseCache::default();
    let first = cache.pose(wings, attachment, motion);
    assert!(Arc::ptr_eq(&first, &cache.pose(wings, attachment, motion)));
    motion.advance(wings, 0.3);
    assert!(!Arc::ptr_eq(&first, &cache.pose(wings, attachment, motion)));
    let crown = fixture::crown();
    let first = cache.pose(crown, attachment, Motion::default());
    assert!(Arc::ptr_eq(&first, &cache.pose(crown, attachment, motion)));
}

#[test]
fn malformed_clips_and_flex_profiles_are_rejected_before_item_admission() {
    let descriptor = fixture::dragon_wings().descriptor();
    let original: serde_json::Value =
        serde_json::from_slice(include_bytes!("dragon_wings.animation.json")).unwrap();
    let clip = "animation.cinnabar.dragon_wings.idle";
    let mut invalid = Vec::new();
    let mut value = original.clone();
    value["animations"][clip]["animation_length"] = 0.into();
    invalid.push(value);
    let mut value = original.clone();
    value["animations"][clip]["bones"]["unknown"] =
        value["animations"][clip]["bones"]["wing_left"].clone();
    invalid.push(value);
    let mut value = original.clone();
    value["animations"][clip]["bones"]["wing_left"]["rotation"]["0.0"] =
        serde_json::json!([0, 999, 0]);
    invalid.push(value);
    let mut value = original.clone();
    value["tip_flex"]["span"] = 0.into();
    invalid.push(value);
    let mut value = original.clone();
    value["tip_flex"]["joints"]["wing_right"] = 0.into();
    invalid.push(value);
    let mut value = original.clone();
    value["tip_flex"]["joints"]["missing"] = 1.into();
    invalid.push(value);
    let mut value = original;
    value["animations"] = serde_json::json!({});
    invalid.push(value);
    for definition in invalid {
        assert!(
            model::build(
                descriptor,
                include_str!("dragon_wings.geo.json"),
                include_bytes!("dragon_wings.png"),
                &serde_json::to_vec(&definition).unwrap()
            )
            .is_none()
        );
    }
}

#[test]
fn trailing_flex_moves_the_tips_but_preserves_the_rigid_mount() {
    let model = rig(fixture::dragon_wings());
    let mut definition: serde_json::Value =
        serde_json::from_slice(include_bytes!("dragon_wings.animation.json")).unwrap();
    definition.as_object_mut().unwrap().remove("tip_flex");
    let rigid = model::build(
        fixture::dragon_wings().descriptor(),
        include_str!("dragon_wings.geo.json"),
        include_bytes!("dragon_wings.png"),
        &serde_json::to_vec(&definition).unwrap(),
    )
    .unwrap();
    let attachment = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [0., 1.5, 0., 1.],
        axis_scale: crate::UNIT_AXIS_SCALE,
    };
    let motion = Motion {
        seconds: 2.1,
        ..Default::default()
    };
    let flexed = model.pose(attachment, motion);
    let unflexed = rigid.pose(attachment, motion);
    let mut moving_tips = 0;
    for (a, b) in model
        .geometry
        .vertices
        .iter()
        .zip(rigid.geometry.vertices.iter())
    {
        let distance = point(model, &flexed, a).distance(point(&rigid, &unflexed, b));
        if a.position[0].abs() <= 7.0 / 16.0 {
            assert!(distance < 1e-6);
        } else if a.position[0].abs() > 1.0 && distance > 0.02 {
            moving_tips += 1;
        }
    }
    assert!(
        moving_tips > 10,
        "tip flex must contribute beyond the shoulder flap"
    );
}

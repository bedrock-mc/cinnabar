use super::*;

/// Supplies four synthetic clips independently of production catalog artwork.
fn rig() -> Rig {
    let definition: Definition = serde_json::from_value(serde_json::json!({
        "idle":"idle", "sit":"sit", "trot":"trot", "gallop":"gallop", "trot_stride":1.6, "gallop_stride":2.6
    })).unwrap();
    let clips = ["idle", "sit", "trot", "gallop"].into_iter().map(|name| {
        (name.to_owned(), serde_json::from_value(serde_json::json!({"loop":true,"animation_length":1.0,"bones":{"root":{"rotation":{"0":[0,0,0],"1":[0,0,0]}}}})).unwrap())
    }).collect();
    Rig::build(definition, clips, &["root"]).unwrap()
}

#[test]
fn follower_accelerates_towards_real_owner_then_sits_without_sliding() {
    let mut follower = Follower::default();
    let rig = rig();
    let start = follower
        .advance(
            &rig,
            Vec3::ZERO,
            0.0,
            0.05,
            |_, _, _| Some(0.0),
            |_, _, delta| delta,
            |_, _| true,
        )
        .unwrap();
    let owner = Vec3::new(0.0, 0.0, 4.0);
    let next = follower
        .advance(
            &rig,
            owner,
            0.0,
            0.05,
            |_, _, _| Some(0.0),
            |_, _, delta| delta,
            |_, _| true,
        )
        .unwrap();
    assert!(
        next.feet.distance(start.feet) < 0.1,
        "owner movement must not rigidly drag the follower"
    );
    assert!(matches!(next.pose.gait, Gait::Trot));
    let mut settled = next;
    for _ in 0..200 {
        settled = follower
            .advance(
                &rig,
                owner,
                0.0,
                0.05,
                |_, _, _| Some(0.0),
                |_, _, delta| delta,
                |_, _| true,
            )
            .unwrap();
    }
    assert!(settled.feet.distance(owner) < 2.5);
    assert_eq!(settled.pose.gait, Gait::Sit);
    let still = follower
        .advance(
            &rig,
            owner,
            0.0,
            0.05,
            |_, _, _| Some(0.0),
            |_, _, delta| delta,
            |_, _| true,
        )
        .unwrap();
    assert!(still.feet.distance(settled.feet) < 0.001);
}

#[test]
fn follower_hops_one_block_and_returns_to_support() {
    let mut follower = Follower::default();
    let rig = rig();
    let terrain = |feet: Vec3, rise: f32, drop: f32| {
        let height = if feet.z > 0.0 { 1.0 } else { 0.0 };
        (height <= feet.y + rise + 0.001 && height >= feet.y - drop - 0.001).then_some(height)
    };
    follower
        .advance(
            &rig,
            Vec3::ZERO,
            0.0,
            0.05,
            terrain,
            |_, _, delta| delta,
            |_, _| true,
        )
        .unwrap();
    let mut highest = 0.0_f32;
    let mut last = None;
    for _ in 0..100 {
        let frame = follower
            .advance(
                &rig,
                Vec3::new(0.0, 1.0, 4.0),
                0.0,
                0.05,
                terrain,
                |_, _, delta| delta,
                |_, _| true,
            )
            .unwrap();
        highest = highest.max(frame.feet.y);
        last = Some(frame);
    }
    let last = last.unwrap();
    assert!(highest > 1.0, "a full block must produce a real hop");
    assert!(last.feet.z > 1.0);
    assert!((last.feet.y - 1.0).abs() < 0.001);
}

#[test]
fn unavailable_terrain_never_becomes_a_floor_and_teleports_reset_motion() {
    let mut follower = Follower::default();
    let rig = rig();
    assert!(
        follower
            .advance(
                &rig,
                Vec3::ZERO,
                0.0,
                0.05,
                |_, _, _| None,
                |_, _, _| Vec3::ZERO,
                |_, _| true
            )
            .is_none()
    );
    let first = follower
        .advance(
            &rig,
            Vec3::ZERO,
            0.0,
            0.05,
            |_, _, _| Some(0.0),
            |_, _, delta| delta,
            |_, _| true,
        )
        .unwrap();
    let frozen = follower
        .advance(
            &rig,
            Vec3::new(0.0, 0.0, 4.0),
            0.0,
            0.05,
            |_, _, _| None,
            |_, _, _| Vec3::ZERO,
            |_, _| true,
        )
        .unwrap();
    assert_eq!(first.feet, frozen.feet);
    let owner = Vec3::new(100.0, 20.0, 100.0);
    let reset = follower
        .advance(
            &rig,
            owner,
            0.0,
            0.05,
            |_, _, _| Some(20.0),
            |_, _, delta| delta,
            |_, _| true,
        )
        .unwrap();
    assert!(reset.feet.distance(owner) < 2.0);
    assert_eq!(reset.feet.y, 20.0);
}

#[test]
fn controller_admission_and_pose_selection_use_asset_clips() {
    use sha2::Digest;
    let mut descriptor = crate::hats::fixture::crown().descriptor().clone();
    descriptor.id = "fixture:ground_companion".into();
    descriptor.slot = "pet".into();
    let animations: serde_json::Map<_,_> = ["idle", "sit", "trot", "gallop"].into_iter().enumerate().map(|(index,name)| {
        let angle = index as f32 * 20.0;
        (name.into(),serde_json::json!({"loop":true,"animation_length":1.0,"bones":{"head":{"rotation":{"0":[angle,0,0],"1":[angle,0,0]}}}}))
    }).collect();
    let mut definition = serde_json::json!({"animations":animations,"companion":{"idle":"idle","sit":"sit","trot":"trot","gallop":"gallop","trot_stride":1.6,"gallop_stride":2.6}});
    let animation = serde_json::to_vec(&definition).unwrap();
    descriptor.assets.animation = Some(cosmetics::Asset {
        url: "/fixture/animation".into(),
        sha256: format!("{:x}", sha2::Sha256::digest(&animation)),
        bytes: animation.len(),
    });
    let item = crate::hats::admit(
        descriptor.clone(),
        include_bytes!("../crown.geo.json"),
        include_bytes!("../crown.png"),
        &animation,
    )
    .unwrap();
    let model = crate::hats::rig(item);
    assert!(model.companion().is_some());
    let sections = model.companion().unwrap().clearance();
    assert!(!sections.is_empty());
    for gait in [Gait::Idle, Gait::Sit, Gait::Trot, Gait::Gallop] {
        for step in 0..101 {
            let pose = model.companion_pose(Pose {
                gait,
                seconds: step as f32 / 101.0,
            });
            for vertex in model.geometry.vertices.iter() {
                let index = vertex.bone_index as usize;
                let bone = pose[index];
                let p = Vec3::from_slice(&bone.translation_scale[..3])
                    + Quat::from_array(bone.rotation)
                        * (Vec3::from_array(vertex.position)
                            - Vec3::from_array(model.geometry.bone_pivots[index]));
                assert!(
                    sections
                        .iter()
                        .any(|s| (p.x - s.centre[0]).hypot(p.z - s.centre[1]) <= s.radius
                            && p.y >= s.bottom
                            && p.y <= s.top),
                    "animated vertex outside collision clearance"
                );
            }
        }
    }
    assert_ne!(
        model.companion_pose(Pose {
            gait: Gait::Idle,
            seconds: 0.0
        }),
        model.companion_pose(Pose {
            gait: Gait::Trot,
            seconds: 0.0
        })
    );
    definition["companion"]["gallop"] = "missing".into();
    let animation = serde_json::to_vec(&definition).unwrap();
    descriptor.assets.animation = Some(cosmetics::Asset {
        url: "/fixture/animation".into(),
        sha256: format!("{:x}", sha2::Sha256::digest(&animation)),
        bytes: animation.len(),
    });
    assert!(
        crate::hats::admit(
            descriptor,
            include_bytes!("../crown.geo.json"),
            include_bytes!("../crown.png"),
            &animation
        )
        .is_none()
    );
}

#[test]
fn ordinary_blockage_never_teleports_the_companion() {
    let rig = rig();
    let mut follower = Follower::default();
    let mut step = |owner| {
        follower
            .advance(
                &rig,
                owner,
                0.0,
                0.05,
                |_, _, _| Some(0.0),
                |_, _, _| Vec3::ZERO,
                |_, _| true,
            )
            .unwrap()
    };
    let first = step(Vec3::ZERO);
    for _ in 0..200 {
        let frame = step(Vec3::new(0.0, 0.0, 4.0));
        assert_eq!(
            frame.feet, first.feet,
            "blocked following must wait or route around, never snap to the owner"
        );
    }
}

#[test]
fn a_blocked_slot_without_a_clear_landing_does_not_cause_repeated_hops() {
    let rig = rig();
    let mut follower = Follower::default();
    let mut step = |owner| {
        follower
            .advance(
                &rig,
                owner,
                0.0,
                0.05,
                |_, _, _| Some(0.0),
                |feet, _, delta| {
                    if feet.y < 0.8 && delta.y == 0.0 {
                        Vec3::ZERO
                    } else {
                        delta
                    }
                },
                |feet, _| feet.z < -1.0,
            )
            .unwrap()
    };
    let first = step(Vec3::ZERO);
    let mut previous = first.feet;
    for _ in 0..200 {
        let frame = step(Vec3::new(0.0, 0.0, 4.0));
        assert!(
            frame.feet.distance(previous) < 0.65,
            "ordinary navigation must be continuous"
        );
        assert!(
            frame.feet.y.abs() < 0.001,
            "a failed landing must not create a hop loop"
        );
        previous = frame.feet;
    }
}

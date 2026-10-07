use super::*;
use crate::{
    ActorAnimationVariables, ActorLifetimeId, EntityRigId, HandPhase, ItemAnimationState,
    SkinRenderLayer,
};

pub(super) struct Fixture {
    skin: Arc<assets::SkinGeometry>,
    names: Vec<Box<str>>,
    pub(super) rest: Vec<BoneTransform>,
}

impl Fixture {
    fn new() -> Self {
        // Owned cubeless fixture with the pinned native player's joint pivots.
        let skin = assets::parse_skin_geometry(
            r#"{"geometry":{"default":"geometry.emote_test"}}"#,
            r#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{
                "identifier":"geometry.emote_test","texture_width":16,"texture_height":16},"bones":[
                {"name":"root","pivot":[0,0,0]},
                {"name":"waist","parent":"root","pivot":[0,12,0]},
                {"name":"body","parent":"waist","pivot":[0,24,0]},
                {"name":"head","parent":"body","pivot":[0,24,0]},
                {"name":"leftArm","parent":"body","pivot":[5,22,0]},
                {"name":"rightArm","parent":"body","pivot":[-5,22,0]},
                {"name":"leftLeg","parent":"root","pivot":[1.9,12,0]},
                {"name":"rightLeg","parent":"root","pivot":[-1.9,12,0]},
                {"name":"coat","parent":"body","pivot":[0,18,0]}]}]}"#,
        )
        .unwrap()
        .unwrap();
        Self::from_skin(skin)
    }

    pub(super) fn from_skin(skin: assets::SkinGeometry) -> Self {
        let (bones, names) = geometry::skeleton(&skin.bones).unwrap();
        let rest = compose_pose(&bones, &[]).unwrap();
        Self {
            skin: Arc::new(skin),
            names,
            rest,
        }
    }

    pub(super) fn rig(&self) -> ActorRigSnapshot<'_> {
        ActorRigSnapshot {
            actor: ActorLifetimeId {
                session_id: 7,
                dimension: 0,
                runtime_id: 42,
                spawn_revision: 3,
            },
            rig: EntityRigId(0),
            previous: &self.rest,
            current: &self.rest,
            rest: &self.rest,
            rest_completed_tick: 20,
            rest_reset_generation: 1,
            completed_tick: 20,
            reset_generation: 1,
            fallback: assets::EntityRigFallback::Skip,
            scale: 1.0,
            axis_scale: [1.0; 3],
            previous_body_yaw: 0.0,
            body_yaw: 0.0,
            render: &[],
            bone_names: &self.names,
            skin_geometry: Some(&self.skin),
            skin_layers: &[],
            hand: [HandPhase::default(); 2],
            item_animation: [ItemAnimationState::default(); 2],
            off_hand_animation: [ItemAnimationState::default(); 2],
            animation_variables: ActorAnimationVariables::default(),
            java: Default::default(),
            java_equipped: None,
        }
    }

    fn index(&self, name: &str) -> usize {
        self.names
            .iter()
            .position(|part| part.as_ref() == name)
            .unwrap()
    }
}

pub(super) fn point(bone: BoneTransform, relative: [f32; 3]) -> [f32; 3] {
    let rotated = rotate_vector(bone.rotation, relative);
    std::array::from_fn(|axis| bone.translation_scale[axis] + rotated[axis])
}

pub(super) fn near(a: [f32; 3], b: [f32; 3]) {
    for axis in 0..3 {
        assert!((a[axis] - b[axis]).abs() < 1e-4, "{a:?} != {b:?}");
    }
}

#[test]
fn owned_custom_emote_deep_squat_keeps_feet_and_attached_level_head() {
    let f = Fixture::new();
    let period = CustomEmote::Twerk.duration_seconds();
    let first = sample(&f.rig(), CustomEmote::Twerk, 0.0, 0.0).unwrap();
    let head_height = first.current[f.index("head")].translation_scale[1];
    let first_hip = point(first.current[f.index("body")], [0.0, -12.0, 0.0]);
    let mut hip_travel = 0.0_f32;
    for step in 0..=32 {
        let phase = period * f64::from(step) / 32.0;
        let pose = sample(&f.rig(), CustomEmote::Twerk, phase, phase).unwrap();
        let body = pose.current[f.index("body")];
        let hips = point(body, [0.0, -12.0, 0.0]);
        assert!(hips[1] < 9.5, "the squat must lower the hips substantially");
        near(hips, point(pose.current[f.index("waist")], [0.0; 3]));
        let head = pose.current[f.index("head")];
        near(point(body, [0.0; 3]), point(head, [0.0; 3]));
        near(
            rotate_vector(head.rotation, [0.0, 1.0, 0.0]),
            [0.0, 1.0, 0.0],
        );
        assert!(
            (head.translation_scale[1] - head_height).abs() < 1e-4,
            "shoulders and head must not hop during the hip pulse"
        );
        hip_travel = hip_travel.max((hips[2] - first_hip[2]).abs());
        for name in ["leftleg", "rightleg"] {
            let foot = point(pose.current[f.index(name)], [0.0, -12.0, 0.0]);
            assert!(foot[1].abs() < 1e-4 && foot[2].abs() < 1e-4, "{foot:?}");
            assert!(foot[0].abs() > 3.0, "feet must form a wide stance");
            near(foot, point(first.current[f.index(name)], [0.0, -12.0, 0.0]));
        }
        for (name, x) in [("leftarm", -5.0), ("rightarm", 5.0)] {
            near(
                point(body, [x, -2.0, 0.0]),
                point(pose.current[f.index(name)], [0.0; 3]),
            );
        }
    }
    assert!(
        hip_travel > 2.0,
        "the hip thrust must remain clearly visible"
    );
}

#[test]
fn owned_custom_emote_twists_torso_without_turning_head_or_sliding_feet() {
    let f = Fixture::new();
    let period = CustomEmote::Twerk.duration_seconds();
    let a = sample(&f.rig(), CustomEmote::Twerk, period / 4.0, period / 4.0).unwrap();
    let b = sample(&f.rig(), CustomEmote::Twerk, period * 0.75, period * 0.75).unwrap();
    let forward = |pose: &CustomEmotePose| {
        rotate_vector(pose.current[f.index("body")].rotation, [0.0, 0.0, 1.0])
    };
    assert!(forward(&a)[0] * forward(&b)[0] < 0.0);
    assert!((forward(&a)[0] - forward(&b)[0]).abs() > 0.1);
    near(
        point(a.current[f.index("head")], [0.0; 3]),
        point(a.current[f.index("body")], [0.0; 3]),
    );
    assert_eq!(
        a.current[f.index("head")].rotation,
        b.current[f.index("head")].rotation
    );
    for name in ["leftleg", "rightleg"] {
        near(
            point(a.current[f.index(name)], [0.0, -12.0, 0.0]),
            point(b.current[f.index(name)], [0.0, -12.0, 0.0]),
        );
    }
}

#[test]
fn owned_custom_emote_flat_biped_matches_native_hierarchy_and_clothing() {
    let f = Fixture::new();
    let mut flat = (*f.skin).clone();
    for bone in &mut flat.bones {
        if matches!(bone.name.as_ref(), "body" | "head" | "leftArm" | "rightArm") {
            bone.parent = Some("root".into());
        }
    }
    let flat = Fixture::from_skin(flat);
    let period = CustomEmote::Twerk.duration_seconds();
    for phase in [0.0, period / 4.0, period / 2.0] {
        let native = sample(&f.rig(), CustomEmote::Twerk, phase, phase).unwrap();
        let other = sample(&flat.rig(), CustomEmote::Twerk, phase, phase).unwrap();
        for name in &f.names {
            let a = native.current[f.index(name)];
            let b = other.current[flat.index(name)];
            near(point(a, [0.0; 3]), point(b, [0.0; 3]));
            near(
                rotate_vector(a.rotation, [0.0, 1.0, 0.0]),
                rotate_vector(b.rotation, [0.0, 1.0, 0.0]),
            );
        }
    }
}

#[test]
fn owned_custom_emote_loops_and_retains_exact_rig_ownership() {
    let f = Fixture::new();
    let rig = f.rig();
    let emote = CustomEmote::Twerk;
    let pose = sample(&rig, emote, 0.0, emote.duration_seconds()).unwrap();
    assert_eq!(pose.previous, pose.current);
    assert_ne!(pose.current.as_ref(), f.rest.as_slice());
    let sampled = pose.snapshot(rig);
    assert_eq!(sampled.actor, rig.actor);
    assert_eq!(sampled.completed_tick, rig.completed_tick);
    assert_eq!(sampled.reset_generation, rig.reset_generation);
    assert_eq!(sampled.rest, rig.rest);
    assert_eq!(sampled.bone_names, rig.bone_names);
    assert_eq!(
        rig.current,
        f.rest.as_slice(),
        "sampling cannot alter the native hand/body source"
    );
    for transform in pose.current.iter() {
        assert!(
            transform
                .rotation
                .iter()
                .chain(transform.translation_scale.iter())
                .chain(transform.axis_scale.iter())
                .all(|value| value.is_finite())
        );
    }
}

#[test]
fn owned_custom_emote_named_channels_preserve_hierarchy_and_reordered_models() {
    let f = Fixture::new();
    let period = CustomEmote::Twerk.duration_seconds();
    let pose = sample(&f.rig(), CustomEmote::Twerk, period / 4.0, period / 4.0).unwrap();
    let body = f.index("body");
    let coat = f.index("coat");
    assert_eq!(
        pose.current[body].rotation, pose.current[coat].rotation,
        "clothing inherits the tilted torso"
    );
    assert_ne!(
        pose.current[coat].translation_scale,
        f.rest[coat].translation_scale
    );
    assert_eq!(
        pose.current[f.index("leftleg")].translation_scale[1],
        pose.current[f.index("rightleg")].translation_scale[1]
    );
    let mut reordered = (*f.skin).clone();
    reordered.bones.reverse();
    let reordered = Fixture::from_skin(reordered);
    let other = sample(
        &reordered.rig(),
        CustomEmote::Twerk,
        period / 4.0,
        period / 4.0,
    )
    .unwrap();
    for name in &f.names {
        assert_eq!(
            pose.current[f.index(name)],
            other.current[reordered.index(name)]
        );
    }
}

#[test]
fn owned_custom_emote_persona_layer_follows_body_and_keeps_artwork() {
    let f = Fixture::new();
    let layer = SkinRenderLayer {
        image: protocol::SkinAnimation {
            kind: protocol::SkinAnimationKind::Face,
            width: 16,
            height: 16,
            rgba8: Arc::from([]),
            frames: 1,
            blinking: false,
        },
        geometry: Arc::clone(&f.skin),
        previous: Arc::from(f.rest.clone()),
        current: Arc::from(f.rest.clone()),
        rest: Arc::from(f.rest.clone()),
        hidden_bones: Arc::from([f.index("coat") as u32]),
        uv_anim: [0.0, 0.0, 1.0, 1.0],
    };
    let layers = [layer];
    let mut rig = f.rig();
    rig.skin_layers = &layers;
    let pose = sample(
        &rig,
        CustomEmote::Twerk,
        0.0,
        CustomEmote::Twerk.duration_seconds() / 4.0,
    )
    .unwrap();
    assert_eq!(pose.skin_layers[0].current, pose.current);
    assert_eq!(pose.skin_layers[0].previous, pose.previous);
    assert_eq!(pose.skin_layers[0].hidden_bones, layers[0].hidden_bones);
    assert_eq!(pose.skin_layers[0].uv_anim, layers[0].uv_anim);
    assert!(Arc::ptr_eq(
        &pose.skin_layers[0].geometry,
        &layers[0].geometry
    ));
    assert_eq!(layers[0].current.as_ref(), f.rest.as_slice());
}

#[test]
fn owned_custom_emote_skips_invalid_phases_and_mismatched_models() {
    let f = Fixture::new();
    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(sample(&f.rig(), CustomEmote::Twerk, invalid, 0.0).is_none());
        assert!(sample(&f.rig(), CustomEmote::Twerk, 0.0, invalid).is_none());
    }
    let mut rig = f.rig();
    rig.bone_names = &[];
    assert!(sample(&rig, CustomEmote::Twerk, 0.0, 0.0).is_none());
    rig = f.rig();
    rig.current = &[];
    assert!(sample(&rig, CustomEmote::Twerk, 0.0, 0.0).is_none());
}

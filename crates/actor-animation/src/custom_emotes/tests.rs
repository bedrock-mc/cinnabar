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
            placement_frame: assets::placement::EntityPlacementFrame::Unresolved,
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
            pose_evaluated: true,
            fallback: assets::EntityRigFallback::Skip,
            scale: 1.0,
            axis_scale: [1.0; 3],
            previous_body_yaw: 0.0,
            body_yaw: 0.0,
            render: &[],
            attachments: None,
            bone_names: &self.names,
            skin_geometry: Some(&self.skin),
            skin_mesh: None,
            skin_layers: &[],
            held_item_scale: None,
            hand: [HandPhase::default(); 2],
            item_animation: [ItemAnimationState::default(); 2],
            off_hand_animation: [ItemAnimationState::default(); 2],
            animation_variables: ActorAnimationVariables::default(),
            java: Default::default(),
            main_hand_equipped: None,
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
fn owned_custom_emote_flat_biped_matches_native_hierarchy_and_clothing() {
    let f = Fixture::new();
    let mut flat = (*f.skin).clone();
    for bone in &mut flat.bones {
        if matches!(bone.name.as_ref(), "body" | "head" | "leftArm" | "rightArm") {
            bone.parent = Some("root".into());
        }
    }
    let flat = Fixture::from_skin(flat);
    let period = catalog_emote().duration_seconds();
    for phase in [0.0, period / 4.0, period / 2.0] {
        let native = sample_custom_emote(&f.rig(), catalog_emote(), phase, phase).unwrap();
        let other = sample_custom_emote(&flat.rig(), catalog_emote(), phase, phase).unwrap();
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
    let emote = catalog_emote();
    let pose = sample_custom_emote(&rig, emote, 0.0, emote.duration_seconds()).unwrap();
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
    let period = catalog_emote().duration_seconds();
    let pose = sample_custom_emote(&f.rig(), catalog_emote(), period / 4.0, period / 4.0).unwrap();
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
    let mut reordered = (*f.skin).clone();
    reordered.bones.reverse();
    let reordered = Fixture::from_skin(reordered);
    let other = sample_custom_emote(
        &reordered.rig(),
        catalog_emote(),
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
        image: render_api::SkinAnimation {
            kind: render_api::SkinAnimationKind::Face,
            width: 16,
            height: 16,
            rgba8: Arc::from([]),
            frames: 1,
            blinking: false,
        },
        geometry: Arc::clone(&f.skin),
        mesh: None,
        previous: Arc::from(f.rest.clone()),
        current: Arc::from(f.rest.clone()),
        rest: Arc::from(f.rest.clone()),
        hidden_bones: Arc::from([f.index("coat") as u32]),
        uv_anim: [0.0, 0.0, 1.0, 1.0],
    };
    let layers = [layer];
    let mut rig = f.rig();
    rig.skin_layers = &layers;
    let pose = sample_custom_emote(
        &rig,
        catalog_emote(),
        0.0,
        catalog_emote().duration_seconds() / 4.0,
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
        assert!(sample_custom_emote(&f.rig(), catalog_emote(), invalid, 0.0).is_none());
        assert!(sample_custom_emote(&f.rig(), catalog_emote(), 0.0, invalid).is_none());
    }
    let mut rig = f.rig();
    rig.bone_names = &[];
    assert!(sample_custom_emote(&rig, catalog_emote(), 0.0, 0.0).is_none());
    rig = f.rig();
    rig.current = &[];
    assert!(sample_custom_emote(&rig, catalog_emote(), 0.0, 0.0).is_none());
}

/// A parsed authored marker with cubeless legs must not imply generated companion joints.
fn partial_marker_fixture() -> Fixture {
    Fixture::from_skin(
        assets::parse_skin_geometry(
            r#"{"geometry":{"default":"geometry.partial_emote"}}"#,
            r#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{
            "identifier":"geometry.partial_emote","texture_width":16,"texture_height":16},"bones":[
            {"name":"body","pivot":[0,24,0]},
            {"name":"head","parent":"body","pivot":[0,24,0]},
            {"name":"leftarm","parent":"body","pivot":[5,22,0]},
            {"name":"rightarm","parent":"body","pivot":[-5,22,0]},
            {"name":"leftleg","pivot":[1.9,12,0]},
            {"name":"rightleg","pivot":[-1.9,12,0]},
            {"name":"leftleg.cinnabar_knee","parent":"leftleg","pivot":[1.9,6,0]}]}]}"#,
        )
        .unwrap()
        .unwrap(),
    )
}

#[test]
fn partial_authored_knee_markers_use_the_ordinary_emote_pose() {
    let f = partial_marker_fixture();
    let pose = sample_custom_emote(&f.rig(), catalog_emote(), 0.0, 0.1).unwrap();
    assert!(pose.articulated.is_none());
    assert_eq!(pose.current.len(), f.names.len());
    assert!(pose.current.iter().all(|bone| {
        bone.translation_scale
            .iter()
            .chain(&bone.rotation)
            .all(|value| value.is_finite())
    }));
    assert_ne!(pose.current[f.index("body")], f.rest[f.index("body")]);
}

#[test]
fn partial_authored_knee_markers_in_an_independent_skin_layer_are_safe() {
    let f = Fixture::new();
    let partial = Fixture::from_skin(
        assets::parse_skin_geometry(
            r#"{"geometry":{"default":"geometry.leg_layer"}}"#,
            r#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{
                "identifier":"geometry.leg_layer","texture_width":16,"texture_height":16},"bones":[
                {"name":"leftleg","pivot":[1.9,12,0]},
                {"name":"leftleg.cinnabar_knee","parent":"leftleg","pivot":[1.9,6,0]},
                {"name":"leftleg.cinnabar_ankle","parent":"leftleg.cinnabar_knee","pivot":[1.9,2,0]},
                {"name":"rightleg","pivot":[-1.9,12,0]},
                {"name":"rightleg.cinnabar_knee","parent":"rightleg","pivot":[-1.9,6,0]},
                {"name":"rightleg.cinnabar_ankle","parent":"rightleg.cinnabar_knee","pivot":[-1.9,2,0]}]}]}"#,
        )
        .unwrap()
        .unwrap(),
    );
    let layers = [SkinRenderLayer {
        image: render_api::SkinAnimation {
            kind: render_api::SkinAnimationKind::Face,
            width: 16,
            height: 16,
            rgba8: Arc::from([]),
            frames: 1,
            blinking: false,
        },
        geometry: Arc::clone(&partial.skin),
        mesh: None,
        previous: partial.rest.clone().into(),
        current: partial.rest.clone().into(),
        rest: partial.rest.clone().into(),
        hidden_bones: Arc::from([]),
        uv_anim: [0.0, 0.0, 1.0, 1.0],
    }];
    let mut rig = f.rig();
    rig.skin_layers = &layers;
    let pose = sample_custom_emote(&rig, catalog_emote(), 0.0, 0.1).unwrap();
    assert!(Arc::ptr_eq(&pose.skin_layers[0].geometry, &partial.skin));
    assert_eq!(pose.skin_layers[0].current.len(), partial.names.len());
    assert!(pose.skin_layers[0].current.iter().all(|bone| {
        bone.translation_scale
            .iter()
            .chain(&bone.rotation)
            .all(|value| value.is_finite())
    }));
}

/// Admits the stable synthetic clip independently of catalog activation.
pub(super) fn catalog_emote() -> Item {
    cosmetics::test_support::emote("fixture:articulated", "Articulated fixture", true)
}

use super::*;

pub(super) fn fixture() -> Arc<SkinGeometry> {
    Arc::new(assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.knee_test"}}"#,
        r#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{
        "identifier":"geometry.knee_test","texture_width":64,"texture_height":64},"bones":[
        {"name":"root","pivot":[0,0,0]},
        {"name":"waist","parent":"root","pivot":[0,12,0]},
        {"name":"body","parent":"waist","pivot":[0,24,0]},
        {"name":"head","parent":"body","pivot":[0,24,0]},
        {"name":"leftArm","parent":"body","pivot":[5,22,0]},
        {"name":"rightArm","parent":"body","pivot":[-5,22,0]},
        {"name":"leftLeg","parent":"root","pivot":[1.9,12,0],"cubes":[{"origin":[0,0,-2],"size":[4,12,4],"uv":[0,16]}]},
        {"name":"rightLeg","parent":"root","pivot":[-1.9,12,0],"cubes":[{"origin":[-4,0,-2],"size":[4,12,4],"uv":[16,48],"mirror":true}]},
        {"name":"pants","parent":"leftLeg","pivot":[1.9,12,0],"cubes":[{"origin":[0,0,-2],"size":[4,12,4],"uv":[0,32],"inflate":0.25}]}]}]}"#,
    ).unwrap().unwrap())
}

#[test]
fn owned_custom_emote_knee_mesh_preserves_skin_uvs_clothing_and_native_model() {
    let source = fixture();
    let original = (*source).clone();
    let model = model(&source).unwrap().geometry;
    assert_eq!(*source, original);
    assert_ne!(model.digest, source.digest);
    assert!(Arc::ptr_eq(
        &model,
        &super::model(&source).unwrap().geometry
    ));
    assert_eq!(model.bones.len(), source.bones.len() + 6);
    let upper = &model.bones[6].cubes[0];
    let lower = &model.bones[9].cubes[0];
    assert_eq!(upper.origin[1].get(), 6.0);
    assert_eq!(upper.size[1].get(), 6.0);
    assert_eq!(lower.origin[1].get(), 2.0);
    assert_eq!(lower.size[1].get(), 4.0);
    let EntityGeometryUv::Faces(upper) = &upper.uv else {
        panic!("explicit UV crop")
    };
    let EntityGeometryUv::Faces(lower) = &lower.uv else {
        panic!("explicit UV crop")
    };
    assert_eq!(
        upper.north.as_ref().unwrap().uv.map(Scalar::get),
        [4.0, 20.0]
    );
    assert_eq!(
        upper
            .north
            .as_ref()
            .unwrap()
            .uv_size
            .unwrap()
            .map(Scalar::get),
        [4.0, 6.0]
    );
    assert_eq!(
        lower.north.as_ref().unwrap().uv.map(Scalar::get),
        [4.0, 26.0]
    );
    assert_ne!(
        lower.down, upper.down,
        "a knee must not acquire a boot sole"
    );
    assert_eq!(upper.down, lower.up);
    let find = |name: &str| {
        model
            .bones
            .iter()
            .find(|bone| bone.name.eq_ignore_ascii_case(name))
            .unwrap()
    };
    assert!(find("rightleg.cinnabar_ankle").cubes[0].mirror);
    assert_eq!(
        find("pants.cinnabar_knee").parent.as_deref(),
        Some("leftLeg.cinnabar_knee")
    );
    assert_eq!(find("pants.cinnabar_ankle").cubes[0].inflate.get(), 0.25);
    assert_eq!(
        find("pants.cinnabar_ankle").parent.as_deref(),
        Some("leftLeg.cinnabar_ankle")
    );
}

#[test]
fn owned_custom_emote_knee_mesh_skips_rotated_or_nonclassic_legs() {
    let mut source = (*fixture()).clone();
    source.bones[6].cubes[0].rotation[0] = Scalar::new(10.0).unwrap();
    assert!(build(&source).is_none());
    let mut source = (*fixture()).clone();
    source.bones[6].cubes[0].size[1] = Scalar::new(3.0).unwrap();
    assert!(build(&source).is_none());
}

#[test]
fn owned_custom_emote_publishes_matching_body_and_layer_meshes() {
    use crate::custom_emotes::{sample_custom_emote, tests::Fixture};
    use crate::{CustomEmote, SkinRenderLayer};

    let source = fixture();
    let f = Fixture::from_skin((*source).clone());
    let native = render_model::skin_geometry(&source, render_model::DIAGNOSTIC_RIG_ID).unwrap();
    let layers = [SkinRenderLayer {
        image: render_api::SkinAnimation {
            kind: render_api::SkinAnimationKind::Face,
            width: 64,
            height: 64,
            rgba8: Arc::from([]),
            frames: 1,
            blinking: false,
        },
        geometry: Arc::clone(&source),
        mesh: Some(native.clone()),
        previous: f.rest.clone().into(),
        current: f.rest.clone().into(),
        rest: f.rest.clone().into(),
        hidden_bones: Arc::from([]),
        uv_anim: [0.0, 0.0, 1.0, 1.0],
    }];
    let rig = crate::ActorRigSnapshot {
        skin_mesh: Some(&native),
        skin_layers: &layers,
        ..f.rig()
    };
    let mut retained_vertices = None;
    for phase in [0.0, 0.1, 0.3, CustomEmote::Twerk.duration_seconds()] {
        let pose = sample_custom_emote(&rig, CustomEmote::Twerk, phase, phase).unwrap();
        let snapshot = pose.snapshot(rig);
        let mesh = snapshot.skin_mesh.expect("articulated body mesh");
        let expected =
            render_model::skin_geometry(snapshot.skin_geometry.unwrap(), mesh.id).unwrap();
        assert!(
            *mesh == expected,
            "body vertices must match the jointed skeleton"
        );
        assert_ne!(mesh.vertices, native.vertices);
        assert_eq!(mesh.bone_pivots.len(), snapshot.current.len());
        if let Some(vertices) = &retained_vertices {
            assert!(
                Arc::ptr_eq(vertices, &mesh.vertices),
                "reuse the prepared emote mesh"
            );
        }
        retained_vertices = Some(Arc::clone(&mesh.vertices));
        let layer = &snapshot.skin_layers[0];
        let layer_mesh = layer.mesh.as_ref().expect("articulated clothing mesh");
        let expected = render_model::skin_geometry(&layer.geometry, layer_mesh.id).unwrap();
        assert!(
            *layer_mesh == expected,
            "clothing vertices must follow their new joints"
        );
        assert!(Arc::ptr_eq(&mesh.vertices, &layer_mesh.vertices));
        assert_eq!(layer_mesh.bone_pivots.len(), layer.current.len());
    }
    assert_eq!(rig.skin_mesh, Some(&native));
    assert_eq!(layers[0].mesh.as_ref(), Some(&native));
    assert_eq!(rig.current, f.rest);
}

#[test]
fn owned_custom_emote_knees_bend_with_connected_segments_and_planted_feet() {
    use crate::CustomEmote;
    use crate::custom_emotes::{
        sample_custom_emote,
        tests::{Fixture, near, point},
    };
    let source = fixture();
    let f = Fixture::from_skin((*source).clone());
    let native = f.rest.clone();
    let period = CustomEmote::Twerk.duration_seconds();
    let mut first_feet: Option<Vec<[f32; 3]>> = None;
    let mut head_height = None;
    let mut lowest_hip = f32::INFINITY;
    let mut highest_hip = f32::NEG_INFINITY;
    for step in 0..=32 {
        let phase = f64::from(step) * period / 32.0;
        let pose = sample_custom_emote(&f.rig(), CustomEmote::Twerk, phase, phase).unwrap();
        let snapshot = pose.snapshot(f.rig());
        let index = |name: &str| {
            snapshot
                .bone_names
                .iter()
                .position(|part| part.as_ref() == name)
                .unwrap()
        };
        let head = snapshot.current[index("head")].translation_scale[1];
        let baseline = *head_height.get_or_insert(head);
        assert!((head - baseline).abs() < 1e-4);
        let hip = snapshot.current[index("leftleg")].translation_scale[1];
        lowest_hip = lowest_hip.min(hip);
        highest_hip = highest_hip.max(hip);
        let mut feet = Vec::new();
        for name in ["leftleg", "rightleg"] {
            let thigh = snapshot.current[index(name)];
            let shin = snapshot.current[index(&format!("{name}.cinnabar_knee"))];
            near(point(thigh, [0.0, -6.0, 0.0]), point(shin, [0.0; 3]));
            let ankle = index(&format!("{name}.cinnabar_ankle"));
            let foot = snapshot.current[ankle];
            near(point(shin, [0.0, -4.0, 0.0]), point(foot, [0.0; 3]));
            assert_eq!(
                foot.rotation,
                [0.0, 0.0, 0.0, 1.0],
                "soles must remain level"
            );
            let cube = &snapshot.skin_geometry.unwrap().bones[ankle].cubes[0];
            let origin = cube.origin.map(Scalar::get);
            let size = cube.size.map(Scalar::get);
            let pivot = snapshot.rest[ankle].translation_scale;
            // The ankle must be buried inside the textured foot, so rotating
            // the shin cannot expose a sliced-off foot at its joint center.
            assert!(origin[1] < pivot[1]);
            assert!(origin[1] + size[1] > pivot[1]);
            let overlap = point(shin, [0.0, -3.0, 0.0]);
            let local = [
                overlap[0] - foot.translation_scale[0],
                overlap[1] - foot.translation_scale[1],
                overlap[2] - foot.translation_scale[2],
            ];
            for axis in 0..3 {
                let (min, max) = if axis == 0 {
                    (-origin[0] - size[0] - pivot[0], -origin[0] - pivot[0])
                } else {
                    (
                        origin[axis] - pivot[axis],
                        origin[axis] + size[axis] - pivot[axis],
                    )
                };
                assert!(
                    local[axis] > min && local[axis] < max,
                    "shin must overlap the foot throughout playback: {local:?}"
                );
            }
            for (x, z) in [
                (0.0, 0.0),
                (size[0], 0.0),
                (0.0, size[2]),
                (size[0], size[2]),
            ] {
                let corner = point(
                    foot,
                    [
                        -origin[0] - x - pivot[0],
                        origin[1] - pivot[1],
                        origin[2] + z - pivot[2],
                    ],
                );
                assert!(
                    corner[1].abs() < 1e-4,
                    "the whole sole must stay on the floor: {corner:?}"
                );
                feet.push(corner);
            }
            let thigh_axis = super::super::rotate_vector(thigh.rotation, [0.0, -1.0, 0.0]);
            let shin_axis = super::super::rotate_vector(shin.rotation, [0.0, -1.0, 0.0]);
            let dot: f32 = (0..3).map(|axis| thigh_axis[axis] * shin_axis[axis]).sum();
            assert!(dot < 0.55, "knees must visibly bend: {dot}");
            assert!(
                shin_axis[1] < -0.5,
                "shins must remain above the planted feet"
            );
        }
        if let Some(first) = &first_feet {
            for (a, b) in feet.iter().zip(first) {
                near(*a, *b);
            }
        } else {
            first_feet = Some(feet);
        }
        assert_eq!(f.rig().current, native);
        assert_eq!(source.bones.len(), 9);
    }
    assert!(
        highest_hip - lowest_hip > 2.0,
        "the pelvis must visibly pulse up/down"
    );
}

#[test]
fn shared_emotes_reuse_prepared_meshes_across_many_distinct_skins() {
    let models: Vec<_> = (0u64..24)
        .map(|index| {
            let mut source = (*fixture()).clone();
            source.digest[..8].copy_from_slice(&index.to_le_bytes());
            let source = Arc::new(source);
            let prepared = model(&source).unwrap();
            (source, prepared)
        })
        .collect();
    for (source, prepared) in models {
        let next = model(&source).unwrap();
        assert!(Arc::ptr_eq(&prepared.mesh.vertices, &next.mesh.vertices));
    }
}

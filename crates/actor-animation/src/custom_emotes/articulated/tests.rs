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
fn breakdance_preserves_unsupported_partial_geometry() {
    use crate::CustomEmote;
    use crate::custom_emotes::{sample_custom_emote, tests::Fixture};

    let source = fixture();
    let f = Fixture::from_skin((*source).clone());
    let rig = f.rig();
    let pose = sample_custom_emote(&rig, CustomEmote::Breakdance, 0.0, 2.0).unwrap();
    assert!(pose.articulated.is_none());
    assert_eq!(pose.snapshot(rig).bone_names, rig.bone_names);
    assert_eq!(pose.current.len(), rig.current.len());
    assert_eq!(
        pose.snapshot(rig).skin_geometry.unwrap().digest,
        source.digest
    );
}

#[test]
fn owned_custom_emote_knee_mesh_preserves_skin_uvs_clothing_and_native_model() {
    let source = fixture();
    let original = (*source).clone();
    let model = model(&source, CustomEmote::Twerk).unwrap().geometry;
    assert_eq!(*source, original);
    assert_ne!(model.digest, source.digest);
    assert!(Arc::ptr_eq(
        &model,
        &super::model(&source, CustomEmote::Twerk).unwrap().geometry
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
    assert!(build(&source, CustomEmote::Twerk).is_none());
    let mut source = (*fixture()).clone();
    source.bones[6].cubes[0].size[1] = Scalar::new(3.0).unwrap();
    assert!(build(&source, CustomEmote::Twerk).is_none());
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
            let prepared = model(&source, CustomEmote::Twerk).unwrap();
            (source, prepared)
        })
        .collect();
    for (source, prepared) in models {
        let next = model(&source, CustomEmote::Twerk).unwrap();
        assert!(Arc::ptr_eq(&prepared.mesh.vertices, &next.mesh.vertices));
    }
}

/// Classic cuboid body with ordinary head, arm and jacket geometry.
fn full_fixture() -> Arc<SkinGeometry> {
    let mut source = (*fixture()).clone();
    let mut bones = source.bones.to_vec();
    for (index, origin, size) in [
        (2, [-4.0, 12.0, -2.0], [8.0, 12.0, 4.0]),
        (3, [-4.0, 24.0, -4.0], [8.0, 8.0, 8.0]),
        (4, [4.0, 12.0, -2.0], [4.0, 12.0, 4.0]),
        (5, [-8.0, 12.0, -2.0], [4.0, 12.0, 4.0]),
    ] {
        let mut cube = bones[6].cubes[0].clone();
        cube.origin = origin.map(|v| Scalar::new(v).unwrap());
        cube.size = size.map(|v| Scalar::new(v).unwrap());
        bones[index].cubes = vec![cube].into();
    }
    for (root, name) in [(2, "jacket"), (4, "sleeve")] {
        let mut layer = bones[root].clone();
        layer.name = name.into();
        layer.parent = Some(bones[root].name.clone());
        layer.cubes[0].inflate = Scalar::new(0.25).unwrap();
        bones.push(layer);
    }
    source.poly_meshes = vec![None; bones.len()].into();
    source.bones = bones.into();
    source.digest = Sha256::digest(b"original full cuboid breakdance test fixture").into();
    Arc::new(source)
}

#[test]
fn breakdance_bends_elbows_knees_and_spine_without_changing_native_geometry() {
    use crate::custom_emotes::{sample_custom_emote, tests::Fixture};
    let source = full_fixture();
    let original = (*source).clone();
    let f = Fixture::from_skin((*source).clone());
    let pose = sample_custom_emote(
        &f.rig(),
        CustomEmote::Breakdance,
        0.0,
        0.24 * CustomEmote::Breakdance.duration_seconds(),
    )
    .unwrap();
    let rig = pose.snapshot(f.rig());
    let find = |name: &str| {
        rig.bone_names
            .iter()
            .position(|part| part.as_ref() == name)
            .unwrap()
    };
    for (a, b) in [
        ("leftarm", "leftarm.cinnabar_elbow"),
        ("leftleg", "leftleg.cinnabar_knee"),
        ("body", "body.cinnabar_spine"),
    ] {
        let a = pose.current[find(a)].rotation;
        let b = pose.current[find(b)].rotation;
        let dot = a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>().abs();
        assert!(dot < 0.995, "{a:?} and {b:?} must articulate");
    }
    assert_eq!(*source, original);
    assert!(rig.bone_names.len() > f.rig().bone_names.len());
}

/// Transforms prepared vertices independently of the cached contact bounds.
fn posed_vertices(
    mesh: &render_model::ActorRigGeometry,
    pose: &[crate::BoneTransform],
    index: usize,
) -> Vec<[f32; 3]> {
    mesh.vertices
        .iter()
        .filter(|v| v.bone_index as usize == index)
        .map(|vertex| {
            let offset = std::array::from_fn(|axis| {
                (vertex.position[axis] - mesh.bone_pivots[index][axis]) * 16.0
            });
            crate::custom_emotes::tests::point(pose[index], offset)
        })
        .collect()
}

#[test]
fn breakdance_preserves_joint_lengths_and_keeps_both_palms_planted_in_the_freeze() {
    use crate::custom_emotes::{
        sample_custom_emote,
        tests::{Fixture, near, point},
    };
    let source = full_fixture();
    let f = Fixture::from_skin((*source).clone());
    let period = CustomEmote::Breakdance.duration_seconds();
    for step in 0..=240 {
        let time = period * f64::from(step) / 240.0;
        let pose = sample_custom_emote(&f.rig(), CustomEmote::Breakdance, time, time).unwrap();
        let rig = pose.snapshot(f.rig());
        let mesh = rig.skin_mesh.unwrap();
        let find = |name: &str| {
            rig.bone_names
                .iter()
                .position(|part| part.as_ref() == name)
                .unwrap()
        };
        let mut lowest = f32::INFINITY;
        for index in 0..rig.current.len() {
            for vertex in posed_vertices(mesh, rig.current, index) {
                lowest = lowest.min(vertex[1]);
            }
        }
        assert!(lowest.abs() < 0.0001, "floor contact at {time}: {lowest}");
        for (a, b) in [
            ("leftarm", "leftarm.cinnabar_elbow"),
            ("leftarm.cinnabar_elbow", "leftarm.cinnabar_wrist"),
            ("rightarm", "rightarm.cinnabar_elbow"),
            ("rightarm.cinnabar_elbow", "rightarm.cinnabar_wrist"),
            ("leftleg", "leftleg.cinnabar_knee"),
            ("leftleg.cinnabar_knee", "leftleg.cinnabar_ankle"),
            ("rightleg", "rightleg.cinnabar_knee"),
            ("rightleg.cinnabar_knee", "rightleg.cinnabar_ankle"),
            ("body", "body.cinnabar_spine"),
            ("body.cinnabar_spine", "leftleg"),
            ("body.cinnabar_spine", "rightleg"),
        ] {
            let (a, b) = (find(a), find(b));
            let relative = std::array::from_fn(|axis| {
                rig.rest[b].translation_scale[axis] - rig.rest[a].translation_scale[axis]
            });
            let expected = point(rig.current[a], relative);
            near(
                expected,
                std::array::from_fn(|axis| rig.current[b].translation_scale[axis]),
            );
        }

        if (0.32..0.70).contains(&(time / period)) {
            let body = posed_vertices(mesh, rig.current, find("body"));
            let min = body.iter().map(|v| v[1]).reduce(f32::min).unwrap();
            assert!(
                min < 0.6,
                "upper back must keep supporting the windmill at {time}, y={min}"
            );
            assert!(rig.current[find("leftleg")].translation_scale[1] > 7.0);
        }
        if (0.79..=0.84).contains(&(time / period)) {
            for wrist in ["leftarm.cinnabar_wrist", "rightarm.cinnabar_wrist"] {
                let vertices = posed_vertices(mesh, rig.current, find(wrist));
                let min = vertices.iter().map(|v| v[1]).reduce(f32::min).unwrap();
                let clearance = if wrist.starts_with("left") { 0.25 } else { 0.0 };
                assert!(
                    (min - clearance).abs() < 0.001,
                    "{wrist} must carry weight through its outer layer, y={min}"
                );
                let up =
                    crate::pose::rotate_vector(rig.current[find(wrist)].rotation, [0.0, 1.0, 0.0]);
                assert!(up[1] > 0.999, "palm must lie flat");
            }
            let head = posed_vertices(mesh, rig.current, find("head"));
            assert!(
                head.iter().all(|v| v[1] > 0.5),
                "head must clear the ground"
            );
        }
    }
    let seam = sample_custom_emote(&f.rig(), CustomEmote::Breakdance, 0.0, period).unwrap();
    assert_eq!(seam.previous, seam.current);
}

#[test]
fn breakdance_mesh_preserves_skin_clothing_slim_arms_and_clip_cache_identity() {
    let source = full_fixture();
    let model = model(&source, CustomEmote::Breakdance).unwrap();
    assert!(Arc::ptr_eq(
        &model.geometry,
        &super::model(&source, CustomEmote::Breakdance)
            .unwrap()
            .geometry
    ));
    assert_ne!(
        model.geometry.digest,
        super::model(&source, CustomEmote::Twerk)
            .unwrap()
            .geometry
            .digest
    );
    let find = |name: &str| {
        model
            .geometry
            .bones
            .iter()
            .find(|part| part.name.eq_ignore_ascii_case(name))
            .unwrap()
    };
    assert_eq!(
        find("sleeve.cinnabar_elbow").parent.as_deref(),
        Some("leftArm.cinnabar_elbow")
    );
    assert_eq!(
        find("jacket.cinnabar_spine").parent.as_deref(),
        Some("body.cinnabar_spine")
    );
    assert_eq!(find("sleeve.cinnabar_wrist").cubes[0].inflate.get(), 0.25);
    let upper = &find("leftarm").cubes[0];
    let lower = &find("leftarm.cinnabar_elbow").cubes[0];
    assert!(
        upper.origin[1].get() < 17.0 && lower.origin[1].get() + lower.size[1].get() > 17.0,
        "elbow overlap closes the hinge"
    );
    let EntityGeometryUv::Faces(uv) = &lower.uv else {
        panic!("cropped UVs")
    };
    assert_eq!(
        uv.north.as_ref().unwrap().uv_size.unwrap()[1].get(),
        lower.size[1].get()
    );
    let mut slim = (*source).clone();
    slim.bones[4].cubes[0].size[0] = Scalar::new(3.0).unwrap();
    let slim = build(&slim, CustomEmote::Breakdance).unwrap();
    for name in [
        "leftarm",
        "leftarm.cinnabar_elbow",
        "leftarm.cinnabar_wrist",
    ] {
        assert_eq!(
            slim.bones
                .iter()
                .find(|part| part.name.eq_ignore_ascii_case(name))
                .unwrap()
                .cubes[0]
                .size[0]
                .get(),
            3.0
        );
    }
}

#[test]
fn breakdance_keeps_head_only_face_layers_attached_to_the_body() {
    use crate::custom_emotes::{
        sample_custom_emote,
        tests::{Fixture, near, point},
    };
    for (source, offset) in [
        (fixture(), [0.0; 3]),
        (full_fixture(), [0.0; 3]),
        (full_fixture(), [0.25, 0.5, 0.75]),
    ] {
        let mut face = (*source).clone();
        let mut head = face.bones[3].clone();
        head.parent = None;
        head.pivot = Some(std::array::from_fn(|axis| {
            Scalar::new(head.pivot.unwrap()[axis].get() + offset[axis]).unwrap()
        }));
        face.bones = vec![head].into();
        face.poly_meshes = vec![None].into();
        face.digest = Sha256::digest(source.digest).into();
        let face = Arc::new(face);
        let (bones, _) = crate::geometry::skeleton(&face.bones).unwrap();
        let rest: Arc<[_]> = crate::pose::compose_pose(&bones, &[]).unwrap().into();
        let layers = [crate::SkinRenderLayer {
            image: render_api::SkinAnimation {
                kind: render_api::SkinAnimationKind::Face,
                width: 16,
                height: 16,
                rgba8: Arc::from([]),
                frames: 1,
                blinking: false,
            },
            geometry: Arc::clone(&face),
            mesh: None,
            previous: Arc::clone(&rest),
            current: Arc::clone(&rest),
            rest: Arc::clone(&rest),
            hidden_bones: Arc::from([]),
            uv_anim: [0.0, 0.0, 1.0, 1.0],
        }];
        let f = Fixture::from_skin((*source).clone());
        let mut original = f.rig();
        original.skin_layers = &layers;
        let period = CustomEmote::Breakdance.duration_seconds();
        for step in 0..=64 {
            let previous = period * f64::from(step) / 64.0;
            let current = period * f64::from(step + 1) / 64.0;
            let pose =
                sample_custom_emote(&original, CustomEmote::Breakdance, previous, current).unwrap();
            let rig = pose.snapshot(original);
            let head = rig
                .bone_names
                .iter()
                .position(|name| name.as_ref() == "head")
                .unwrap();
            for (body, layer) in [
                (pose.previous[head], pose.skin_layers[0].previous[0]),
                (pose.current[head], pose.skin_layers[0].current[0]),
            ] {
                let offset = std::array::from_fn(|axis| {
                    rest[0].translation_scale[axis] - rig.rest[head].translation_scale[axis]
                });
                near(point(body, offset), point(layer, [0.0; 3]));
                for axis in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
                    near(
                        crate::pose::rotate_vector(body.rotation, axis),
                        crate::pose::rotate_vector(layer.rotation, axis),
                    );
                }
            }
        }
        assert!(Arc::ptr_eq(&layers[0].geometry, &face));
        assert_eq!(layers[0].current, rest);
    }
}

#[test]
fn breakdance_item_anchors_follow_wrist_rotation_and_bind_offsets() {
    use crate::custom_emotes::{
        sample_custom_emote,
        tests::{Fixture, near, point},
    };
    let mut source = (*full_fixture()).clone();
    let mut bones = source.bones.to_vec();
    for (root, name) in [(4, "leftItem"), (5, "rightItem")] {
        let mut anchor = bones[root].clone();
        anchor.name = name.into();
        anchor.parent = Some(bones[root].name.clone());
        anchor.pivot = Some([
            bones[root].pivot.unwrap()[0],
            Scalar::new(12.0).unwrap(),
            Scalar::new(1.0).unwrap(),
        ]);
        anchor.cubes = Box::new([]);
        bones.push(anchor);
    }
    source.poly_meshes = vec![None; bones.len()].into();
    source.bones = bones.into();
    source.digest = Sha256::digest(b"breakdance item anchor fixture").into();
    let f = Fixture::from_skin(source.clone());
    let period = CustomEmote::Breakdance.duration_seconds();
    for step in 0..=64 {
        let previous = period * f64::from(step) / 64.0;
        let current = period * f64::from(step + 1) / 64.0;
        let pose =
            sample_custom_emote(&f.rig(), CustomEmote::Breakdance, previous, current).unwrap();
        let rig = pose.snapshot(f.rig());
        let find = |name: &str| {
            rig.bone_names
                .iter()
                .position(|part| part.as_ref() == name)
                .unwrap()
        };
        for (anchor, wrist) in [
            ("leftitem", "leftarm.cinnabar_wrist"),
            ("rightitem", "rightarm.cinnabar_wrist"),
        ] {
            let (anchor, wrist) = (find(anchor), find(wrist));
            let offset = std::array::from_fn(|axis| {
                rig.rest[anchor].translation_scale[axis] - rig.rest[wrist].translation_scale[axis]
            });
            for endpoint in [&pose.previous, &pose.current] {
                near(
                    point(endpoint[anchor], [0.0; 3]),
                    point(endpoint[wrist], offset),
                );
                for axis in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
                    near(
                        crate::pose::rotate_vector(endpoint[anchor].rotation, axis),
                        crate::pose::rotate_vector(endpoint[wrist].rotation, axis),
                    );
                }
            }
        }
        for (name, rest) in f.rig().bone_names.iter().zip(&f.rest) {
            assert_eq!(rig.rest[find(name)], *rest, "preserve the source bind pose");
        }
    }
    assert_eq!(*f.rig().skin_geometry.unwrap().as_ref(), source);
}

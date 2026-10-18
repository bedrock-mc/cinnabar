use super::*;

mod visibility;

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
fn catalog_preserves_unsupported_partial_geometry() {
    use crate::custom_emotes::{sample_custom_emote, tests::Fixture};

    let source = fixture();
    let f = Fixture::from_skin((*source).clone());
    let rig = f.rig();
    let pose =
        sample_custom_emote(&rig, crate::custom_emotes::tests::catalog_emote(), 0.0, 2.0).unwrap();
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
    let source = full_fixture();
    let original = (*source).clone();
    let model = model(&source).unwrap().geometry;
    assert_eq!(*source, original);
    assert_ne!(model.digest, source.digest);
    assert!(Arc::ptr_eq(
        &model,
        &super::model(&source).unwrap().geometry
    ));
    assert!(model.bones.len() > source.bones.len());
    let upper = &model.bones[6].cubes[0];
    let lower = &model
        .bones
        .iter()
        .find(|bone| bone.name.as_ref() == "leftLeg.cinnabar_knee")
        .unwrap()
        .cubes[0];
    assert_eq!(upper.origin[1].get(), 5.0);
    assert_eq!(upper.size[1].get(), 7.0);
    assert_eq!(lower.origin[1].get(), 2.0);
    assert_eq!(lower.size[1].get(), 5.0);
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
        [4.0, 7.0]
    );
    assert_eq!(
        lower.north.as_ref().unwrap().uv.map(Scalar::get),
        [4.0, 25.0]
    );
    assert_ne!(
        lower.down, upper.down,
        "a knee must not acquire a boot sole"
    );
    assert!(upper.down.is_some() && lower.up.is_some());
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
    use crate::SkinRenderLayer;
    use crate::custom_emotes::{sample_custom_emote, tests::Fixture};

    let source = full_fixture();
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
    for phase in [
        0.0,
        0.1,
        0.3,
        crate::custom_emotes::tests::catalog_emote().duration_seconds(),
    ] {
        let pose = sample_custom_emote(
            &rig,
            crate::custom_emotes::tests::catalog_emote(),
            phase,
            phase,
        )
        .unwrap();
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
fn shared_emotes_reuse_prepared_meshes_across_many_distinct_skins() {
    let models: Vec<_> = (0u64..24)
        .map(|index| {
            let mut source = (*full_fixture()).clone();
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
    source.digest = Sha256::digest(b"original full cuboid catalog test fixture").into();
    Arc::new(source)
}

#[test]
fn catalog_floor_correction_preserves_root_lift_in_the_full_pose_and_attachments() {
    use crate::custom_emotes::{
        sample_custom_emote,
        tests::{Fixture, near},
    };
    let source = full_fixture();
    let f = Fixture::from_skin((*source).clone());
    let layer = crate::SkinRenderLayer {
        image: render_api::SkinAnimation {
            kind: render_api::SkinAnimationKind::Face,
            width: 16,
            height: 16,
            rgba8: Arc::from([]),
            frames: 1,
            blinking: false,
        },
        geometry: source,
        mesh: None,
        previous: f.rest.clone().into(),
        current: f.rest.clone().into(),
        rest: f.rest.clone().into(),
        hidden_bones: Arc::from([]),
        uv_anim: [0.0, 0.0, 1.0, 1.0],
    };
    let layers = [layer];
    let rig = crate::ActorRigSnapshot {
        skin_layers: &layers,
        ..f.rig()
    };
    let mut poses = Vec::new();
    for lift in [false, true] {
        let bytes = format!(r#"{{"version":1,"duration":1,"loop":false,"tracks":[{{"joint":"root","keys":[{{"time":0,"rotation":[0,0,0,1],"offset":[0,0,0]}},{{"time":1,"rotation":[0,0,0,1],"offset":[0,{},0]}}]}}]}}"#, if lift { 0.5 } else { 0.0 }).into_bytes();
        let mut descriptor = cosmetics::test_support::emote("fixture:lift", "Lift", true)
            .descriptor()
            .clone();
        descriptor.clip.sha256 = format!("{:x}", Sha256::digest(&bytes));
        descriptor.clip.bytes = bytes.len();
        let clip = cosmetics::emotes::Item::admit(descriptor, &bytes).unwrap();
        poses.push(sample_custom_emote(&rig, clip, 0.25, 1.0).unwrap());
    }
    assert!(poses[1].articulated.is_some());
    for (a, b, lift) in [
        (&poses[0].previous, &poses[1].previous, 1.5),
        (&poses[0].current, &poses[1].current, 6.0),
        (
            &poses[0].skin_layers[0].previous,
            &poses[1].skin_layers[0].previous,
            1.5,
        ),
        (
            &poses[0].skin_layers[0].current,
            &poses[1].skin_layers[0].current,
            6.0,
        ),
    ] {
        for (a, b) in a.iter().zip(b.iter()) {
            near(
                b.translation_scale[..3].try_into().unwrap(),
                [
                    a.translation_scale[0],
                    a.translation_scale[1] + lift,
                    a.translation_scale[2],
                ],
            );
            assert_eq!(a.rotation, b.rotation);
        }
    }
}

#[test]
fn catalog_bends_elbows_knees_and_spine_without_changing_native_geometry() {
    use crate::custom_emotes::{sample_custom_emote, tests::Fixture};
    let source = full_fixture();
    let original = (*source).clone();
    let f = Fixture::from_skin((*source).clone());
    let pose = sample_custom_emote(
        &f.rig(),
        crate::custom_emotes::tests::catalog_emote(),
        0.0,
        0.24 * crate::custom_emotes::tests::catalog_emote().duration_seconds(),
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
fn catalog_mesh_preserves_skin_clothing_slim_arms_and_clip_cache_identity() {
    let source = full_fixture();
    let model = model(&source).unwrap();
    assert!(Arc::ptr_eq(
        &model.geometry,
        &super::model(&source).unwrap().geometry
    ));
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
    let slim = build(&slim).unwrap().geometry;
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
fn catalog_keeps_head_only_face_layers_attached_to_the_body() {
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
        let period = crate::custom_emotes::tests::catalog_emote().duration_seconds();
        for step in 0..=64 {
            let previous = period * f64::from(step) / 64.0;
            let current = period * f64::from(step + 1) / 64.0;
            let pose = sample_custom_emote(
                &original,
                crate::custom_emotes::tests::catalog_emote(),
                previous,
                current,
            )
            .unwrap();
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
fn catalog_item_anchors_follow_wrist_rotation_and_bind_offsets() {
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
    source.digest = Sha256::digest(b"catalog item anchor fixture").into();
    let f = Fixture::from_skin(source.clone());
    let period = crate::custom_emotes::tests::catalog_emote().duration_seconds();
    for step in 0..=64 {
        let previous = period * f64::from(step) / 64.0;
        let current = period * f64::from(step + 1) / 64.0;
        let pose = sample_custom_emote(
            &f.rig(),
            crate::custom_emotes::tests::catalog_emote(),
            previous,
            current,
        )
        .unwrap();
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

#[test]
fn installed_cape_hangs_behind_the_body_with_its_front_raster_outward() {
    use glam::{Quat, Vec3};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/assets/compiled/vanilla-v1.mcbeent");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping installed cape fixture: missing {}; make assets",
                path.display()
            );
            return;
        }
        Err(error) => panic!("read installed cape fixture {}: {error}", path.display()),
    };
    let assets = assets::RuntimeEntityAssets::decode(&bytes).unwrap();
    let cape = super::CapeRig::resolve(&assets).expect("installed cape geometry");
    let names: Vec<Box<str>> = ["root", "body", "waist", "cape"].map(Into::into).into();
    for tilt in [0.0, 0.45] {
        for flap in [6.0_f32.to_radians(), 0.7] {
            let [body, cape_pose] = fixture_poses(tilt, flap);
            let root = crate::RenderBoneTransform::from_model_space(
                [0.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0, 1.0],
            )
            .unwrap();
            let rest = [
                root,
                fixture_poses(0.0, 0.0)[0],
                root,
                fixture_poses(0.0, 0.0)[1],
            ];
            let poses = retarget(&cape, &names, &rest, &[root, body, root, cape_pose]);
            let index = cape
                .bone_names
                .iter()
                .position(|name| &**name == "cape")
                .unwrap();
            let rotation = Quat::from_array(poses[index].rotation);
            let parent = Quat::from_rotation_x(tilt);
            let pivot = Vec3::from_array(cape.geometry.bone_pivots[index]);
            let direction = parent * Quat::from_rotation_x(-flap) * Vec3::Z;
            let outward = cape
                .geometry
                .vertices
                .iter()
                .filter(|vertex| (rotation * Vec3::from_array(vertex.normal)).dot(direction) > 0.99)
                .collect::<Vec<_>>();
            assert_eq!(outward.len(), 6);
            assert!(
                outward.iter().all(|vertex| vertex.uv[0] <= 11.0 / 64.0),
                "cape exterior must display the front artwork"
            );
            for vertex in cape
                .geometry
                .vertices
                .iter()
                .filter(|vertex| vertex.position[1] < 0.51)
            {
                let edge = rotation * (Vec3::from_array(vertex.position) - pivot);
                assert!(
                    (parent.inverse() * edge).z > 0.0,
                    "cape hangs behind the shoulder"
                );
            }
        }
    }
}

/// Builds an original cape rig for pose and wing-routing tests.
pub(super) fn fixture_cape() -> super::CapeRig {
    let model = fixture_model();
    let geometry = crate::skin_geometry(&model, crate::EntityRigId(1)).unwrap();
    super::CapeRig::new(geometry, &model.bones)
}

/// Supplies the original authored cape bones for independent appearance-binding tests.
fn fixture_model() -> assets::SkinGeometry {
    let source = serde_json::json!({
        "format_version":"1.12.0",
        "minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture","texture_width":64,"texture_height":32},
            "bones":[{"name":"body","pivot":[0,24,0]},
                {"name":"cape","parent":"body","pivot":[0,24,3],"bind_pose_rotation":[0,180,0],
                "cubes":[{"origin":[-5,8,3],"size":[10,16,1],"uv":[0,0]}]}]
        }]
    });
    assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.fixture"}}"#,
        &source.to_string(),
    )
    .unwrap()
    .unwrap()
}

/// Supplies immutable render-space rest frames to the shared cape pose.
fn retarget(
    cape: &super::CapeRig,
    names: &[Box<str>],
    rest: &[crate::RenderBoneTransform],
    body: &[crate::RenderBoneTransform],
) -> std::sync::Arc<[crate::RenderBoneTransform]> {
    cape.pose(names, |index| rest.get(index).copied(), body, None)
}

/// Supplies a torso and its animated cape hinge in block coordinates.
fn fixture_poses(tilt: f32, flap: f32) -> [crate::RenderBoneTransform; 2] {
    use glam::Quat;
    let parent = Quat::from_rotation_x(tilt);
    let pose = |rotation: Quat, z: f32| crate::RenderBoneTransform {
        rotation: rotation.to_array(),
        translation_scale: [0.0, 1.5, z, 1.0],
        axis_scale: crate::UNIT_AXIS_SCALE,
    };
    [
        pose(parent, 0.0),
        pose(parent * Quat::from_rotation_x(flap), 3.0 / 16.0),
    ]
}

#[test]
fn slim_avatar_cape_uses_the_cape_models_shoulder_attachment() {
    let cape = fixture_cape();
    let mut body = fixture_poses(0.0, 0.1);
    body[1].translation_scale[2] = -3.0 / 16.0;
    let mut rest = fixture_poses(0.0, 0.0);
    rest[1].translation_scale[2] = -3.0 / 16.0;
    let poses = retarget(&cape, &["body".into(), "cape".into()], &rest, &body);
    assert!(
        poses[1].translation_scale[2] > body[0].translation_scale[2],
        "a slim avatar's cape must attach behind the torso, not across its chest"
    );
}

#[test]
fn cape_outer_face_keeps_the_front_raster_under_parent_tilt() {
    use glam::{Quat, Vec3};
    let cape = fixture_cape();
    for tilt in [0.0, 0.45] {
        let parent = Quat::from_rotation_x(tilt);
        let poses = retarget(
            &cape,
            &["body".into(), "cape".into()],
            &fixture_poses(0.0, 0.0),
            &fixture_poses(tilt, 0.0),
        );
        let rotation = Quat::from_array(poses[1].rotation);
        let outward = cape
            .geometry
            .vertices
            .iter()
            .filter(|vertex| {
                (rotation * Vec3::from_array(vertex.normal)).dot(parent * Vec3::Z) > 0.99
            })
            .collect::<Vec<_>>();
        assert_eq!(outward.len(), 6);
        assert!(
            outward.iter().all(|vertex| vertex.uv[0] <= 11.0 / 64.0),
            "outward face must sample the front strip, not the inside strip"
        );
    }
}

#[test]
fn unresolved_parent_keeps_the_completed_pose() {
    let cape = fixture_cape();
    let body = fixture_poses(0.45, 0.7);
    let poses = retarget(&cape, &["cape".into()], &[], &body[1..]);
    assert_eq!(poses[1].rotation, body[1].rotation);
    assert_eq!(poses[1].translation_scale, body[1].translation_scale);
}

#[test]
fn mixed_axis_bind_uses_mesh_rotation_order() {
    use glam::{Quat, Vec3};
    let mut bone: assets::EntityGeometryBone = serde_json::from_value(serde_json::json!({
        "name":"cape", "cubes":[]
    }))
    .unwrap();
    bone.bind_pose_rotation =
        Some([25.0, 50.0, 15.0].map(|angle| assets::EntityGeometryScalar::new(angle).unwrap()));
    let frame = super::pose_frames(&[bone], &[[0.0; 3]]);
    let expected = Quat::from_rotation_z(15.0_f32.to_radians())
        * Quat::from_rotation_y(-50.0_f32.to_radians())
        * Quat::from_rotation_x(-25.0_f32.to_radians());
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        assert!((frame[0].bind * axis - expected * axis).length() < 1e-5);
    }
}

#[test]
fn moving_cape_trails_behind_the_body_without_reversing_parent_tilt() {
    use glam::{Quat, Vec3};
    let cape = fixture_cape();
    for tilt in [0.0, 0.45] {
        for flap in [0.2, 0.7, 1.4, 2.3] {
            let body = fixture_poses(tilt, flap);
            let poses = retarget(
                &cape,
                &["body".into(), "cape".into()],
                &fixture_poses(0.0, 0.0),
                &body,
            );
            for (actual, expected) in poses[1]
                .translation_scale
                .iter()
                .zip(body[1].translation_scale)
            {
                assert!(
                    (actual - expected).abs() < 1e-6,
                    "shoulder hinge stays attached"
                );
            }
            let rotation = Quat::from_array(poses[1].rotation);
            let parent = Quat::from_rotation_x(tilt);
            let down = parent.inverse() * rotation * Vec3::NEG_Y;
            assert!(down.y <= 1e-6, "cape must stay below its shoulder");
            let pivot = Vec3::from_array(cape.geometry.bone_pivots[1]);
            for vertex in cape
                .geometry
                .vertices
                .iter()
                .filter(|vertex| vertex.position[1] < 0.51)
            {
                let from_hinge = rotation * (Vec3::from_array(vertex.position) - pivot);
                assert!(
                    (parent.inverse() * from_hinge).z > 0.0,
                    "cape lower edge must swing behind the shoulder, not over the chest"
                );
            }
        }
    }
}

#[test]
fn hanging_cape_keeps_authored_yaw_and_roll_across_a_quarter_turn() {
    use glam::{Quat, Vec3};
    let cape = fixture_cape();
    let bind = cape.pose_frames[1].bind;
    let rest = fixture_poses(0.0, 0.0);
    for tilt in [0.0, 0.45] {
        let parent = Quat::from_rotation_x(tilt);
        for yaw in [
            -179.0_f32, -135.0, -91.0, -90.0, -89.0, 0.0, 89.0, 90.0, 91.0, 135.0, 179.0,
        ] {
            let yaw = Quat::from_rotation_y(yaw.to_radians());
            for local in [
                yaw,
                yaw * Quat::from_rotation_x(0.35) * Quat::from_rotation_z(0.2),
            ] {
                let mut body = fixture_poses(tilt, 0.0);
                body[1].rotation = (parent * local).to_array();
                let expected = parent * bind * local * bind.inverse();
                assert!((parent.inverse() * expected * Vec3::NEG_Y).y < 0.0);
                let poses = retarget(&cape, &["body".into(), "cape".into()], &rest, &body);
                let actual = Quat::from_array(poses[1].rotation);
                for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                    assert!(
                        (actual * axis - expected * axis).length() < 1e-5,
                        "a hanging cape must keep its complete authored rotation"
                    );
                }
                assert!(
                    (super::pose::position(&poses[1]) - super::pose::position(&body[1])).length()
                        < 1e-6
                );
            }
        }
    }
}

#[test]
fn upward_cape_directions_are_bounded_after_bind_rotation() {
    use glam::{Quat, Vec3};
    let cape = fixture_cape();
    let bind = cape.pose_frames[1].bind;
    let rest = fixture_poses(0.0, 0.0);
    for tilt in [0.0, 0.45] {
        let parent = Quat::from_rotation_x(tilt);
        for local in [
            Quat::from_rotation_x((super::CAPE_MAX_SWING_DEGREES + 0.01).to_radians()),
            Quat::from_rotation_x(2.3),
            Quat::from_rotation_y(1.8) * Quat::from_rotation_x(2.3),
            Quat::from_rotation_z(2.3),
            Quat::from_rotation_x(std::f32::consts::PI),
        ] {
            let original = bind * local * bind.inverse() * Vec3::NEG_Y;
            assert!(original.y > 0.0);
            let mut body = fixture_poses(tilt, 0.0);
            body[1].rotation = (parent * local).to_array();
            let poses = retarget(&cape, &["body".into(), "cape".into()], &rest, &body);
            let down = parent.inverse() * Quat::from_array(poses[1].rotation) * Vec3::NEG_Y;
            assert!(
                down.y <= 1e-6,
                "cape must stay below its shoulder: {down:?}"
            );
            let horizontal = Vec3::new(original.x, 0.0, original.z);
            if horizontal.length_squared() > 1e-6 {
                assert!(
                    down.dot(horizontal.normalize()) > 1.0 - 1e-5,
                    "the bound must preserve the cape's horizontal direction"
                );
            }
            assert!(
                (super::pose::position(&poses[1]) - super::pose::position(&body[1])).length()
                    < 1e-6
            );
        }
    }
}

#[test]
fn cape_retargeting_preserves_neck_motion_under_parent_rotation_and_scale() {
    use glam::{Quat, Vec3};
    let cape = fixture_cape();
    let parent = Quat::from_rotation_x(0.45);
    let scale = Vec3::new(1.1, 0.8, 1.3);
    let origin = Vec3::new(0.2, 1.4, -0.1);
    let motion = Vec3::new(0.0, 0.04, 0.0);
    let expected = origin + parent * ((Vec3::new(0.0, 0.0, 3.0 / 16.0) + motion) * scale);
    for rest_z in [-3.0 / 16.0, 3.0 / 16.0] {
        let mut rest = fixture_poses(0.0, 0.0);
        rest[1].translation_scale[2] = rest_z;
        let mut body = fixture_poses(0.45, 0.7);
        body[0].translation_scale[..3].copy_from_slice(&origin.to_array());
        body[0].axis_scale[..3].copy_from_slice(&scale.to_array());
        body[1].axis_scale[..3].copy_from_slice(&scale.to_array());
        let source = origin + parent * ((Vec3::new(0.0, 0.0, rest_z) + motion) * scale);
        body[1].translation_scale[..3].copy_from_slice(&source.to_array());
        let poses = retarget(&cape, &["body".into(), "cape".into()], &rest, &body);
        assert!((super::pose::position(&poses[1]) - expected).length() < 1e-6);
        assert_eq!(poses[1].axis_scale, body[1].axis_scale);
    }
}

#[test]
fn prepared_cape_indices_preserve_missing_duplicate_case_and_skin_reordering() {
    let cape = fixture_cape();
    let mut body = fixture_poses(0.45, 0.3).to_vec();
    let rest = fixture_poses(0.0, 0.0).to_vec();
    for names in [
        vec!["body", "cape"],
        vec!["CAPE", "BODY"],
        vec!["cape"],
        vec!["body", "missing"],
        vec!["body", "cape", "CAPE"],
        vec![],
    ] {
        let names: Vec<Box<str>> = names.into_iter().map(Into::into).collect();
        body.resize(names.len(), fixture_poses(0.2, 0.1)[0]);
        let bound = crate::ActorAttachmentBindings::from_cape(&fixture_model().bones, &names);
        for java in [None, Some(crate::java_animation::JavaCapeInput::default())] {
            assert_eq!(
                cape.pose(
                    &names,
                    |index| rest.get(index).copied(),
                    &body,
                    java.as_ref()
                ),
                cape.pose_bound(
                    bound.cape(),
                    |index| rest.get(index).copied(),
                    &body,
                    java.as_ref()
                )
            );
        }
    }
}

#[test]
fn cosmetic_binding_payload_does_not_grow_with_catalog() {
    let names = vec![Box::<str>::from("body")];
    let bones = fixture_model().bones;
    let before = crate::ActorAttachmentBindings::from_cape(&bones, &names);
    let seed = crate::hats::fixture::cloth().descriptor().clone();
    let items: Vec<_> = (0..16)
        .map(|index| {
            let mut descriptor = seed.clone();
            descriptor.id = format!("fixture:sparse_anchor_{index}");
            descriptor.attachment_bone = "BODY".into();
            cosmetics::Item::admit(descriptor).unwrap()
        })
        .collect();
    let after = crate::ActorAttachmentBindings::from_cape(&bones, &names);
    assert_eq!(after.retained_bytes(), before.retained_bytes());
    for item in items {
        assert_eq!(after.anchor(item), Some(0));
    }
}

#[test]
fn prepared_cosmetic_anchors_rebind_skins_and_retain_equipped_catalog_revisions() {
    let original = crate::hats::fixture::cloth();
    let mut descriptor = original.descriptor().clone();
    descriptor.id = "fixture:anchor_revisions".into();
    descriptor.attachment_bone = "BODY".into();
    let old = cosmetics::Item::admit(descriptor.clone()).unwrap();
    let names: Vec<Box<str>> = ["body", "rightitem", "BODY"]
        .into_iter()
        .map(Into::into)
        .collect();
    let initial = std::sync::Arc::new(crate::ActorAttachmentBindings::from_cape(
        &fixture_model().bones,
        &names,
    ));
    assert_eq!(initial.anchor(old), Some(0));
    descriptor.attachment_bone = "rightitem".into();
    let new = cosmetics::Item::admit(descriptor).unwrap();
    assert_eq!(initial.anchor(new), Some(1));
    let mut cache = crate::hats::HatPoseCache::default();
    let before = crate::actor_bindings::ANCHOR_LOOKUPS.with(std::cell::Cell::get);
    assert_eq!(cache.anchor(new, None, &names), Some(1));
    assert_eq!(
        crate::actor_bindings::ANCHOR_LOOKUPS.with(std::cell::Cell::get),
        before + 1
    );
    let before = crate::actor_bindings::ANCHOR_LOOKUPS.with(std::cell::Cell::get);
    assert_eq!(cache.anchor(new, Some(&initial), &names), Some(1));
    let bound = crate::actor_bindings::ANCHOR_LOOKUPS.with(std::cell::Cell::get);
    assert_eq!(bound, before);
    for _ in 0..8 {
        assert_eq!(cache.anchor(new, Some(&initial), &names), Some(1));
    }
    assert_eq!(
        crate::actor_bindings::ANCHOR_LOOKUPS.with(std::cell::Cell::get),
        bound
    );
    let rebound = crate::ActorAttachmentBindings::from_cape(&fixture_model().bones, &names);
    for item in [old, new] {
        assert_eq!(
            rebound.anchor(item),
            names
                .iter()
                .position(|name| name.eq_ignore_ascii_case(&item.descriptor().attachment_bone))
        );
    }
    let names: Vec<Box<str>> = ["rightitem", "body"].into_iter().map(Into::into).collect();
    let changed = crate::ActorAttachmentBindings::from_cape(&fixture_model().bones, &names);
    assert_eq!(changed.anchor(old), Some(1));
    assert_eq!(changed.anchor(new), Some(0));
    assert_eq!(
        crate::ActorAttachmentBindings::from_cape(&fixture_model().bones, &[]).anchor(old),
        None
    );
}

#[test]
fn prepared_cape_indices_decline_a_different_cape_with_the_same_bone_count() {
    let model = fixture_model();
    let names = vec![Box::<str>::from("body"), Box::<str>::from("cape")];
    let bound = crate::ActorAttachmentBindings::from_cape(&model.bones, &names);
    let cape = fixture_cape();
    assert!(bound.cape_for(&cape).is_some());
    let mut different = model.bones.into_vec();
    different[0].name = "torso".into();
    different[1].parent = Some("torso".into());
    let other = super::CapeRig::new(cape.geometry.clone(), &different);
    assert!(bound.cape_for(&other).is_none());
}

use super::*;

#[test]
fn cosmetic_preview_vertices_follow_each_biped_attachment_bone() {
    let source = fixture::bunny();
    for (part, bone) in crate::STANDARD_BIPED_BONES.iter().enumerate() {
        let mut descriptor = source.descriptor().clone();
        descriptor.attachment_bone = bone.to_uppercase();
        let geometry = serde_json::json!({
            "format_version": "1.12.0",
            "minecraft:geometry": [{
                "description": {"identifier": descriptor.geometry_id, "texture_width": rig(source).texture.width, "texture_height": rig(source).texture.height},
                "bones": [{"name": descriptor.attachment_bone, "pivot": [0, 24, 0], "cubes": [{"origin": [0, 24, 0], "size": [1, 1, 1], "uv": [0, 0]}]}]
            }]
        });
        let model = model::build(
            &descriptor,
            &geometry.to_string(),
            include_bytes!("bunny_ears.png"),
            &[],
        )
        .unwrap();
        assert!(!model.rest_vertices.is_empty());
        assert!(
            model
                .rest_vertices
                .iter()
                .all(|vertex| vertex.part == part as u32)
        );
    }
}

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
    let model = rig(fixture::bunny());
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
    let model = rig(fixture::bunny());
    for rotation in [
        Quat::IDENTITY,
        Quat::from_rotation_y(1.3),
        Quat::from_rotation_x(-0.4),
    ] {
        let rest = model.pose(head(rotation), Motion::default());
        let folded = model.pose(
            head(rotation),
            Motion {
                bend: 1.0,
                ..Default::default()
            },
        );
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
    let walk = EarWind::default().advance(fixture::bunny(), 3.0, 1.0);
    let run = EarWind::default().advance(fixture::bunny(), 6.0, 1.0);
    assert!(run > walk && run <= 65.0_f32.to_radians());
    let mut slow = EarWind::default();
    let mut fast = EarWind::default();
    for _ in 0..30 {
        slow.advance(fixture::bunny(), 5.0, 1.0 / 30.0);
    }
    for _ in 0..120 {
        fast.advance(fixture::bunny(), 5.0, 1.0 / 120.0);
    }
    assert!((slow.bend - fast.bend).abs() < 1e-4);
    let before = fast.bend;
    let after = fast.advance(fixture::bunny(), -3.0, 0.1);
    assert!(after > 0.0 && after < before);
    for _ in 0..120 {
        fast.advance(fixture::bunny(), 0.0, 1.0 / 30.0);
    }
    assert_eq!(fast.bend, 0.0);
    assert_eq!(fast.advance(fixture::bunny(), f32::NAN, f32::INFINITY), 0.0);
}

#[test]
fn settled_headwear_reuses_its_pose_and_retains_head_scale() {
    let mut cache = HatPoseCache::default();
    let mut head = head(Quat::IDENTITY);
    head.translation_scale[3] = 0.5;
    let first = cache.pose(fixture::bunny(), head, Motion::default());
    let second = cache.pose(fixture::bunny(), head, Motion::default());
    assert!(Arc::ptr_eq(&first, &second));
    for bone in &*first {
        assert_eq!(bone.translation_scale[3], 0.5);
    }
    let changed = cache.pose(
        fixture::bunny(),
        head,
        Motion {
            bend: 0.5,
            ..Default::default()
        },
    );
    assert!(!Arc::ptr_eq(&first, &changed));
}

#[test]
fn shared_surface_edges_stay_welded_and_joint_angles_form_a_curve() {
    let model = rig(fixture::bunny());
    for bend in [0.0, 0.5, 1.0] {
        let pose = model.pose(
            head(Quat::IDENTITY),
            Motion {
                bend,
                ..Default::default()
            },
        );
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
    for (project, geometry) in [
        (
            include_str!("shoulder_parrot.bbmodel"),
            include_str!("shoulder_parrot.geo.json"),
        ),
        (
            include_str!("dragon_wings.bbmodel"),
            include_str!("dragon_wings.geo.json"),
        ),
        (
            include_str!("bunny_ears.bbmodel"),
            include_str!("bunny_ears.geo.json"),
        ),
        (
            include_str!("crown.bbmodel"),
            include_str!("crown.geo.json"),
        ),
        (
            include_str!("devil_horns.bbmodel"),
            include_str!("devil_horns.geo.json"),
        ),
        (
            include_str!("fox_tail.bbmodel"),
            include_str!("fox_tail.geo.json"),
        ),
        (
            include_str!("pixel_shades.bbmodel"),
            include_str!("pixel_shades.geo.json"),
        ),
        (
            include_str!("bee_buddy.bbmodel"),
            include_str!("bee_buddy.geo.json"),
        ),
        (
            include_str!("morpho_wings.bbmodel"),
            include_str!("morpho_wings.geo.json"),
        ),
        (
            include_str!("knit_scarf.bbmodel"),
            include_str!("knit_scarf.geo.json"),
        ),
        (
            include_str!("kitsune_mask.bbmodel"),
            include_str!("kitsune_mask.geo.json"),
        ),
        (
            include_str!("studio_headphones.bbmodel"),
            include_str!("studio_headphones.geo.json"),
        ),
        (include_str!("halo.bbmodel"), include_str!("halo.geo.json")),
        (
            include_str!("ember_jetpack.bbmodel"),
            include_str!("ember_jetpack.geo.json"),
        ),
        (
            include_str!("amethyst_orbit.bbmodel"),
            include_str!("amethyst_orbit.geo.json"),
        ),
    ] {
        let project: serde_json::Value = serde_json::from_str(project).unwrap();
        let exported: serde_json::Value = serde_json::from_str(geometry).unwrap();
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
        let cubes: Vec<_> = geometry["bones"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|bone| bone["cubes"].as_array().cloned().unwrap_or_default())
            .collect();
        for cube in project["elements"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|element| element["type"] == "cube")
        {
            let from = cube["from"].as_array().unwrap();
            let size: Vec<f64> = (0..3)
                .map(|axis| cube["to"][axis].as_f64().unwrap() - from[axis].as_f64().unwrap())
                .collect();
            let rotated = cube["rotation"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n.as_f64() != Some(0.0));
            // Repeated parts share origins and sizes, so a cube matches only with all its faces.
            let matches = |candidate: &&serde_json::Value| {
                candidate["origin"] == cube["from"]
                    && (0..3).all(|axis| {
                        (candidate["size"][axis].as_f64().unwrap() - size[axis]).abs() < 1e-9
                    })
                    && (!rotated
                        || (candidate["rotation"] == cube["rotation"]
                            && candidate["pivot"] == cube["origin"]))
                    && cube["faces"]
                        .as_object()
                        .unwrap()
                        .iter()
                        .all(|(name, face)| {
                            let uv = face["uv"].as_array().unwrap();
                            if face["texture"].is_null() {
                                return candidate["uv"].get(name).is_none();
                            }
                            let corner = |i: usize| uv[i].as_f64().unwrap();
                            let exported = &candidate["uv"][name];
                            exported["uv"] == serde_json::json!([uv[0], uv[1]])
                                && [
                                    exported["uv_size"][0].as_f64(),
                                    exported["uv_size"][1].as_f64(),
                                ] == [Some(corner(2) - corner(0)), Some(corner(3) - corner(1))]
                        })
            };
            assert!(
                cubes.iter().any(|candidate| matches(&candidate)),
                "Blockbench cubes, transforms and face UVs must be exported together"
            );
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
            assert!(!positions.is_empty());
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
fn admits_new_identifiers_slots_offsets_and_render_flags_without_code_changes() {
    let first = fixture::bunny();
    let mut descriptor = first.descriptor().clone();
    descriptor.id = "fixture:offset".into();
    descriptor.slot = "face".into();
    descriptor.offset = [0.25, 0.0, 0.0];
    descriptor.render_flags.double_sided = true;
    let item = admit(
        descriptor,
        include_bytes!("bunny_ears.geo.json"),
        include_bytes!("bunny_ears.png"),
        include_bytes!("fixture/animation.json"),
    )
    .unwrap();
    assert_eq!(
        rig(item).geometry.vertices.len(),
        rig(first).geometry.vertices.len() * 2
    );
    let posed = rig(item).pose(head(Quat::IDENTITY), Motion::default());
    assert!((posed[0].translation_scale[0] - 0.25).abs() < 1e-5);
    assert_eq!(Item::from_id(item.id()), Some(item));
}

#[test]
fn corrupt_assets_and_unknown_slots_never_admit_a_renderable_handle() {
    let mut descriptor = fixture::bunny().descriptor().clone();
    descriptor.id = "fixture:corrupt".into();
    assert!(
        admit(
            descriptor.clone(),
            b"bad",
            include_bytes!("bunny_ears.png"),
            &[]
        )
        .is_none()
    );
    descriptor.slot = "future:slot".into();
    assert!(!cosmetics::supported(&descriptor));
    assert!(Item::from_id("fixture:corrupt").is_none());
    let slots = [
        ("head".into(), Some(fixture::bunny().id().into())),
        ("back".into(), Some("future:unknown".into())),
    ]
    .into();
    assert_eq!(
        cosmetics::resolve(&slots, |_| false),
        vec![fixture::bunny()]
    );
}

#[test]
fn catalog_geometry_keeps_authored_rotations_at_a_different_attachment_bone() {
    use sha2::{Digest, Sha256};
    let mut geometry: serde_json::Value =
        serde_json::from_str(include_str!("bunny_ears.geo.json")).unwrap();
    let bones = geometry["minecraft:geometry"][0]["bones"]
        .as_array_mut()
        .unwrap();
    bones[0]["name"] = "body".into();
    bones[0]["rotation"] = serde_json::json!([0, 90, 0]);
    for bone in bones {
        if bone["parent"] == "head" {
            bone["parent"] = "body".into();
        }
    }
    let bytes = serde_json::to_vec(&geometry).unwrap();
    let mut descriptor = fixture::bunny().descriptor().clone();
    descriptor.id = "fixture:body-rotation".into();
    descriptor.slot = "back".into();
    descriptor.attachment_bone = "body".into();
    descriptor.assets.geometry.sha256 = format!("{:x}", Sha256::digest(&bytes));
    descriptor.assets.geometry.bytes = bytes.len();
    let item = admit(
        descriptor,
        &bytes,
        include_bytes!("bunny_ears.png"),
        include_bytes!("fixture/animation.json"),
    )
    .unwrap();
    let pose = rig(item).pose(head(Quat::IDENTITY), Motion::default());
    assert!(
        rig(item)
            .rest_vertices
            .iter()
            .all(|vertex| vertex.part == 1)
    );
    let actual = Quat::from_array(pose[0].rotation);
    assert!(
        actual
            .dot(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
            .abs()
            > 0.99999
    );
    assert!(
        rig(item)
            .rest_vertices
            .iter()
            .any(|vertex| vertex.position[2].abs() > 0.2)
    );
}

#[test]
fn catalog_devil_horns_attach_directly_to_the_head_and_stay_rigid() {
    let item = fixture::devil_horns();
    let model = rig(item);
    assert!(item.descriptor().free);
    assert_eq!(item.descriptor().attachment_bone, "head");
    assert!(!model.bends_in_wind());
    let bottom = model
        .rest_vertices
        .iter()
        .map(|v| v.position[1])
        .fold(f32::INFINITY, f32::min);
    assert!(
        (1.98..=2.0).contains(&bottom),
        "horn roots intersect the standard head top"
    );
    assert!(
        model
            .rest_vertices
            .iter()
            .all(|v| v.position[0].abs() <= 0.25)
    );
    assert!(
        model.rest_vertices.as_chunks::<3>().0.iter().all(|face| {
            face.iter().all(|v| v.position[0] > 0.0) || face.iter().all(|v| v.position[0] < 0.0)
        }),
        "no band joins the two horns"
    );
    let attachment = head(Quat::from_rotation_y(0.8));
    assert_eq!(
        *model.pose(attachment, Motion::default()),
        *model.pose(
            attachment,
            Motion {
                bend: 1.0,
                ..Default::default()
            }
        )
    );
    let preview = thumbnail::render(item, 128, 128).unwrap();
    assert!(
        preview
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0)
    );
}

#[test]
fn shoulder_parrot_sits_on_the_shoulder_and_coexists_with_head_and_back_slots() {
    let pet = fixture::shoulder_parrot();
    let model = rig(pet);
    assert_eq!(pet.descriptor().slot, "pet");
    assert_eq!(pet.descriptor().attachment_bone, "body");
    assert!(pet.descriptor().free);
    assert!(
        model
            .rest_vertices
            .iter()
            .all(|v| v.position[0] > 0.25 && v.position[1] < 2.0)
    );
    assert!(
        model
            .rest_vertices
            .iter()
            .any(|v| (v.position[1] - 1.5).abs() < 1e-6),
        "feet touch the standard shoulder plane"
    );
    let slots = [fixture::devil_horns(), fixture::dragon_wings(), pet]
        .map(|item| (item.descriptor().slot.clone(), Some(item.id().into())))
        .into();
    let resolved = cosmetics::resolve(&slots, |_| false);
    assert_eq!(resolved.len(), 3);
    assert!(resolved.contains(&pet));
    let preview = thumbnail::render(pet, 128, 128).unwrap();
    assert!(
        preview
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0)
    );
}

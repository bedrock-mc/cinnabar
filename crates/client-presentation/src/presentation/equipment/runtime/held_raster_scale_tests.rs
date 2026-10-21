//! Java raster placement and owner scale through the complete equipment draw path.

use super::*;
use bevy::math::Mat4;
use render_model::java_animation::{JavaHand, JavaItemMesh};

/// Authors the same opaque raster in two different rig frames.
fn raster_pack(translated: bool) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = held_pack();
    for (path, bytes) in &mut files {
        if path.as_ref() == "attachables/held.json" {
            let mut document: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            document["minecraft:attachable"]["description"]["identifier"] =
                serde_json::json!("minecraft:bow");
            *bytes = serde_json::to_vec(&document).unwrap();
        } else if translated && path.as_ref() == "models/entity/held.json" {
            let mut document: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let bone = &mut document["minecraft:geometry"][0]["bones"][0];
            bone["pivot"] = serde_json::json!([2, 5, -3]);
            bone["texture_meshes"][0] = serde_json::json!({
                "texture": "default", "position": [7, -4, 11],
                "local_pivot": [3, 1, 2], "rotation": [17, 31, -23],
                "scale": [1.5, 0.75, 2]
            });
            *bytes = serde_json::to_vec(&document).unwrap();
        }
    }
    files
}

/// Returns actual camera-space mesh vertices, including their submitted bone transforms.
fn raster_points(translated: bool, scale: f32, off_hand: bool, hand: JavaHand) -> Vec<Vec3> {
    let (mut runtime, _) = startup_runtime(raster_pack(translated));
    let body = player_body(&mut runtime);
    let owner = owner();
    let names = [Box::from("rightItem"), Box::from("leftItem")];
    let mut rig = owner_rig(&owner, &names);
    rig.held_item_scale = Some(scale);
    let item = held("minecraft:bow").main.unwrap();
    let draw = runtime
        .first_person_attachable(
            &body,
            &item,
            &owner,
            &rig,
            actor_animation::AttachableAnimationInput {
                first_person: true,
                off_hand,
                // The sampled owner, rather than a stale input, supplies the factor.
                held_item_scale: Some(7.0),
                ..Default::default()
            },
            Some(hand),
        )
        .expect("synthetic raster must reach the attachable draw path");
    assert!(draw.camera_space);
    let camera = draw
        .java_camera
        .expect("Java raster camera must be selected");
    let geometry = runtime
        .take_pending_geometries()
        .into_iter()
        .find(|mesh| mesh.id == draw.presentation.submission.input.rig)
        .unwrap();
    assert!(!geometry.vertices.is_empty());
    geometry
        .vertices
        .iter()
        .map(|vertex| {
            let index = vertex.bone_index as usize;
            let bone = draw.presentation.submission.input.current_bones[index];
            let matrix = Mat4::from_scale_rotation_translation(
                Vec3::from_slice(&bone.axis_scale[..3]) * bone.translation_scale[3],
                Quat::from_array(bone.rotation),
                Vec3::from_slice(&bone.translation_scale[..3]),
            );
            camera.transform_point3(matrix.transform_point3(
                Vec3::from_array(vertex.position) - Vec3::from_array(geometry.bone_pivots[index]),
            ))
        })
        .collect()
}

/// Compares corresponding generated vertices with a tolerance below a raster texel.
fn assert_points(actual: &[Vec3], expected: &[Vec3]) {
    assert_eq!(actual.len(), expected.len());
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            actual.abs_diff_eq(*expected, 2e-5),
            "vertex {index}: {actual:?} vs {expected:?}"
        );
    }
}

#[test]
fn java_raster_owner_scale_is_invariant_to_authored_image_placement() {
    for hand in [
        JavaHand {
            swing: 0.0,
            equip: 1.0,
            using: None,
        },
        JavaHand {
            swing: 0.3,
            equip: 0.8,
            using: Some(render_model::java_animation::JavaUse::Bow { pull: 7.5 }),
        },
    ] {
        for off_hand in [false, true] {
            for scale in [1.0, 2.0] {
                let reference = raster_points(false, scale, off_hand, hand);
                let translated = raster_points(true, scale, off_hand, hand);
                assert_points(&translated, &reference);
            }
        }
    }
}

#[test]
fn java_raster_owner_scale_uses_the_same_hand_origin_once() {
    let hand = JavaHand {
        swing: 0.3,
        equip: 0.8,
        using: None,
    };
    let origin = render_model::java_animation::first_person_item(
        hand,
        JavaItemMesh::Raster {
            width: 16,
            height: 16,
        },
        false,
    )
    .transform_point3(Vec3::ZERO);
    let unit = raster_points(true, 1.0, false, hand);
    for scale in [0.5, 2.0] {
        let expected: Vec<_> = unit
            .iter()
            .map(|point| origin + (*point - origin) * scale)
            .collect();
        assert_points(&raster_points(true, scale, false, hand), &expected);
    }
    assert_points(&raster_points(true, 2.0, true, hand), &unit);
}

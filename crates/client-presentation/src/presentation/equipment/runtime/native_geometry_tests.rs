//! Full-texture planes must receive the same extrusion as ordinary held sprites.

use super::*;
use bevy::math::{Quat, Vec3};

/// Builds an original full-texture plane and colored weapon stages at the requested resolution.
fn plane_fixture(
    identifier: &str,
    geometry: &str,
    side: u32,
    stages: bool,
) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = fixture(identifier, geometry, 80, stages);
    for (path, bytes) in &mut files {
        if path.ends_with(".png") {
            let color = *image::load_from_memory(bytes)
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0);
            let mut image = image::RgbaImage::from_pixel(side, side, color);
            image.put_pixel(side / 2, side / 2, image::Rgba([0; 4]));
            let mut output = std::io::Cursor::new(Vec::new());
            image
                .write_to(&mut output, image::ImageFormat::Png)
                .unwrap();
            *bytes = output.into_inner();
        } else if path.as_ref() == "models/weapon.json" {
            *bytes = serde_json::to_vec(&serde_json::json!({
                "format_version": "1.16.0", "minecraft:geometry": [{
                    "description": {"identifier": geometry, "texture_width": 8, "texture_height": 8},
                    "bones": [{"name": "rightitem", "cubes": [{
                        "origin": [-4, -2, 0], "size": [8, 8, 0], "uv": [0, 0]
                    }]}]
                }]
            })).unwrap();
        }
    }
    files
}

/// Measures mesh-space extrusion and requires nondegenerate, outward-facing contour walls.
fn extrusion(mesh: &ActorRigGeometry) -> f32 {
    let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut side_area = 0.0;
    for vertex in mesh.vertices.iter() {
        min = min.min(vertex.position[2]);
        max = max.max(vertex.position[2]);
    }
    for triangle in mesh.vertices.as_chunks::<3>().0 {
        if triangle[0].normal[2] != 0.0 {
            continue;
        }
        let [a, b, c] = triangle.map(|vertex| Vec3::from_array(vertex.position));
        let cross = (b - a).cross(c - a);
        assert!(cross.dot(Vec3::from_array(triangle[0].normal)) > 0.0);
        side_area += cross.length() * 0.5;
    }
    assert!(side_area > 0.0);
    assert!(max > min);
    max - min
}

/// Verifies that a rendered pose preserves mesh depth under its uniform hand scale.
fn assert_posed_depth(
    mesh: &ActorRigGeometry,
    bone: render_model::RenderBoneTransform,
    depth: f32,
) {
    assert_eq!(bone.axis_scale, render_model::UNIT_AXIS_SCALE);
    let rotation = Quat::from_array(bone.rotation);
    let normal = rotation * Vec3::Z;
    let scale = bone.translation_scale[3];
    let origin = Vec3::from_array(bone.translation_scale[..3].try_into().unwrap());
    let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
    for vertex in mesh.vertices.iter() {
        let point = origin + rotation * (Vec3::from_array(vertex.position) * scale);
        let along = point.dot(normal);
        min = min.min(along);
        max = max.max(along);
    }
    assert!(scale.abs() > 0.0);
    assert!((max - min - depth * scale.abs()).abs() < 1e-5);
}

#[test]
fn full_texture_weapon_planes_use_sword_extrusion_for_every_stage_and_view() {
    for (identifier, stock, last_frame) in [
        ("minecraft:bow", "geometry.bow_standby", 3u32),
        ("minecraft:crossbow", "geometry.crossbow_standby", 5),
        ("minecraft:iron_spear", "geometry.spear", 1),
    ] {
        for side in [8, 16, 64] {
            let (mut runtime, pages) = startup_runtime(fixture(identifier, stock, 10, true));
            let pages = install(
                &mut runtime,
                pages,
                plane_fixture(identifier, "geometry.item_sprite", side, true),
            );
            assert!(runtime.uses_native_item_mesh(identifier));
            let body = player_body(&mut runtime);
            let weapon = item(identifier);
            let (_, bones) = runtime.body_bones_for(body.input.rig).unwrap();
            for frame in 0..=last_frame {
                let input = AttachableAnimationInput {
                    first_person: true,
                    animation_frame: frame,
                    use_elapsed_ticks: match frame {
                        0 | 4.. => None,
                        1 => Some(0),
                        2 => Some(9),
                        _ => Some(15),
                    },
                    max_use_ticks: 100,
                    ..Default::default()
                };
                let draw = runtime
                    .first_person_item_with_use(
                        &body,
                        &weapon,
                        FirstPersonHand {
                            swing: 0.0,
                            equip: 1.0,
                            consume: None,
                            aspect: 1.0,
                        },
                        render_api::DEFAULT_COMBAT_VISUALS,
                        input,
                    )
                    .unwrap();
                assert!(draw.camera_space);
                assert_eq!(
                    selected_pixel(&pages, &draw.presentation)[0],
                    80 + frame as u8
                );
                let mesh = runtime
                    .take_pending_geometries()
                    .into_iter()
                    .find(|mesh| mesh.id == draw.presentation.submission.input.rig)
                    .unwrap();
                // An ordinary sword with the same silhouette uses the atlas sprite route.
                let texture = runtime
                    .pack
                    .as_ref()
                    .unwrap()
                    .catalog
                    .as_ref()
                    .unwrap()
                    .textures()
                    .iter()
                    .find(|texture| texture.rgba8[0] == 80 + frame as u8)
                    .unwrap()
                    .clone();
                let items = crate::session_assets::SessionItems {
                    components: Arc::default(),
                    icons: Some(Arc::new(
                        client_ui::ui_runtime::presentation::SessionIcons {
                            icons: vec![client_ui::ui_runtime::presentation::SessionIcon {
                                identifier: "minecraft:diamond_sword".into(),
                                metadata: 0,
                                width: u32::from(texture.width),
                                height: u32::from(texture.height),
                                rgba8: texture.rgba8.to_vec().into(),
                            }],
                            ..Default::default()
                        },
                    )),
                };
                let staged = StagedSessionIcons::stage(Some(&items)).unwrap();
                let (_, locations) = pages.clone().with_equipment_rasters(staged.rasters());
                runtime.set_session_items(Some(&items), Some(staged), locations);
                let (sword, _, _, _) = runtime
                    .held_mesh(&item("minecraft:diamond_sword"), true)
                    .unwrap();
                let sword = runtime
                    .take_pending_geometries()
                    .into_iter()
                    .find(|mesh| mesh.id == sword)
                    .unwrap();
                let depth = extrusion(&sword);
                assert_eq!(extrusion(&mesh), depth);
                assert_eq!(mesh.vertices.len(), sword.vertices.len());
                for (actual, expected) in mesh.vertices.iter().zip(sword.vertices.iter()) {
                    assert_eq!(actual.position, expected.position);
                    assert_eq!(actual.normal, expected.normal);
                }
                assert_posed_depth(
                    &mesh,
                    draw.presentation.submission.input.current_bones[0],
                    depth,
                );
                let mut layers = Vec::new();
                runtime.push_native_held(
                    &body,
                    &weapon,
                    1,
                    &bones,
                    None,
                    AttachableAnimationInput {
                        first_person: false,
                        ..input
                    },
                    &mut layers,
                );
                assert_eq!(layers.len(), 1);
                assert_eq!(layers[0].submission.input.rig, mesh.id);
                assert_eq!(selected_pixel(&pages, &layers[0])[0], 80 + frame as u8);
                assert_posed_depth(&mesh, layers[0].submission.input.current_bones[0], depth);
            }
        }
    }
}

#[test]
fn full_texture_plane_routing_rejects_3d_or_partial_atlas_models() {
    for change in [
        serde_json::json!({"size": [8, 8, 1]}),
        serde_json::json!({"size": [4, 8, 0]}),
        serde_json::json!({"uv": [1, 0]}),
        serde_json::json!({"inflate": 0.1}),
        serde_json::json!({"mirror": true}),
    ] {
        let (mut runtime, pages) =
            startup_runtime(fixture("minecraft:bow", "geometry.bow_standby", 10, true));
        let mut files = plane_fixture("minecraft:bow", "geometry.item_sprite", 16, true);
        let (_, bytes) = files
            .iter_mut()
            .find(|(path, _)| path.as_ref() == "models/weapon.json")
            .unwrap();
        let mut model: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        let cube = model["minecraft:geometry"][0]["bones"][0]["cubes"][0]
            .as_object_mut()
            .unwrap();
        cube.extend(change.as_object().unwrap().clone());
        *bytes = serde_json::to_vec(&model).unwrap();
        install(&mut runtime, pages, files);
        assert!(!runtime.uses_native_item_mesh("minecraft:bow"));
    }
}

#[test]
fn startup_full_texture_plane_supplies_missing_pull_stages() {
    let (mut runtime, pages) = startup_runtime(plane_fixture(
        "minecraft:bow",
        "geometry.bow_standby",
        32,
        true,
    ));
    let pages = install(
        &mut runtime,
        pages,
        plane_fixture("minecraft:bow", "geometry.item_sprite", 64, false),
    );
    let body = player_body(&mut runtime);
    let draw = runtime
        .first_person_item_with_use(
            &body,
            &item("minecraft:bow"),
            FirstPersonHand {
                swing: 0.0,
                equip: 1.0,
                consume: None,
                aspect: 1.0,
            },
            render_api::DEFAULT_COMBAT_VISUALS,
            AttachableAnimationInput {
                first_person: true,
                use_elapsed_ticks: Some(9),
                max_use_ticks: 100,
                ..Default::default()
            },
        )
        .unwrap();
    let mesh = runtime
        .take_pending_geometries()
        .into_iter()
        .find(|mesh| mesh.id == draw.presentation.submission.input.rig)
        .unwrap();
    assert_eq!(selected_pixel(&pages, &draw.presentation)[0], 82);
    assert!(extrusion(&mesh) > 0.0);
}

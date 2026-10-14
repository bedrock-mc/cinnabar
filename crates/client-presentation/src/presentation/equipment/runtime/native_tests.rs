//! Original pack fixtures for native weapon routing, artwork, and placement.

use crate::presentation::equipment::tests::held_animation_tests::{owner, owner_rig};
use crate::presentation::equipment::tests::{pack_catalog, player_body, startup_runtime};
use crate::presentation::equipment::{FirstPersonHand, runtime::*};
use actor_animation::AttachableAnimationInput;
use render_model::equipment::{attach_to_bone, held_sprite_display};

/// Reads the selected equipment image's first pixel from its retained artwork page.
fn selected_pixel(
    pages: &render_model::ActorArtworkPages,
    layer: &EquipmentPresentation,
) -> [u8; 4] {
    let page = &pages.pages()[usize::from(layer.location.page() - 1)];
    let (width, height) = page.dimensions();
    let start = layer.location.layer() as usize * usize::from(width) * usize::from(height) * 4;
    page.pixels()[start..start + 4].try_into().unwrap()
}

/// Builds original sprite geometry, deliberately displaced animation, and colored stage images.
fn fixture(identifier: &str, geometry: &str, color: u8, stages: bool) -> Vec<(Box<str>, Vec<u8>)> {
    let mut textures = serde_json::Map::new();
    let aliases: &[&str] = match identifier {
        "minecraft:bow" => &["default", "bow_pulling_0", "bow_pulling_1", "bow_pulling_2"],
        "minecraft:crossbow" => &[
            "default",
            "crossbow_pulling_0",
            "crossbow_pulling_1",
            "crossbow_pulling_2",
            "crossbow_arrow",
            "crossbow_rocket",
        ],
        _ => &["default", "raised"],
    };
    let mut files = Vec::new();
    for (index, alias) in aliases
        .iter()
        .enumerate()
        .filter(|(index, _)| stages || *index == 0)
    {
        let path = format!("textures/fixture/{color}_{alias}");
        textures.insert((*alias).into(), serde_json::json!(path));
        let mut image =
            image::RgbaImage::from_pixel(32, 32, image::Rgba([color + index as u8, 40, 60, 255]));
        image.put_pixel(31, 31, image::Rgba([0; 4]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        files.push((format!("{path}.png").into(), bytes.into_inner()));
    }
    for (path, json) in [
        (
            "attachables/weapon.json",
            serde_json::json!({
                "format_version": "1.10.0", "minecraft:attachable": {"description": {
                    "identifier": identifier, "materials": {"default": "entity_alphatest"},
                    "textures": textures, "geometry": {"default": geometry},
                    "animations": {"wield": "animation.fixture.wield"}, "scripts": {"animate": ["wield"]},
                    "render_controllers": ["controller.render.fixture"]
                }}
            }),
        ),
        (
            "models/weapon.json",
            serde_json::json!({
                "format_version": "1.16.0", "minecraft:geometry": [{
                    "description": {"identifier": geometry, "texture_width": 16, "texture_height": 16},
                    "bones": [{"name": "rightitem", "texture_meshes": [{"texture": "default"}]}]
                }]
            }),
        ),
        (
            "animations/weapon.json",
            serde_json::json!({
                "format_version": "1.10.0", "animations": {"animation.fixture.wield": {
                    "loop": true, "bones": {"rightitem": {"position": [50, 60, 70]}}
                }}
            }),
        ),
        (
            "render_controllers/weapon.json",
            serde_json::json!({
                "format_version": "1.8.0", "render_controllers": {"controller.render.fixture": {
                    "geometry": "Geometry.default", "textures": ["Texture.default"],
                    "materials": [{"*": "Material.default"}]
                }}
            }),
        ),
    ] {
        files.push((path.into(), serde_json::to_vec(&json).unwrap()));
    }
    files
}

/// Installs an independently compiled overlay and keeps its artwork pages available for assertions.
fn install(
    runtime: &mut EquipmentRuntime,
    pages: render_model::ActorArtworkPages,
    files: Vec<(Box<str>, Vec<u8>)>,
) -> render_model::ActorArtworkPages {
    let (entities, catalog) = pack_catalog(files);
    let (pages, locations) =
        pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
    pages
}

/// Supplies a held stack with admitted use timing and ordinary native grip facts.
fn item(identifier: &str) -> WornItem {
    WornItem {
        identifier: identifier.into(),
        metadata: 0,
        damage: None,
        charged_projectile: None,
        use_facts: client_world::resolve_item_use(
            &assets::item_use::diagnostic_profile(identifier),
            None,
            None,
        ),
        grip: assets::item::grip::builtin_item_grip(identifier),
        kind: HeldKind::Sprite,
        dye_rgb: None,
        enchanted: true,
    }
}

#[test]
fn texture_only_bow_uses_native_mesh_and_pack_stages_over_the_use_duration() {
    let identifier = "minecraft:bow";
    let (mut runtime, pages) =
        startup_runtime(fixture(identifier, "geometry.bow_standby", 10, true));
    let pages = install(
        &mut runtime,
        pages,
        fixture(identifier, "geometry.bow_standby", 80, true),
    );
    let body = player_body(&mut runtime);
    let item = item(identifier);
    assert!(runtime.uses_native_item_mesh(identifier));
    assert!(!runtime.is_vanilla_attachable(identifier));
    let owner = owner();
    let names = [Box::from("rightItem")];
    let rig = owner_rig(&owner, &names);
    assert!(
        runtime
            .first_person_attachable(
                &body,
                &item,
                &owner,
                &rig,
                AttachableAnimationInput::default(),
                None
            )
            .is_none()
    );
    let hand = FirstPersonHand {
        swing: 0.3,
        equip: 0.8,
        consume: None,
    };
    for (elapsed, color) in [
        (None, 80),
        (Some(0), 81),
        (Some(8), 81),
        (Some(9), 82),
        (Some(14), 82),
        (Some(15), 83),
    ] {
        let draw = runtime
            .first_person_item_with_use(
                &body,
                &item,
                hand,
                render_api::DEFAULT_COMBAT_VISUALS,
                AttachableAnimationInput {
                    first_person: true,
                    use_elapsed_ticks: elapsed,
                    max_use_ticks: 100,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(draw.camera_space);
        assert_eq!(selected_pixel(&pages, &draw.presentation)[0], color);
        assert_eq!(
            draw.presentation.submission.material.glint.texture,
            render_model::GlintTexture::Item
        );
        let expected = crate::presentation::equipment::display::view_bone(
            crate::presentation::equipment::display::first_person_display_for_profile(
                crate::presentation::equipment::display::FirstPersonShape::Sprite {
                    mirrored_art: false,
                },
                hand,
                render_api::DEFAULT_COMBAT_VISUALS,
            ),
        )
        .unwrap();
        assert_eq!(
            draw.presentation.submission.input.current_bones.as_ref(),
            &[expected]
        );
        let mesh = runtime
            .take_pending_geometries()
            .into_iter()
            .find(|mesh| mesh.id == draw.presentation.submission.input.rig);
        if let Some(mesh) = mesh {
            let depth = mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position[2])
                .fold(0.0, f32::min);
            assert_eq!(depth.abs(), 1.0 / 16.0);
            assert!(mesh.vertices.iter().all(|vertex| vertex.bone_index == 0));
        }
    }
}

#[test]
fn missing_bow_stage_uses_the_matching_startup_stage() {
    let (mut runtime, pages) =
        startup_runtime(fixture("minecraft:bow", "geometry.bow_standby", 10, true));
    let pages = install(
        &mut runtime,
        pages,
        fixture("minecraft:bow", "geometry.bow_standby", 80, false),
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
            },
            render_api::DEFAULT_COMBAT_VISUALS,
            AttachableAnimationInput {
                first_person: true,
                use_elapsed_ticks: Some(15),
                max_use_ticks: 100,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(selected_pixel(&pages, &draw.presentation)[0], 13);
}

#[test]
fn custom_models_including_overrides_of_stock_names_keep_attachable_routing() {
    for rename in [false, true] {
        let (mut runtime, pages) =
            startup_runtime(fixture("minecraft:bow", "geometry.bow_standby", 10, true));
        let geometry = if rename {
            "geometry.custom_weapon"
        } else {
            "geometry.bow_standby"
        };
        let mut files = fixture("minecraft:bow", geometry, 80, true);
        let (_, bytes) = files
            .iter_mut()
            .find(|(path, _)| path.as_ref() == "models/weapon.json")
            .unwrap();
        let mut model: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        model["minecraft:geometry"][0]["bones"][0]["cubes"] =
            serde_json::json!([{"origin":[0,0,0],"size":[2,3,4],"uv":[0,0]}]);
        *bytes = serde_json::to_vec(&model).unwrap();
        install(&mut runtime, pages, files);
        assert!(!runtime.uses_native_item_mesh("minecraft:bow"));
        assert!(runtime.is_vanilla_attachable("minecraft:bow"));
        let body = player_body(&mut runtime);
        let owner = owner();
        let names = [Box::from("rightItem")];
        let rig = owner_rig(&owner, &names);
        let draw = runtime
            .first_person_attachable(
                &body,
                &item("minecraft:bow"),
                &owner,
                &rig,
                AttachableAnimationInput {
                    first_person: true,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        assert!(!draw.camera_space);
    }
}

#[test]
fn crossbow_charge_and_loaded_art_follow_native_frames() {
    let (mut runtime, pages) = startup_runtime(fixture(
        "minecraft:crossbow",
        "geometry.crossbow_standby",
        10,
        true,
    ));
    let pages = install(
        &mut runtime,
        pages,
        fixture("minecraft:crossbow", "geometry.crossbow_standby", 80, true),
    );
    let body = player_body(&mut runtime);
    let mut item = item("minecraft:crossbow");
    for (elapsed, frame) in [(0, 0), (8, 1), (14, 2), (18, 3), (25, 4)] {
        let input = AttachableAnimationInput {
            use_elapsed_ticks: Some(elapsed),
            max_use_ticks: 25,
            ..Default::default()
        };
        let (_, location, _, _) = runtime.held_mesh_with_use(&item, input).unwrap();
        let draw = runtime
            .first_person_java_item_with_use(
                &body,
                &item,
                render_model::java_animation::JavaHand {
                    swing: 0.2,
                    equip: 1.0,
                    using: None,
                },
                input,
            )
            .unwrap();
        assert_eq!(draw.presentation.location, location);
        assert_eq!(selected_pixel(&pages, &draw.presentation)[0], 80 + frame);
        assert!(draw.java_camera.is_some());
    }
    for (projectile, color) in [
        (assets::item_use::ItemProjectileRole::Arrow, 84),
        (assets::item_use::ItemProjectileRole::Firework, 85),
    ] {
        item.charged_projectile = Some(projectile);
        let draw = runtime
            .first_person_java_item(
                &body,
                &item,
                render_model::java_animation::JavaHand {
                    swing: 0.0,
                    equip: 1.0,
                    using: None,
                },
            )
            .unwrap();
        assert_eq!(selected_pixel(&pages, &draw.presentation)[0], color);
        let off = runtime.first_person_offhand(&body, &item).unwrap();
        assert_eq!(selected_pixel(&pages, &off.presentation)[0], color);
    }
    item.charged_projectile = None;
    let predicted = runtime
        .first_person_java_item_with_use(
            &body,
            &item,
            render_model::java_animation::JavaHand {
                swing: 0.0,
                equip: 1.0,
                using: None,
            },
            AttachableAnimationInput {
                first_person: true,
                hand_charged: true,
                animation_frame: 5,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(selected_pixel(&pages, &predicted.presentation)[0], 85);
}

#[test]
fn spears_and_tridents_keep_native_hands_in_both_views_and_reload_cleanly() {
    for (identifier, geometry) in [
        ("minecraft:trident", "geometry.trident"),
        ("minecraft:iron_spear", "geometry.spear"),
    ] {
        let (mut runtime, pages) = startup_runtime(fixture(identifier, geometry, 10, true));
        let pages = install(&mut runtime, pages, fixture(identifier, geometry, 80, true));
        let body = player_body(&mut runtime);
        let item = item(identifier);
        let draw = runtime
            .first_person_item_with_use(
                &body,
                &item,
                FirstPersonHand {
                    swing: 0.0,
                    equip: 1.0,
                    consume: None,
                },
                render_api::DEFAULT_COMBAT_VISUALS,
                AttachableAnimationInput {
                    use_elapsed_ticks: Some(0),
                    max_use_ticks: 100,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(selected_pixel(&pages, &draw.presentation)[0], 81);
        let off = runtime.first_person_offhand(&body, &item).unwrap();
        assert_eq!(selected_pixel(&pages, &off.presentation)[0], 80);
        let (_, bones) = runtime.body_bones_for(body.input.rig).unwrap();
        for java in [None, Some(JavaGrip::default())] {
            for off_hand in [false, true] {
                let mut layers = Vec::new();
                runtime.push_native_held(
                    &body,
                    &item,
                    if off_hand { 2 } else { 1 },
                    &bones,
                    java,
                    AttachableAnimationInput {
                        off_hand,
                        ..Default::default()
                    },
                    &mut layers,
                );
                assert_eq!(layers.len(), 1);
                let hand = if java.is_some() && !off_hand {
                    bones.right_arm
                } else if off_hand {
                    bones.left_item
                } else {
                    bones.right_item
                }
                .unwrap();
                let display = if java.is_some() && !off_hand {
                    render_model::equipment::ItemDisplay::from_matrix(
                        render_model::java_animation::third_person_item(
                            render_model::java_animation::JavaHeldItem::Flat,
                            render_model::java_animation::JavaItemMesh::Sprite,
                        ),
                    )
                } else {
                    held_sprite_display(item.grip.upright().unwrap_or(false))
                };
                let expected = attach_to_bone(body.input.current_bones[hand], display).unwrap();
                assert_eq!(
                    layers[0].submission.input.current_bones.as_ref(),
                    &[expected]
                );
            }
        }
        runtime.set_pack_layer(None);
        assert!(!runtime.uses_native_item_mesh(identifier));
        assert!(runtime.native_meshes.is_empty());
    }
}

#[test]
fn inherited_binding_and_item_sprite_geometry_use_native_meshes() {
    let identifier = "minecraft:bow";
    let (mut runtime, pages) =
        startup_runtime(fixture(identifier, "geometry.bow_standby", 10, true));
    let pages = install(
        &mut runtime,
        pages,
        fixture(identifier, "geometry.item_sprite", 80, true),
    );
    assert!(runtime.uses_native_item_mesh(identifier));
    let entities = std::sync::Arc::clone(&runtime.assets);
    let mut texture = runtime.catalog.as_ref().unwrap().textures()[0].clone();
    texture.rgba8 = [90, 40, 60, 255]
        .repeat(usize::from(texture.width) * usize::from(texture.height))
        .into();
    let catalog = std::sync::Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts([2; 32], Vec::new(), vec![texture]).unwrap(),
    );
    let (_, locations) = pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
    assert!(runtime.uses_native_item_mesh(identifier));
}

#[test]
fn an_extra_custom_stage_model_prevents_native_routing() {
    let (mut runtime, pages) =
        startup_runtime(fixture("minecraft:bow", "geometry.bow_standby", 10, true));
    let mut files = fixture("minecraft:bow", "geometry.bow_standby", 80, true);
    let (_, description) = files
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "attachables/weapon.json")
        .unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(description).unwrap();
    document["minecraft:attachable"]["description"]["geometry"]["extra"] =
        serde_json::json!("geometry.custom_stage");
    *description = serde_json::to_vec(&document).unwrap();
    files.push(("models/custom.json".into(), serde_json::to_vec(&serde_json::json!({
        "format_version": "1.16.0", "minecraft:geometry": [{
            "description": {"identifier": "geometry.custom_stage", "texture_width": 16, "texture_height": 16},
            "bones": [{"name": "custom", "cubes": [{"origin":[0,0,0],"size":[2,3,4],"uv":[0,0]}]}]
        }]
    })).unwrap()));
    install(&mut runtime, pages, files);
    assert!(!runtime.uses_native_item_mesh("minecraft:bow"));
}

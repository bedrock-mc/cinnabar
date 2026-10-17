//! Original pack fixtures for native weapon routing, artwork, and placement.

#[path = "held_scale_native_tests.rs"]
mod held_scale;

use super::NativeItem;
use crate::presentation::equipment::tests::held_animation_tests::{owner, owner_rig};
use crate::presentation::equipment::tests::{pack_catalog, player_body, startup_runtime};
use crate::presentation::equipment::{FirstPersonHand, runtime::*};
use actor_animation::AttachableAnimationInput;
use render_model::equipment::{attach_to_bone, held_sprite_display};

#[path = "native_geometry_tests.rs"]
mod geometry;

#[path = "native_variant_tests.rs"]
mod variants;

#[path = "routing_tests.rs"]
mod routing;

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

/// Samples the center of a generated mesh's artwork region, including atlas sprites.
fn mesh_pixel(
    pages: &render_model::ActorArtworkPages,
    location: render_model::ActorArtworkLocation,
    mesh: &render_model::ActorRigGeometry,
) -> [u8; 4] {
    let page = &pages.pages()[usize::from(location.page() - 1)];
    let (width, height) = page.dimensions();
    let bounds = mesh.vertices.iter().fold(
        ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]),
        |(mut min, mut max), vertex| {
            for axis in 0..2 {
                min[axis] = min[axis].min(vertex.uv[axis]);
                max[axis] = max[axis].max(vertex.uv[axis]);
            }
            (min, max)
        },
    );
    let x = ((bounds.0[0] + bounds.1[0]) * 0.5 * f32::from(width)) as usize;
    let y = ((bounds.0[1] + bounds.1[1]) * 0.5 * f32::from(height)) as usize;
    let start = (location.layer() as usize * usize::from(width) * usize::from(height)
        + y * usize::from(width)
        + x)
        * 4;
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
        variant: Default::default(),
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
        glint: Default::default(),
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
        aspect: 1.0,
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
                aspect: 1.0,
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
fn missing_weapon_stage_textures_use_animation_icons_and_loaded_offhand_dimensions() {
    for (identifier, geometry, animation, last_frame) in [
        (
            "minecraft:bow",
            "geometry.bow_standby",
            "minecraft:bow_pulling",
            3_u32,
        ),
        (
            "minecraft:crossbow",
            "geometry.crossbow_standby",
            "minecraft:crossbow_pulling",
            5_u32,
        ),
    ] {
        let (entities, catalog) = pack_catalog(fixture(identifier, geometry, 10, false));
        let sprites = (0..=last_frame)
            .map(|frame| {
                let width = 16 + frame as u16;
                let height = 24 + 2 * frame as u16;
                assets::IconSprite {
                    width,
                    height,
                    rgba8: [60 + frame as u8, 40, 60, 255]
                        .repeat(usize::from(width) * usize::from(height))
                        .into(),
                }
            })
            .collect::<Vec<_>>();
        let entries = (0..=last_frame)
            .map(|frame| assets::IconEntry {
                identifier: if frame == 0 { identifier } else { animation }.into(),
                metadata: frame.saturating_sub(1),
                sprite: frame,
            })
            .collect::<Vec<_>>();
        let icons = Arc::new(
            assets::RuntimeIconCatalog::decode(
                &assets::encode_icon_catalog([0; 32], &sprites, &entries).unwrap(),
            )
            .unwrap(),
        );
        let (mut runtime, pages, _) = EquipmentRuntime::build(
            entities,
            Some(catalog),
            icons,
            None,
            None,
            render_model::ActorArtworkPages::default(),
        );
        let pages = install(
            &mut runtime,
            pages,
            fixture(identifier, geometry, 80, false),
        );
        assert!(runtime.uses_native_item_mesh(identifier));
        let mut item = item(identifier);
        for frame in 1..=last_frame {
            let (rig, location, _, span) = runtime
                .held_mesh_with_use(
                    &item,
                    AttachableAnimationInput {
                        first_person: true,
                        animation_frame: frame,
                        use_elapsed_ticks: match frame {
                            1 => Some(0),
                            2 => Some(9),
                            3 => Some(15),
                            _ => None,
                        },
                        max_use_ticks: 100,
                        ..Default::default()
                    },
                )
                .unwrap();
            let mesh = runtime
                .take_pending_geometries()
                .into_iter()
                .find(|mesh| mesh.id == rig)
                .unwrap();
            assert_eq!(mesh_pixel(&pages, location, &mesh)[0], 60 + frame as u8);
            let page = &pages.pages()[usize::from(location.page() - 1)];
            let (width, height) = page.dimensions();
            assert_eq!(
                span,
                [
                    f32::from(sprites[frame as usize].width) / f32::from(width),
                    f32::from(sprites[frame as usize].height) / f32::from(height),
                ]
            );
        }
        if identifier == "minecraft:crossbow" {
            for (projectile, frame) in [
                (assets::item_use::ItemProjectileRole::Arrow, 4),
                (assets::item_use::ItemProjectileRole::Firework, 5),
            ] {
                item.charged_projectile = Some(projectile);
                assert_eq!(
                    runtime.native_sprite_dimensions(&item),
                    Some([sprites[frame].width, sprites[frame].height])
                );
                let (rig, location, _, _) = runtime
                    .held_mesh_with_use(
                        &item,
                        AttachableAnimationInput {
                            off_hand: true,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let main = runtime
                    .held_mesh_with_use(
                        &item,
                        AttachableAnimationInput {
                            first_person: true,
                            animation_frame: frame as u32,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                assert_eq!((rig, location), (main.0, main.1));
            }
        }
    }
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
                    aspect: 1.0,
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

/// Replaces the fixture sprite geometry with an original cube model using a UV atlas.
fn cube_fixture(identifier: &str, geometry: &str, color: u8) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = fixture(identifier, geometry, color, false);
    let (_, bytes) = files
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "models/weapon.json")
        .unwrap();
    let mut model: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    let bone = &mut model["minecraft:geometry"][0]["bones"][0];
    bone.as_object_mut().unwrap().remove("texture_meshes");
    bone["cubes"] = serde_json::json!([{"origin": [0, 0, 0], "size": [2, 3, 4], "uv": [0, 0]}]);
    *bytes = serde_json::to_vec(&model).unwrap();
    files
}

/// Builds a cube-backed startup attachable with an optional original item sprite.
fn cube_runtime(
    identifier: &str,
    geometry: &str,
    icon: Option<u8>,
) -> (EquipmentRuntime, render_model::ActorArtworkPages) {
    let (entities, catalog) = pack_catalog(cube_fixture(identifier, geometry, 10));
    let sprites = icon
        .into_iter()
        .map(|color| assets::IconSprite {
            width: 16,
            height: 32,
            rgba8: [color, 40, 60, 255].repeat(16 * 32).into(),
        })
        .collect::<Vec<_>>();
    let entries = icon
        .into_iter()
        .map(|_| assets::IconEntry {
            identifier: identifier.into(),
            metadata: 0,
            sprite: 0,
        })
        .collect::<Vec<_>>();
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog([0; 32], &sprites, &entries).unwrap(),
        )
        .unwrap(),
    );
    let (runtime, pages, _) = EquipmentRuntime::build(
        entities,
        Some(catalog),
        icons,
        None,
        None,
        render_model::ActorArtworkPages::default(),
    );
    (runtime, pages)
}

#[test]
fn cube_backed_weapons_use_item_sprites_in_both_views_and_hands() {
    for (identifier, geometry) in [
        ("minecraft:trident", "geometry.trident"),
        ("minecraft:iron_spear", "geometry.spear"),
    ] {
        for (startup_icon, pack_icon) in [(Some(30), None), (Some(30), Some(90)), (None, Some(90))]
        {
            let (mut runtime, pages) = cube_runtime(identifier, geometry, startup_icon);
            let mut pages = install(&mut runtime, pages, cube_fixture(identifier, geometry, 80));
            if let Some(color) = pack_icon {
                let items = crate::session_assets::SessionItems {
                    components: Arc::default(),
                    icons: Some(Arc::new(
                        client_ui::ui_runtime::presentation::SessionIcons {
                            icons: vec![client_ui::ui_runtime::presentation::SessionIcon {
                                identifier: identifier.into(),
                                metadata: 0,
                                width: 8,
                                height: 24,
                                rgba8: [color, 40, 60, 255].repeat(8 * 24).into(),
                            }],
                            ..Default::default()
                        },
                    )),
                };
                let staged = StagedSessionIcons::stage(Some(&items)).unwrap();
                let (next, locations) = pages.with_equipment_rasters(staged.rasters());
                pages = next;
                runtime.set_session_items(Some(&items), Some(staged), locations);
            }
            assert!(runtime.uses_native_item_mesh(identifier));
            let item = item(identifier);
            let body = player_body(&mut runtime);
            assert_eq!(
                runtime.native_sprite_dimensions(&item),
                Some(if pack_icon.is_some() {
                    [8, 24]
                } else {
                    [16, 32]
                })
            );
            for input in [
                AttachableAnimationInput::default(),
                AttachableAnimationInput {
                    use_elapsed_ticks: Some(0),
                    ..Default::default()
                },
            ] {
                let hand = FirstPersonHand {
                    swing: 0.0,
                    equip: 1.0,
                    consume: None,
                    aspect: 1.0,
                };
                let native = runtime
                    .first_person_item_with_use(
                        &body,
                        &item,
                        hand,
                        render_api::DEFAULT_COMBAT_VISUALS,
                        input,
                    )
                    .unwrap();
                let java = runtime
                    .first_person_java_item_with_use(
                        &body,
                        &item,
                        render_model::java_animation::JavaHand {
                            swing: 0.0,
                            equip: 1.0,
                            using: None,
                        },
                        input,
                    )
                    .unwrap();
                let off = runtime.first_person_offhand(&body, &item).unwrap();
                for draw in [native, java, off] {
                    assert_eq!(
                        selected_pixel(&pages, &draw.presentation)[0],
                        pack_icon.or(startup_icon).unwrap()
                    );
                }
            }
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
                    assert_eq!(
                        selected_pixel(&pages, &layers[0])[0],
                        pack_icon.or(startup_icon).unwrap()
                    );
                }
            }
        }
    }
}

#[test]
fn cube_backed_weapons_keep_model_rendering_when_no_item_sprite_exists() {
    for (identifier, geometry) in [
        ("minecraft:trident", "geometry.trident"),
        ("minecraft:iron_spear", "geometry.spear"),
    ] {
        let (mut runtime, pages) = cube_runtime(identifier, geometry, None);
        let pages = install(&mut runtime, pages, cube_fixture(identifier, geometry, 80));
        assert!(!runtime.uses_native_item_mesh(identifier));
        let body = player_body(&mut runtime);
        let owner = owner();
        let names = [Box::from("rightItem")];
        let rig = owner_rig(&owner, &names);
        for first_person in [false, true] {
            let draw = runtime
                .first_person_attachable(
                    &body,
                    &item(identifier),
                    &owner,
                    &rig,
                    AttachableAnimationInput {
                        first_person,
                        ..Default::default()
                    },
                    None,
                )
                .unwrap();
            assert!(!draw.camera_space);
            assert_eq!(selected_pixel(&pages, &draw.presentation)[0], 80);
            let mesh = runtime
                .take_pending_geometries()
                .into_iter()
                .find(|mesh| mesh.id == draw.presentation.submission.input.rig);
            if let Some(mesh) = mesh {
                let (min, max) = mesh
                    .vertices
                    .iter()
                    .map(|vertex| vertex.position[2])
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), z| {
                        (min.min(z), max.max(z))
                    });
                assert!((max - min) > 1.0 / 16.0);
            }
        }
    }
}

#[test]
fn inherited_cube_geometry_uses_the_item_icon_for_a_texture_only_overlay() {
    let identifier = "minecraft:trident";
    let (mut runtime, pages) = cube_runtime(identifier, "geometry.trident", Some(30));
    let mut texture = runtime.catalog.as_ref().unwrap().textures()[0].clone();
    texture.rgba8 = [80, 40, 60, 255].repeat(32 * 32).into();
    let catalog = Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts([2; 32], vec![], vec![texture]).unwrap(),
    );
    let (pages, locations) =
        pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    runtime.set_pack_layer(Some((
        Arc::clone(&runtime.assets),
        Some(catalog),
        locations,
    )));
    assert!(runtime.uses_native_item_mesh(identifier));
    let body = player_body(&mut runtime);
    let draw = runtime
        .first_person_item(
            &body,
            &item(identifier),
            FirstPersonHand {
                swing: 0.0,
                equip: 1.0,
                consume: None,
                aspect: 1.0,
            },
        )
        .unwrap();
    assert_eq!(selected_pixel(&pages, &draw.presentation)[0], 30);
}

#[test]
fn missing_pack_sprite_never_falls_back_to_a_startup_cube_uv_atlas() {
    let identifier = "minecraft:trident";
    let mut files = cube_fixture(identifier, "geometry.trident", 10);
    let sprite_model = fixture(identifier, "geometry.item_sprite", 10, false)
        .into_iter()
        .find(|(path, _)| path.as_ref() == "models/weapon.json")
        .unwrap()
        .1;
    files.push(("models/item_sprite.json".into(), sprite_model));
    let (entities, catalog) = pack_catalog(files);
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog(
                [0; 32],
                &[assets::IconSprite {
                    width: 16,
                    height: 16,
                    rgba8: [30, 40, 60, 255].repeat(16 * 16).into(),
                }],
                &[assets::IconEntry {
                    identifier: identifier.into(),
                    metadata: 0,
                    sprite: 0,
                }],
            )
            .unwrap(),
        )
        .unwrap(),
    );
    let (mut runtime, pages, _) = EquipmentRuntime::build(
        entities,
        Some(catalog),
        icons,
        None,
        None,
        render_model::ActorArtworkPages::default(),
    );
    let overlay = fixture(identifier, "geometry.item_sprite", 10, false)
        .into_iter()
        .filter(|(path, _)| !path.ends_with(".png"))
        .collect();
    let pages = install(&mut runtime, pages, overlay);
    assert!(runtime.uses_native_item_mesh(identifier));
    let body = player_body(&mut runtime);
    let draw = runtime
        .first_person_item(
            &body,
            &item(identifier),
            FirstPersonHand {
                swing: 0.0,
                equip: 1.0,
                consume: None,
                aspect: 1.0,
            },
        )
        .unwrap();
    assert_eq!(selected_pixel(&pages, &draw.presentation)[0], 30);
}

/// Round-trips a startup carrier in the older format that omits resource aliases.
fn version_five_catalog(
    catalog: &assets::RuntimeEquipmentCatalog,
) -> Arc<assets::RuntimeEquipmentCatalog> {
    let mut bindings = catalog.bindings().to_vec();
    for binding in &mut bindings {
        binding.resources = Default::default();
    }
    let bytes = assets::encode_equipment_catalog_with_textures(
        catalog.source_manifest_sha256(),
        catalog.entity_blob_sha256(),
        &bindings,
        catalog.textures(),
    )
    .unwrap();
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 5);
    Arc::new(assets::RuntimeEquipmentCatalog::decode(&bytes).unwrap())
}

#[test]
fn version_five_carriers_without_aliases_use_pack_weapon_stage_textures() {
    for (identifier, geometry, last_frame) in [
        ("minecraft:bow", "geometry.bow_standby", 3),
        ("minecraft:crossbow", "geometry.crossbow_standby", 5),
    ] {
        let (entities, catalog) = pack_catalog(fixture(identifier, geometry, 10, true));
        let legacy = version_five_catalog(&catalog);
        assert!(
            legacy
                .binding(identifier)
                .unwrap()
                .resources
                .textures
                .is_empty()
        );
        let icons = Arc::new(
            assets::RuntimeIconCatalog::decode(
                &assets::encode_icon_catalog([0; 32], &[], &[]).unwrap(),
            )
            .unwrap(),
        );
        let (mut runtime, pages, _) = EquipmentRuntime::build(
            Arc::clone(&entities),
            Some(Arc::clone(&legacy)),
            icons,
            None,
            None,
            render_model::ActorArtworkPages::default(),
        );
        let (_, overlay) = pack_catalog(fixture(identifier, geometry, 80, true));
        let textures = overlay
            .binding(identifier)
            .unwrap()
            .resources
            .textures
            .iter()
            .map(|(alias, path)| {
                let mut texture = overlay.texture(path).unwrap().clone();
                texture.identifier = if alias.as_ref() == "default" {
                    legacy
                        .binding(identifier)
                        .unwrap()
                        .texture
                        .identifier
                        .clone()
                } else if alias.as_ref() == "crossbow_rocket" {
                    "textures/items/crossbow_firework".into()
                } else {
                    format!("textures/items/{alias}").into_boxed_str()
                };
                texture
            })
            .collect();
        let overlay = Arc::new(
            assets::RuntimeEquipmentCatalog::from_parts([2; 32], vec![], textures).unwrap(),
        );
        let (pages, locations) =
            pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&overlay));
        runtime.set_pack_layer(Some((entities, Some(overlay), locations)));
        assert!(runtime.uses_native_item_mesh(identifier));
        let body = player_body(&mut runtime);
        for frame in 0..=last_frame {
            let draw = runtime
                .first_person_item_with_use(
                    &body,
                    &item(identifier),
                    FirstPersonHand {
                        swing: 0.0,
                        equip: 1.0,
                        consume: None,
                        aspect: 1.0,
                    },
                    render_api::DEFAULT_COMBAT_VISUALS,
                    AttachableAnimationInput {
                        first_person: true,
                        animation_frame: frame,
                        use_elapsed_ticks: match frame {
                            1 => Some(0),
                            2 => Some(9),
                            3 => Some(15),
                            _ => None,
                        },
                        max_use_ticks: 100,
                        ..Default::default()
                    },
                )
                .unwrap();
            assert_eq!(
                selected_pixel(&pages, &draw.presentation)[0],
                80 + frame as u8
            );
        }
    }
}

#[test]
fn version_five_carriers_recognize_single_stage_texture_overrides() {
    for (identifier, geometry, stem, last_frame) in [
        ("minecraft:bow", "geometry.bow_standby", "bow", 3),
        (
            "minecraft:crossbow",
            "geometry.crossbow_standby",
            "crossbow",
            5,
        ),
    ] {
        for frame in 1..=last_frame {
            let (mut runtime, pages) = startup_runtime(fixture(identifier, geometry, 10, false));
            runtime.catalog = Some(version_five_catalog(runtime.catalog.as_ref().unwrap()));
            let path = match (stem, frame) {
                ("crossbow", 4) => "textures/items/crossbow_arrow".into(),
                ("crossbow", 5) => "textures/items/crossbow_firework".into(),
                _ => format!("textures/items/{stem}_pulling_{}", frame - 1).into_boxed_str(),
            };
            let texture = assets::EquipmentTexture {
                identifier: path,
                width: 8,
                height: 16,
                rgba8: [80, 40, 60, 255].repeat(8 * 16).into(),
            };
            let overlay = Arc::new(
                assets::RuntimeEquipmentCatalog::from_parts([2; 32], vec![], vec![texture])
                    .unwrap(),
            );
            let (pages, locations) =
                pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&overlay));
            runtime.set_pack_layer(Some((
                Arc::clone(&runtime.assets),
                Some(overlay),
                locations,
            )));
            assert!(runtime.uses_native_item_mesh(identifier));
            let (rig, location, _, span) = runtime
                .held_mesh_with_use(
                    &item(identifier),
                    AttachableAnimationInput {
                        first_person: true,
                        animation_frame: frame,
                        use_elapsed_ticks: match frame {
                            1 => Some(0),
                            2 => Some(9),
                            3 => Some(15),
                            _ => None,
                        },
                        max_use_ticks: 100,
                        ..Default::default()
                    },
                )
                .unwrap();
            let mesh = runtime
                .take_pending_geometries()
                .into_iter()
                .find(|mesh| mesh.id == rig)
                .unwrap();
            assert_eq!(mesh_pixel(&pages, location, &mesh)[0], 80);
            assert_eq!(span, [1.0; 2]);
        }
    }
}

#[test]
fn compiled_weapon_traits_preserve_every_retail_native_model_choice() {
    for record in &assets::facts::ItemFacts::builtin()
        .unwrap()
        .payload()
        .records
    {
        let old = match record.identifier.strip_prefix("minecraft:").unwrap() {
            "bow" => Some(NativeItem::Bow),
            "crossbow" => Some(NativeItem::Crossbow),
            "trident" => Some(NativeItem::Spear),
            name if name.ends_with("_spear") => Some(NativeItem::Spear),
            _ => None,
        };
        assert_eq!(
            NativeItem::from_identifier(&record.identifier),
            old,
            "{}",
            record.identifier
        );
    }
}

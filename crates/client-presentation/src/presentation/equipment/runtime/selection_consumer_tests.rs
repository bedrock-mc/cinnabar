//! Selected definitions retain fallback artwork and the sampled owner frame.
use super::*;

#[test]
fn selection_consumers_ownerless_same_id_conditions_require_unrestricted_fallback() {
    for (identifier, worn) in [("minecraft:iron_helmet", true), ("fixture:tool", false)] {
        for condition in ["0", "1"] {
            let mut files = variant_pack(identifier, "fixture:unused", false);
            files.retain(|(path, _)| !path.contains("mapped"));
            let (_, bytes) = files
                .iter_mut()
                .find(|(path, _)| path.as_ref() == "attachables/ordinary.json")
                .unwrap();
            let mut json: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let description = &mut json["minecraft:attachable"]["description"];
            description["item"] = serde_json::json!({identifier: condition});
            description["textures"]["default"] = serde_json::json!("textures/items/restricted");
            *bytes = serde_json::to_vec(&json).unwrap();
            let (path, bytes) = files
                .iter_mut()
                .find(|(path, _)| path.as_ref() == "textures/items/held.png")
                .unwrap();
            *path = "textures/items/restricted.png".into();
            let mut png = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(16, 16, image::Rgba([0, 255, 0, 255]))
                .write_to(&mut png, image::ImageFormat::Png)
                .unwrap();
            *bytes = png.into_inner();
            let mut input = held(identifier);
            if worn {
                input.armor[0] = input.main.take();
            }
            let (mut runtime, _) = startup_runtime(files.clone());
            let body = player_body(&mut runtime);
            assert!(runtime.layers_for(&body, &input, None).is_empty());

            let (mut runtime, pages) =
                startup_runtime(variant_pack(identifier, "fixture:unused", false));
            let (entities, catalog) = pack_catalog(files);
            let (pages, locations) =
                pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
            runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
            let body = player_body(&mut runtime);
            let drawn = runtime.layers_for(&body, &input, None);
            assert_eq!(drawn.len(), 1, "{identifier} condition={condition}");
            assert_eq!(
                crate::presentation::equipment::tests::elytra_tests::selected_pixel(
                    &pages, &drawn[0]
                ),
                [255, 0, 0, 255],
                "{identifier} condition={condition}"
            );
        }
    }
}

/// Gives each startup owner mapping a distinct original raster.
fn startup_variants(item: &str) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = variant_pack(item, "fixture:a_player", false);
    for (name, condition, color) in [
        (
            "ordinary",
            "q.owner_identifier == 'minecraft:skeleton'",
            [210, 70, 40, 255],
        ),
        (
            "mapped",
            "q.owner_identifier == 'minecraft:player'",
            [40, 190, 100, 255],
        ),
    ] {
        let (_, bytes) = files
            .iter_mut()
            .find(|(path, _)| path.as_ref() == format!("attachables/{name}.json"))
            .unwrap();
        let mut json: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        let definition = &mut json["minecraft:attachable"]["description"];
        if name == "ordinary" {
            definition["identifier"] = serde_json::json!("fixture:z_skeleton");
        }
        definition["item"] = serde_json::json!({(item):condition});
        definition["textures"]["default"] = serde_json::json!(format!("textures/items/{name}"));
        *bytes = serde_json::to_vec(&json).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(16, 16, image::Rgba(color))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        files.push((
            format!("textures/items/{name}.png").into(),
            png.into_inner(),
        ));
    }
    files
}

#[test]
fn selection_consumers_fallback_artwork_keeps_the_startup_owner_in_both_views() {
    let item = "fixture:z_tool";
    for player in [true, false] {
        for first_person in [true, false] {
            let (mut runtime, pages) = startup_runtime(startup_variants(item));
            let body = player_body(&mut runtime);
            let mut actor = owner();
            if player {
                actor.kind = protocol::ActorKind::Player {
                    uuid: [2; 16],
                    username: "fixture".into(),
                };
            }
            let names = [Box::from("rightItem")];
            let rig = owner_rig(&actor, &names);
            let files = variant_pack(item, "fixture:unused", false)
                .into_iter()
                .filter(|(path, _)| !path.contains("mapped"))
                .collect();
            let (entities, catalog) = pack_catalog(files);
            runtime.set_pack_layer(Some((entities, Some(catalog), Vec::new())));
            let input = held(item);
            let draw = if first_person {
                runtime
                    .first_person_attachable(
                        &body,
                        input.main.as_ref().unwrap(),
                        &actor,
                        &rig,
                        actor_animation::AttachableAnimationInput {
                            first_person: true,
                            ..Default::default()
                        },
                        None,
                    )
                    .unwrap()
                    .presentation
            } else {
                runtime.layers_for(
                    &body,
                    &input,
                    Some(EquipmentAnimation {
                        owner: &actor,
                        rig: &rig,
                        frame_alpha: 1.0,
                        delta_seconds: 0.05,
                    }),
                )[0]
                .clone()
            };
            assert_eq!(
                crate::presentation::equipment::tests::elytra_tests::selected_pixel(&pages, &draw),
                if player {
                    [40, 190, 100, 255]
                } else {
                    [210, 70, 40, 255]
                },
                "player={player} first_person={first_person}"
            );
        }
    }
}

#[test]
fn selection_consumers_predicates_sample_the_rendered_swing_in_both_settings() {
    let item = "fixture:z_tool";
    let mut files = variant_pack(item, "fixture:a_tool", false);
    let (_, bytes) = files
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "attachables/mapped.json")
        .unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    json["minecraft:attachable"]["description"]["item"] = serde_json::json!({(item):"t.sample = t.sample + 1; return t.sample == 1 && q.body_y_rotation == 0 && v.attack_time > 0.5;"});
    *bytes = serde_json::to_vec(&json).unwrap();
    for java in [false, true] {
        let (mut runtime, _) = startup_runtime(files.clone());
        let body = player_body(&mut runtime);
        let actor = owner();
        let names = [Box::from("rightItem")];
        let mut rig = owner_rig(&actor, &names);
        rig.body_yaw = 90.0;
        rig.item_animation[0].attack_time = 0.0;
        rig.item_animation[1].attack_time = 1.0;
        let input = held(item);
        let mut meshes = Vec::new();
        for alpha in [0.25, 0.75, 0.5, 0.25, 0.75] {
            rig.java.local_swing_alpha = java.then_some(1.0 - alpha);
            let draw = runtime
                .first_person_attachable(
                    &body,
                    input.main.as_ref().unwrap(),
                    &actor,
                    &rig,
                    actor_animation::AttachableAnimationInput {
                        first_person: true,
                        frame_alpha: alpha,
                        ..Default::default()
                    },
                    None,
                )
                .unwrap();
            let (entities, catalog) = pack_catalog(files.clone());
            let selected = if if java { 1.0 - alpha } else { alpha } > 0.5 {
                "geometry.mapped"
            } else {
                "geometry.ordinary"
            };
            let index = render_model::find_geometry_index(&entities, selected).unwrap();
            let expected = render_model::attachable_geometry(
                &entities,
                index as usize,
                draw.presentation.submission.input.rig,
                &catalog.textures()[0],
            )
            .unwrap();
            meshes.extend(runtime.take_pending_geometries());
            let actual = meshes
                .iter()
                .find(|mesh| mesh.id == draw.presentation.submission.input.rig)
                .unwrap();
            assert!(
                actual.vertices == expected.vertices,
                "java={java} alpha={alpha} selected={selected}"
            );
        }
    }
}

#[test]
fn selection_consumers_elytra_uses_its_admitted_definition() {
    let item = "minecraft:elytra";
    let (mut runtime, pages) = startup_runtime(startup_variants(item));
    let body = player_body(&mut runtime);
    let actor = crate::presentation::equipment::tests::elytra_tests::owner();
    let names = [Box::from("body")];
    let rig = owner_rig(&actor, &names);
    let mut input = held(item);
    input.armor[1] = input.main.take();
    let drawn = runtime.layers_for(
        &body,
        &input,
        Some(EquipmentAnimation {
            owner: &actor,
            rig: &rig,
            frame_alpha: 1.0,
            delta_seconds: 0.05,
        }),
    );
    assert_eq!(drawn.len(), 1);
    assert_eq!(
        crate::presentation::equipment::tests::elytra_tests::selected_pixel(&pages, &drawn[0]),
        [40, 190, 100, 255]
    );
}

#[test]
fn selection_consumers_ownerless_worn_draws_reject_conditional_definitions() {
    let item = "minecraft:iron_helmet";
    for conditional in [true, false] {
        let files = if conditional {
            startup_variants(item)
        } else {
            variant_pack(item, "fixture:a_player", false)
        };
        let (mut runtime, _) = startup_runtime(files);
        let body = player_body(&mut runtime);
        let mut input = held(item);
        input.armor[0] = input.main.take();
        assert_eq!(
            runtime.layers_for(&body, &input, None).is_empty(),
            conditional
        );
    }
}

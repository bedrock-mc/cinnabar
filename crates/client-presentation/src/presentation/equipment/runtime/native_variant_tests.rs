//! Mixed native and authored models must retain the owner's selected definition.

use super::*;

const BOW: &str = "minecraft:bow";

/// Supplies startup bow artwork and its inventory sprite so Java fallback can draw.
fn bow_startup_runtime() -> (EquipmentRuntime, render_model::ActorArtworkPages) {
    let (entities, catalog) = pack_catalog(fixture(BOW, "geometry.bow_standby", 10, true));
    let texture = catalog
        .texture(&catalog.binding(BOW).unwrap().texture.identifier)
        .unwrap();
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog(
                [0; 32],
                &[assets::IconSprite {
                    width: texture.width,
                    height: texture.height,
                    rgba8: Arc::clone(&texture.rgba8),
                }],
                &[assets::IconEntry {
                    identifier: BOW.into(),
                    metadata: 0,
                    sprite: 0,
                }],
            )
            .unwrap(),
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
fn java_bow_third_person_hand_follows_the_owner_admitted_definition() {
    let draw = |condition: Option<&str>| {
        let (mut runtime, mut pages) = bow_startup_runtime();
        if let Some(condition) = condition {
            let mut files = fixture(BOW, "geometry.bow_standby", 80, true);
            let (_, bytes) = files
                .iter_mut()
                .find(|(path, _)| path.as_ref() == "attachables/weapon.json")
                .unwrap();
            let mut json: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            json["minecraft:attachable"]["description"]["item"] =
                serde_json::json!({BOW: condition});
            *bytes = serde_json::to_vec(&json).unwrap();
            pages = install(&mut runtime, pages, files);
        }
        let body = player_body(&mut runtime);
        let actor = owner();
        let names = [Box::from("rightItem")];
        let rig = owner_rig(&actor, &names);
        let equipment = ActorEquipmentInput {
            main: Some(item(BOW)),
            java: Some(JavaGrip::default()),
            ..Default::default()
        };
        let layers = runtime.layers_for(
            &body,
            &equipment,
            Some(EquipmentAnimation {
                owner: &actor,
                rig: &rig,
                frame_alpha: 0.5,
                delta_seconds: 0.05,
            }),
        );
        assert_eq!(layers.len(), 1);
        (layers[0].clone(), pages)
    };
    let (baseline, _) = draw(None);
    let (rejected, _) = draw(Some("q.owner_identifier == 'minecraft:player'"));
    let (admitted, pages) = draw(Some("q.owner_identifier != 'minecraft:player'"));
    assert_eq!(
        rejected.submission.input.current_bones,
        baseline.submission.input.current_bones
    );
    assert_eq!(
        rejected.submission.input.previous_bones,
        baseline.submission.input.previous_bones
    );
    assert_eq!(rejected.location, baseline.location);
    assert_ne!(
        admitted.submission.input.current_bones,
        baseline.submission.input.current_bones
    );
    assert_eq!(selected_pixel(&pages, &admitted)[0], 80);
}

#[test]
fn same_id_native_conditions_select_pack_or_startup_artwork_in_both_views() {
    for player in [false, true] {
        for first_person in [false, true] {
            for java in [false, true] {
                let mut files = fixture(BOW, "geometry.bow_standby", 80, true);
                let (_, bytes) = files
                    .iter_mut()
                    .find(|(path, _)| path.as_ref() == "attachables/weapon.json")
                    .unwrap();
                let mut json: serde_json::Value = serde_json::from_slice(bytes).unwrap();
                json["minecraft:attachable"]["description"]["item"] =
                    serde_json::json!({BOW: "q.owner_identifier == 'minecraft:player'"});
                *bytes = serde_json::to_vec(&json).unwrap();
                let (mut runtime, pages) = bow_startup_runtime();
                let pages = install(&mut runtime, pages, files);
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
                let stack = item(BOW);
                let hand = render_model::java_animation::JavaHand {
                    swing: 0.0,
                    equip: 1.0,
                    using: None,
                };
                let draw = if first_person {
                    let input = AttachableAnimationInput {
                        first_person: true,
                        ..Default::default()
                    };
                    runtime
                        .first_person_attachable(
                            &body,
                            &stack,
                            &actor,
                            &rig,
                            input,
                            java.then_some(hand),
                        )
                        .or_else(|| {
                            if java {
                                runtime.first_person_java_item_with_use(&body, &stack, hand, input)
                            } else {
                                runtime.first_person_item_with_use(
                                    &body,
                                    &stack,
                                    FirstPersonHand {
                                        swing: 0.0,
                                        equip: 1.0,
                                        consume: None,
                                        aspect: 1.0,
                                    },
                                    render_api::DEFAULT_COMBAT_VISUALS,
                                    input,
                                )
                            }
                        })
                        .unwrap()
                        .presentation
                } else {
                    runtime.layers_for(
                        &body,
                        &ActorEquipmentInput {
                            main: Some(stack),
                            java: java.then_some(JavaGrip::default()),
                            ..Default::default()
                        },
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
                    selected_pixel(&pages, &draw)[0],
                    if player { 80 } else { 10 },
                    "player={player} first_person={first_person} java={java}"
                );
            }
        }
    }
}

/// Adds an owner-mapped literal cube to an otherwise native sprite bow pack.
fn mixed_fixture() -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = fixture(BOW, "geometry.bow_standby", 80, true);
    let mut custom = fixture(
        "fixture:custom_bow",
        "geometry.fixture.custom_bow",
        120,
        false,
    );
    for (path, bytes) in &mut custom {
        if path.starts_with("textures/") {
            continue;
        }
        let mut json: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        if path.starts_with("attachables/") {
            let description = &mut json["minecraft:attachable"]["description"];
            description["item"] =
                serde_json::json!({BOW: "q.owner_identifier == 'minecraft:player'"});
            description["animations"] = serde_json::json!({"wield_third_person":"animation.fixture.custom.wield_third_person"});
            description["scripts"]["animate"] = serde_json::json!(["wield_third_person"]);
        } else if path.starts_with("models/") {
            let bone = &mut json["minecraft:geometry"][0]["bones"][0];
            bone.as_object_mut().unwrap().remove("texture_meshes");
            bone["cubes"] = serde_json::json!([{"origin":[0,0,0],"size":[6,10,4],"uv":[0,0]}]);
        } else if path.starts_with("animations/") {
            json["animations"] = serde_json::json!({"animation.fixture.custom.wield_third_person":{
                "loop":true,"bones":{"rightitem":{"position":[2,3,4],"rotation":[0,0,0]}}
            }});
        } else {
            continue; // Both definitions intentionally share the ordinary render controller.
        }
        *bytes = serde_json::to_vec(&json).unwrap();
        *path = path.replace("weapon", "custom").into();
    }
    files.extend(
        custom
            .into_iter()
            .filter(|(path, _)| !path.starts_with("render_controllers/")),
    );
    files
}

/// Checks the emitted mesh and artwork against the selected compiled definition.
fn assert_selected(
    runtime: &mut EquipmentRuntime,
    pages: &render_model::ActorArtworkPages,
    entities: &assets::RuntimeEntityAssets,
    catalog: &assets::RuntimeEquipmentCatalog,
    draw: &EquipmentPresentation,
    player: bool,
    literal: bool,
) {
    let name = if player {
        "geometry.fixture.custom_bow"
    } else {
        "geometry.bow_standby"
    };
    let index = render_model::find_geometry_index(entities, name).unwrap() as usize;
    let selected = if player { "fixture:custom_bow" } else { BOW };
    let binding = catalog
        .bindings_for(BOW)
        .iter()
        .find(|binding| binding.attachable_identifier() == selected)
        .unwrap();
    if literal {
        // Static pack meshes are uploaded by the pack publisher, outside this runtime.
        assert_eq!(
            draw.submission.input.rig,
            render_model::pack_equipment_rig_id(index as u32)
        );
    } else {
        let expected = render_model::attachable_geometry(
            entities,
            index,
            draw.submission.input.rig,
            catalog.texture(&binding.texture.identifier).unwrap(),
        )
        .unwrap();
        let meshes = runtime.take_pending_geometries();
        let actual = meshes
            .iter()
            .find(|mesh| mesh.id == draw.submission.input.rig)
            .unwrap();
        assert!(
            actual.vertices == expected.vertices,
            "wrong mesh for {selected}"
        );
    }
    assert_eq!(
        selected_pixel(pages, draw)[0],
        if player { 120 } else { 80 }
    );
    assert_eq!(
        draw.submission.material.glint.texture,
        render_model::GlintTexture::Item
    );
}

#[test]
fn mixed_native_variants_keep_selected_first_person_models_in_both_settings() {
    for player in [true, false] {
        for java in [false, true] {
            let files = mixed_fixture();
            let (entities, catalog) = pack_catalog(files.clone());
            let (mut runtime, pages) = bow_startup_runtime();
            let pages = install(&mut runtime, pages, files);
            let body = player_body(&mut runtime);
            let mut owner = owner();
            if player {
                owner.kind = protocol::ActorKind::Player {
                    uuid: [2; 16],
                    username: "fixture".into(),
                };
            }
            let names = [Box::from("rightItem")];
            let rig = owner_rig(&owner, &names);
            let input = AttachableAnimationInput {
                first_person: true,
                ..Default::default()
            };
            let stack = item(BOW);
            let hand = render_model::java_animation::JavaHand {
                swing: 0.0,
                equip: 1.0,
                using: None,
            };
            let draw = runtime
                .first_person_attachable(&body, &stack, &owner, &rig, input, java.then_some(hand))
                .or_else(|| {
                    if java {
                        runtime.first_person_java_item_with_use(&body, &stack, hand, input)
                    } else {
                        runtime.first_person_item_with_use(
                            &body,
                            &stack,
                            FirstPersonHand {
                                swing: 0.0,
                                equip: 1.0,
                                consume: None,
                                aspect: 1.0,
                            },
                            render_api::DEFAULT_COMBAT_VISUALS,
                            input,
                        )
                    }
                })
                .unwrap();
            assert_selected(
                &mut runtime,
                &pages,
                &entities,
                &catalog,
                &draw.presentation,
                player,
                false,
            );
        }
    }
}

#[test]
fn mixed_native_variants_keep_selected_third_person_models_in_both_settings() {
    for player in [true, false] {
        for java in [false, true] {
            let files = mixed_fixture();
            let (entities, catalog) = pack_catalog(files.clone());
            let (mut runtime, pages) = bow_startup_runtime();
            let pages = install(&mut runtime, pages, files);
            let body = player_body(&mut runtime);
            let mut owner = owner();
            if player {
                owner.kind = protocol::ActorKind::Player {
                    uuid: [2; 16],
                    username: "fixture".into(),
                };
            }
            let names = [Box::from("rightItem")];
            let rig = owner_rig(&owner, &names);
            let input = ActorEquipmentInput {
                main: Some(item(BOW)),
                java: java.then_some(JavaGrip::default()),
                ..Default::default()
            };
            let drawn = runtime
                .layers_for(
                    &body,
                    &input,
                    Some(EquipmentAnimation {
                        owner: &owner,
                        rig: &rig,
                        frame_alpha: 1.0,
                        delta_seconds: 0.05,
                    }),
                )
                .to_vec();
            assert_eq!(drawn.len(), 1);
            assert_selected(
                &mut runtime,
                &pages,
                &entities,
                &catalog,
                &drawn[0],
                player,
                java && player,
            );
        }
    }
}

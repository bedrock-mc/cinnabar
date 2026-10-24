use super::*;
use crate::presentation::equipment::LAYER_MAIN_HAND;
use crate::presentation::equipment::display::LAYER_OFF_HAND;
use render_model::ActorRigSubmission;

/// Authors one selected texture with independent controller shading and raster state.
fn styled_pack(material: &str, ignore_lighting: bool) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = held_pack();
    for (path, bytes) in &mut files {
        if path.as_ref() == "attachables/held.json" {
            let mut definition: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            definition["minecraft:attachable"]["description"]["materials"]["default"] =
                material.into();
            *bytes = serde_json::to_vec(&definition).unwrap();
        } else if path.as_ref() == "render_controllers/held.json" {
            let mut definition: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let controller = &mut definition["render_controllers"]["controller.render.held"];
            controller["color"] = serde_json::json!({"r": 1.0, "g": 0.25, "b": 0.5, "a": 1.0});
            controller["overlay_color"] =
                serde_json::json!({"r": 0.1, "g": 0.2, "b": 0.3, "a": 0.4});
            controller["uv_anim"] =
                serde_json::json!({"offset": [0.25, 0.125], "scale": [0.5, 0.75]});
            controller["ignore_lighting"] = ignore_lighting.into();
            controller["light_color_multiplier"] = 0.6.into();
            *bytes = serde_json::to_vec(&definition).unwrap();
        }
    }
    let mut materials = serde_json::json!({"materials": {"version": "1.0.0"}});
    materials["materials"][format!("{material}:entity_alphatest")] = serde_json::json!({
        "+states": ["DisableCulling"], "+defines": ["USE_OVERLAY"], "depthFunc": "Always"
    });
    files.push((
        "materials/held.material".into(),
        serde_json::to_vec(&materials).unwrap(),
    ));
    files
}

/// Checks the published style against the actual compiled and evaluated controller layer.
fn assert_style(
    submission: &ActorRigSubmission,
    selected: &actor_animation::RenderTextureLayer,
    enchanted: bool,
) {
    assert_eq!(submission.material.kind, selected.material);
    assert_eq!(submission.material.state, selected.material_state);
    assert_eq!(submission.material.dissolve_multiplier, selected.overlay[3]);
    assert_eq!(
        submission.material.light_color_multiplier,
        selected.light_color_multiplier
    );
    assert_eq!(
        submission.tint,
        render_model::pack_overlay_rgba8(selected.color)
    );
    assert_eq!(
        submission.overlay_rgba8,
        render_model::pack_overlay_rgba8(selected.overlay)
    );
    assert_eq!(submission.uv_anim, selected.uv_anim);
    assert_eq!(
        submission.light,
        render_model::pack_actor_light_without_lightmap()
    );
    let expected_glint = if enchanted {
        render_model::ActorGlint {
            profile: render_api::CombatVisuals::Modern,
            texture: render_model::GlintTexture::Item,
            ..Default::default()
        }
    } else {
        render_model::ActorGlint::default()
    };
    assert_eq!(submission.material.glint, expected_glint);
}

#[test]
fn evaluated_held_style_reaches_both_hands_and_third_person_for_startup_and_pack_models() {
    for from_pack in [false, true] {
        let files = styled_pack("fixture_ordinary", true);
        let (entities, _) = pack_catalog(files.clone());
        let mut evaluator = actor_animation::AttachablesRuntime::new(entities);
        let (mut runtime, _) = if from_pack {
            pack_runtime(files)
        } else {
            startup_runtime(files)
        };
        let mut body = player_body(&mut runtime);
        body.overlay_rgba8 = 0x6600_00ff;
        body.light = 123;
        let owner = owner();
        let names = [Box::from("rightItem"), Box::from("leftItem")];
        let rig = owner_rig(&owner, &names);
        for enchanted in [false, true] {
            let mut equipment = held("test:held");
            equipment.main.as_mut().unwrap().enchanted = enchanted;
            equipment.off = equipment.main.clone();
            for first_person in [false, true] {
                for off_hand in [false, true] {
                    let input = actor_animation::AttachableAnimationInput {
                        first_person,
                        off_hand,
                        frame_alpha: 0.5,
                        delta_seconds: Some(0.016),
                        ..Default::default()
                    };
                    let evaluated = evaluator
                        .evaluate("test:held", &owner, &rig, input)
                        .unwrap();
                    assert_eq!(evaluated.render.len(), 1);
                    let selected = &evaluated.render[0];
                    assert_eq!(selected.texture_slot, 0);
                    assert_eq!(selected.material, assets::EntityRenderMaterial::Default);
                    assert_eq!(selected.color, [1.0, 0.25, 0.5, 1.0]);
                    assert_eq!(selected.overlay, [0.1, 0.2, 0.3, 0.4]);
                    assert_eq!(selected.uv_anim, [0.25, 0.125, 0.5, 0.75]);
                    let state = selected.material_state.unwrap();
                    assert!(state.depth_always && !state.cull && !state.disable_overlay);
                    assert!(selected.ignore_lighting);
                    assert_eq!(selected.light_color_multiplier, 0.6);
                    if first_person {
                        let item = runtime
                            .first_person_attachable(
                                &body,
                                equipment.main.as_ref().unwrap(),
                                &owner,
                                &rig,
                                input,
                                None,
                            )
                            .unwrap();
                        assert_style(&item.presentation.submission, selected, enchanted);
                    } else {
                        let layers = runtime.layers_for(
                            &body,
                            &equipment,
                            Some(EquipmentAnimation {
                                owner: &owner,
                                rig: &rig,
                                frame_alpha: 0.5,
                                delta_seconds: 0.016,
                            }),
                        );
                        assert_eq!(layers.len(), 2);
                        let layer = if off_hand {
                            LAYER_OFF_HAND
                        } else {
                            LAYER_MAIN_HAND
                        };
                        let submission = &layers
                            .iter()
                            .find(|presentation| {
                                presentation.submission.input.identity.layer == layer
                            })
                            .unwrap()
                            .submission;
                        assert_style(submission, selected, enchanted);
                    }
                }
            }
        }
        assert_eq!(body.overlay_rgba8, 0x6600_00ff);
        assert_eq!(body.light, 123);
    }
}

#[test]
fn held_dissolve_fallback_keeps_coverage_metadata_without_adding_hurt_lighting() {
    use assets::EntityRenderMaterial::{Default as Ordinary, DissolveColor, DissolveDepth};

    for from_pack in [false, true] {
        for (material, kind, expected_alpha) in [
            ("entity_dissolve_layer0", DissolveDepth, 0.0),
            ("entity_dissolve_layer1", DissolveColor, 0.0),
            ("fixture_ordinary", Ordinary, 0.4),
        ] {
            let files = styled_pack(material, false);
            let (entities, _) = pack_catalog(files.clone());
            let mut evaluator = actor_animation::AttachablesRuntime::new(entities);
            let (mut runtime, _) = if from_pack {
                pack_runtime(files)
            } else {
                startup_runtime(files)
            };
            let mut body = player_body(&mut runtime);
            body.light = render_api::ACTOR_LIGHT_DIRECTIONAL;
            body.overlay_rgba8 = 0x6600_00ff;
            let owner = owner();
            let names = [Box::from("rightItem"), Box::from("leftItem")];
            let rig = owner_rig(&owner, &names);
            let mut equipment = held("test:held");
            equipment.main.as_mut().unwrap().enchanted = true;
            equipment.off = equipment.main.clone();
            for first_person in [false, true] {
                for off_hand in [false, true] {
                    let input = actor_animation::AttachableAnimationInput {
                        first_person,
                        off_hand,
                        frame_alpha: 0.5,
                        delta_seconds: Some(0.016),
                        ..Default::default()
                    };
                    let evaluated = evaluator
                        .evaluate("test:held", &owner, &rig, input)
                        .unwrap();
                    let selected = &evaluated.render[0];
                    assert_eq!(selected.material, kind);
                    assert_eq!(selected.overlay[3], 0.4);
                    assert!(!selected.ignore_lighting);
                    let submission = if first_person {
                        runtime
                            .first_person_attachable(
                                &body,
                                equipment.main.as_ref().unwrap(),
                                &owner,
                                &rig,
                                input,
                                None,
                            )
                            .unwrap()
                            .presentation
                            .submission
                    } else {
                        let layers = runtime.layers_for(
                            &body,
                            &equipment,
                            Some(EquipmentAnimation {
                                owner: &owner,
                                rig: &rig,
                                frame_alpha: 0.5,
                                delta_seconds: 0.016,
                            }),
                        );
                        let layer = if off_hand {
                            LAYER_OFF_HAND
                        } else {
                            LAYER_MAIN_HAND
                        };
                        layers
                            .iter()
                            .find(|presentation| {
                                presentation.submission.input.identity.layer == layer
                            })
                            .unwrap()
                            .submission
                            .clone()
                    };
                    assert_eq!(submission.material.kind, Ordinary);
                    assert_eq!(
                        submission.material.gpu_word()
                            & assets::EntityRenderMaterialState::KIND_MASK,
                        Ordinary as u32
                    );
                    if kind != Ordinary {
                        assert!(!submission.material.allows_overlay());
                    }
                    assert_eq!(submission.material.dissolve_multiplier, 0.4);
                    assert_eq!(submission.light, body.light);
                    let alpha = (submission.overlay_rgba8 >> 24) as f32 / 255.0;
                    for normal in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
                        assert_eq!(
                            render_api::fancy_actor_shade(normal, alpha),
                            render_api::fancy_actor_shade(normal, expected_alpha)
                        );
                        assert!(
                            render_api::fancy_actor_shade(normal, 0.4)
                                > render_api::fancy_actor_shade(normal, 0.0)
                        );
                    }
                    assert_eq!(
                        submission.overlay_rgba8,
                        if expected_alpha == 0.0 {
                            0
                        } else {
                            render_model::pack_overlay_rgba8(selected.overlay)
                        }
                    );

                    assert_eq!(
                        submission.material.glint.texture,
                        render_model::GlintTexture::Item
                    );
                }
            }
        }
    }
}

/// Authors real depth/color passes with optional raster declarations and no Always override.
fn dissolve_pack(
    first: &str,
    companion: Option<&str>,
    ignore_lighting: bool,
    authored_state: bool,
) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = styled_pack(first, ignore_lighting);
    if !authored_state {
        files.retain(|(path, _)| path.as_ref() != "materials/held.material");
    }
    for (path, bytes) in &mut files {
        let mut definition: serde_json::Value = match path.as_ref() {
            "attachables/held.json"
            | "render_controllers/held.json"
            | "materials/held.material" => serde_json::from_slice(bytes).unwrap(),
            _ => continue,
        };
        match path.as_ref() {
            "attachables/held.json" => {
                if let Some(material) = companion {
                    let description = &mut definition["minecraft:attachable"]["description"];
                    description["materials"]["companion"] = material.into();
                    description["render_controllers"] = serde_json::json!([
                        "controller.render.held",
                        "controller.render.companion"
                    ]);
                }
            }
            "render_controllers/held.json" => {
                if companion.is_some() {
                    let mut controller =
                        definition["render_controllers"]["controller.render.held"].clone();
                    controller["materials"] = serde_json::json!([{"*": "Material.companion"}]);
                    definition["render_controllers"]["controller.render.companion"] = controller;
                }
            }
            "materials/held.material" => {
                let mut state =
                    definition["materials"][format!("{first}:entity_alphatest")].clone();
                state.as_object_mut().unwrap().remove("depthFunc");
                definition["materials"][format!("{first}:entity_alphatest")] = state.clone();
                if let Some(material) = companion {
                    definition["materials"][format!("{material}:entity_alphatest")] = state;
                }
            }
            _ => unreachable!(),
        }
        *bytes = serde_json::to_vec(&definition).unwrap();
    }
    files
}

#[test]
fn held_dissolve_passes_keep_a_drawable_packet_in_both_views_and_hands() {
    use assets::EntityRenderMaterial::{Default as Ordinary, DissolveColor, DissolveDepth};

    for from_pack in [false, true] {
        for (first, kind, companion) in [
            (
                "entity_dissolve_layer0",
                DissolveDepth,
                Some("entity_dissolve_layer1"),
            ),
            (
                "entity_dissolve_layer1",
                DissolveColor,
                Some("entity_dissolve_layer0"),
            ),
            ("entity_dissolve_layer0", DissolveDepth, None),
            ("entity_dissolve_layer1", DissolveColor, None),
        ] {
            for authored_state in [false, true] {
                for ignore_lighting in [false, true] {
                    let files = dissolve_pack(first, companion, ignore_lighting, authored_state);
                    let (entities, _) = pack_catalog(files.clone());
                    let mut evaluator = actor_animation::AttachablesRuntime::new(entities);
                    let (mut runtime, _) = if from_pack {
                        pack_runtime(files)
                    } else {
                        startup_runtime(files)
                    };
                    let mut body = player_body(&mut runtime);
                    body.light = render_api::ACTOR_LIGHT_DIRECTIONAL;
                    body.overlay_rgba8 = 0x6600_00ff;
                    let owner = owner();
                    let names = [Box::from("rightItem"), Box::from("leftItem")];
                    let rig = owner_rig(&owner, &names);
                    for enchanted in [false, true] {
                        let mut equipment = held("test:held");
                        equipment.main.as_mut().unwrap().enchanted = enchanted;
                        equipment.off = equipment.main.clone();
                        for first_person in [false, true] {
                            for off_hand in [false, true] {
                                let input = actor_animation::AttachableAnimationInput {
                                    first_person,
                                    off_hand,
                                    frame_alpha: 0.5,
                                    delta_seconds: Some(0.016),
                                    ..Default::default()
                                };
                                let evaluated = evaluator
                                    .evaluate("test:held", &owner, &rig, input)
                                    .unwrap();
                                assert_eq!(
                                    evaluated.render.len(),
                                    1 + usize::from(companion.is_some())
                                );
                                assert!(
                                    evaluated.render.iter().all(|layer| layer.texture_slot == 0)
                                );
                                let selected = &evaluated.render[0];
                                assert_eq!(selected.material, kind);
                                if companion.is_some() {
                                    assert_ne!(evaluated.render[1].material, kind);
                                    assert!(matches!(
                                        evaluated.render[1].material,
                                        DissolveDepth | DissolveColor
                                    ));
                                }
                                let selected_state = selected.material_state;
                                if authored_state {
                                    let state = selected_state.expect("authored raster state");
                                    assert!(state.alpha_test && !state.cull);
                                    assert!(!state.depth_always && !state.disable_overlay);
                                } else {
                                    assert_eq!(
                                        selected_state, None,
                                        "alias without a material definition"
                                    );
                                }
                                assert_eq!(selected.overlay, [0.1, 0.2, 0.3, 0.4]);
                                let submission = if first_person {
                                    runtime
                                        .first_person_attachable(
                                            &body,
                                            equipment.main.as_ref().unwrap(),
                                            &owner,
                                            &rig,
                                            input,
                                            None,
                                        )
                                        .unwrap()
                                        .presentation
                                        .submission
                                } else {
                                    let layers = runtime.layers_for(
                                        &body,
                                        &equipment,
                                        Some(EquipmentAnimation {
                                            owner: &owner,
                                            rig: &rig,
                                            frame_alpha: 0.5,
                                            delta_seconds: 0.016,
                                        }),
                                    );
                                    assert_eq!(layers.len(), 2, "one drawable fallback per hand");
                                    let layer = if off_hand {
                                        LAYER_OFF_HAND
                                    } else {
                                        LAYER_MAIN_HAND
                                    };
                                    layers
                                        .iter()
                                        .find(|draw| draw.submission.input.identity.layer == layer)
                                        .unwrap()
                                        .submission
                                        .clone()
                                };
                                assert_eq!(submission.route, render_model::ActorRigRoute::Compiled);
                                assert!(!submission.input.current_bones.is_empty());
                                assert_eq!(submission.material.kind, Ordinary);
                                let word = submission.material.gpu_word();
                                assert_eq!(
                                    word & assets::EntityRenderMaterialState::KIND_MASK,
                                    Ordinary as u32
                                );
                                assert_eq!(submission.material.state, selected_state);
                                assert_ne!(
                                    word & assets::EntityRenderMaterialState::DISABLE_OVERLAY,
                                    0
                                );
                                let expected_state = selected_state.map(|mut state| {
                                    state.disable_overlay = true;
                                    state
                                });
                                assert_eq!(
                                    assets::EntityRenderMaterialState::from_word(word),
                                    expected_state
                                );
                                assert_eq!(
                                    word & assets::EntityRenderMaterialState::AUTHORED != 0,
                                    authored_state
                                );
                                assert!(!submission.material.allows_overlay());
                                assert_eq!(submission.overlay_rgba8, 0);
                                assert_eq!(
                                    submission.material.dissolve_multiplier,
                                    selected.overlay[3]
                                );
                                assert_eq!(
                                    submission.tint,
                                    render_model::pack_overlay_rgba8(selected.color)
                                );
                                assert_eq!(submission.uv_anim, selected.uv_anim);
                                assert_eq!(
                                    submission.material.light_color_multiplier,
                                    selected.light_color_multiplier
                                );
                                assert_eq!(
                                    submission.light,
                                    if ignore_lighting {
                                        render_model::pack_actor_light_without_lightmap()
                                    } else {
                                        body.light
                                    }
                                );
                                assert_eq!(
                                    submission.material.glint,
                                    if enchanted {
                                        render_model::ActorGlint {
                                            profile: render_api::CombatVisuals::Modern,
                                            texture: render_model::GlintTexture::Item,
                                            ..Default::default()
                                        }
                                    } else {
                                        Default::default()
                                    }
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

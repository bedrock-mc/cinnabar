//! Original equipment fixtures cover each worn slot and held-item separation.
use super::elytra_tests::{owner, owner_rig};
use super::*;
use crate::presentation::equipment::runtime::{
    ActorEquipmentInput, EquipmentAnimation, HeldKind, JavaGrip, StagedSessionIcons, WornItem,
};
use render_api::{BEDROCK_112, CombatVisuals};

/// Creates an original four-piece set over the small fixture geometry.
fn armor_pack(leather: bool) -> Vec<(Box<str>, Vec<u8>)> {
    let original = crown_pack();
    let template: serde_json::Value = serde_json::from_slice(&original[0].1).unwrap();
    let mut files: Vec<_> = original
        .into_iter()
        .filter(|(path, _)| !path.starts_with("attachables/"))
        .collect();
    for slot in ["helmet", "chestplate", "leggings", "boots"] {
        let mut attachable = template.clone();
        let description = &mut attachable["minecraft:attachable"]["description"];
        description["identifier"] = format!(
            "minecraft:{}_{slot}",
            if leather { "leather" } else { "iron" }
        )
        .into();
        description["materials"]["default"] =
            if leather { "armor_leather" } else { "armor" }.into();
        files.push((
            format!("attachables/{slot}.json").into(),
            serde_json::to_vec(&attachable).unwrap(),
        ));
    }
    files
}

/// Creates a stack with explicit rendering facts instead of installed item assets.
fn stack(identifier: &str, leather: bool, enchanted: bool) -> WornItem {
    WornItem {
        variant: Default::default(),
        map_id: None,
        charged_projectile: None,
        use_facts: Default::default(),
        grip: assets::item::grip::builtin_item_grip(identifier),
        identifier: identifier.into(),
        metadata: 0,
        damage: None,
        kind: HeldKind::Other,
        dye_rgb: leather.then_some(0x00_33_cc_66),
        enchanted,
        glint: Default::default(),
    }
}

#[test]
fn all_worn_slots_keep_dye_and_glint_with_legacy_precedence_over_java() {
    for leather in [false, true] {
        for enchanted in [false, true] {
            let (mut runtime, pages) = pack_runtime(armor_pack(leather));
            let mut body = player_body(&mut runtime);
            body.overlay_rgba8 = render_model::pack_overlay_rgba8(BEDROCK_112.hurt_overlay);
            let mut input = ActorEquipmentInput {
                armor: ["helmet", "chestplate", "leggings", "boots"].map(|slot| {
                    Some(stack(
                        &format!(
                            "minecraft:{}_{slot}",
                            if leather { "leather" } else { "iron" }
                        ),
                        leather,
                        enchanted,
                    ))
                }),
                ..Default::default()
            };
            let icons = session_items(vec![("test:gem", Default::default())], vec!["test:gem"]);
            let staged = StagedSessionIcons::stage(Some(&icons)).unwrap();
            let (_, locations) = pages.with_equipment_rasters(staged.rasters());
            runtime.set_session_items(Some(&icons), Some(staged), locations);
            let mut held = stack("test:gem", false, false);
            held.kind = HeldKind::Sprite;
            input.main = Some(held.clone());
            input.off = Some(held.clone());
            for java in [false, true] {
                input.java = java.then_some(JavaGrip { blocking: false });
                for profile in [CombatVisuals::Modern, CombatVisuals::Legacy112] {
                    let layers = runtime
                        .layers_for_profile(&body, &input, profile, None)
                        .layers;
                    assert_eq!(layers.len(), input.armor.len() + 2);
                    for layer in layers {
                        let id = layer.submission.input.identity.layer;
                        if id < crate::presentation::equipment::display::LAYER_HELMET {
                            assert_eq!(layer.submission.overlay_rgba8, 0);
                        } else {
                            let legacy = profile == CombatVisuals::Legacy112;
                            assert_eq!(
                                layer.submission.overlay_rgba8,
                                if legacy || java {
                                    body.overlay_rgba8
                                } else {
                                    0
                                }
                            );
                            assert_eq!(layer.submission.material.glint.profile, profile);
                            assert_eq!(layer.submission.material.color_mask, legacy && leather);
                            assert_eq!(
                                layer.submission.tint,
                                if leather {
                                    pack_tint(input.armor[0].as_ref().unwrap().dye_rgb.unwrap())
                                } else {
                                    0
                                }
                            );
                            assert_eq!(
                                layer.submission.material.kind,
                                if legacy && enchanted {
                                    assets::EntityRenderMaterial::Glint
                                } else {
                                    assets::EntityRenderMaterial::Default
                                }
                            );
                        }
                    }
                }
            }
            let arm = runtime
                .mask_first_person(&body, FirstPersonArms::for_hands(None, None))
                .unwrap();
            assert_eq!(arm.overlay_rgba8, body.overlay_rgba8);
            assert_eq!(
                runtime
                    .first_person_item(&body, &held, actor_animation::ItemAnimationState::default())
                    .unwrap()
                    .presentation
                    .submission
                    .overlay_rgba8,
                0
            );
        }
    }
}

/// Adds an authored controller and optional overlay-suppression material to the fixture helmet.
fn controller_pack(suppressed: bool) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = crown_pack();
    let mut declaration: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    let description = &mut declaration["minecraft:attachable"]["description"];
    description["materials"]["default"] = if suppressed {
        "fixture_hidden"
    } else {
        "armor"
    }
    .into();
    description["render_controllers"] = serde_json::json!(["controller.render.fixture"]);
    files[0].1 = serde_json::to_vec(&declaration).unwrap();
    let mut controller = serde_json::json!({"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]});
    if !suppressed {
        controller["overlay_color"] = serde_json::json!({"r":0.2,"g":0.3,"b":0.8,"a":0.7});
    }
    files.push(("render_controllers/fixture.json".into(),serde_json::to_vec(&serde_json::json!({"format_version":"1.8.0","render_controllers":{"controller.render.fixture":controller}})).unwrap()));
    files.push(("materials/fixture.material".into(),serde_json::to_vec(&serde_json::json!({"materials":{"fixture_hidden:entity_alphatest":{"-defines":["USE_OVERLAY"]}}})).unwrap()));
    files
}

#[test]
fn worn_controllers_keep_explicit_overlay_and_suppression() {
    for suppressed in [false, true] {
        let (mut runtime, _) = pack_runtime(controller_pack(suppressed));
        let mut body = player_body(&mut runtime);
        body.overlay_rgba8 = render_model::pack_overlay_rgba8(BEDROCK_112.hurt_overlay);
        let items = session_items(
            vec![(
                "test:crown",
                protocol::ItemComponents {
                    wearable_slot: Some("slot.armor.head".into()),
                    ..Default::default()
                },
            )],
            vec![],
        );
        runtime.set_session_items(Some(&items), None, Vec::new());
        let input = ActorEquipmentInput {
            armor: [Some(stack("test:crown", false, false)), None, None, None],
            ..Default::default()
        };
        let owner = owner();
        let names = [Box::<str>::from("head")];
        let rig = owner_rig(&owner, &names, 7);
        let layers = runtime
            .layers_for_profile(
                &body,
                &input,
                CombatVisuals::Legacy112,
                Some(EquipmentAnimation {
                    owner: &owner,
                    rig: &rig,
                    frame_alpha: 0.5,
                    delta_seconds: 0.025,
                }),
            )
            .layers;
        assert_eq!(layers.len(), 1);
        let worn = &layers[0].submission;
        if suppressed {
            assert!(worn.material.state.unwrap().disable_overlay);
            assert_eq!(worn.overlay_rgba8, 0);
        } else {
            assert_eq!(
                worn.overlay_rgba8,
                render_model::pack_overlay_rgba8([0.2, 0.3, 0.8, 0.7])
            );
        }
        assert_eq!(worn.material.glint.profile, CombatVisuals::Legacy112);
    }
}

/// Authors a lit overlay controller with either a material definition or an unresolved alias.
fn worn_style_pack(
    material: &str,
    overlay: [f32; 4],
    authored_state: bool,
) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = controller_pack(false);
    if !authored_state {
        files.retain(|(path, _)| path.as_ref() != "materials/fixture.material");
    }
    for (path, bytes) in &mut files {
        if path.as_ref() == "attachables/crown.json" {
            let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            value["minecraft:attachable"]["description"]["materials"]["default"] = material.into();
            *bytes = serde_json::to_vec(&value).unwrap();
        } else if path.as_ref() == "render_controllers/fixture.json" {
            let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let controller = &mut value["render_controllers"]["controller.render.fixture"];
            controller["color"] = serde_json::json!({"r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0});
            controller["overlay_color"] = serde_json::json!({
                "r": overlay[0], "g": overlay[1], "b": overlay[2], "a": overlay[3]
            });
            *bytes = serde_json::to_vec(&value).unwrap();
        } else if path.as_ref() == "materials/fixture.material" {
            let mut value = serde_json::json!({"materials": {"version": "1.0.0"}});
            value["materials"][format!("{material}:entity_alphatest")] = serde_json::json!({
                "+defines": ["USE_OVERLAY", "USE_COLOR_MASK"]
            });
            *bytes = serde_json::to_vec(&value).unwrap();
        }
    }
    files
}

#[test]
fn worn_dissolve_layers_reject_owner_hurt_after_authored_style_and_combat() {
    use assets::EntityRenderMaterial::{Default as Ordinary, DissolveColor, DissolveDepth, Glint};

    for authored_state in [false, true] {
        for (material, kind) in [
            ("entity_dissolve_layer0", DissolveDepth),
            ("entity_dissolve_layer1", DissolveColor),
            ("fixture_ordinary", Ordinary),
        ] {
            for (overlay, inherits_owner_overlay) in [
                ([0.2, 0.3, 0.8, 0.4], false),
                ([0.2, 0.3, 0.8, 0.0], false),
                ([0.0; 4], true),
            ] {
                let alpha = overlay[3];
                let files = worn_style_pack(material, overlay, authored_state);
                let (entities, catalog) = pack_catalog(files.clone());
                assert_eq!(
                    catalog.binding("test:crown").unwrap().color_mask,
                    if authored_state {
                        assets::EquipmentColorMask::Dye
                    } else {
                        assets::EquipmentColorMask::Unresolved
                    }
                );
                let mut evaluator = actor_animation::AttachablesRuntime::new(entities);
                let (mut runtime, _) = pack_runtime(files);
                let mut body = player_body(&mut runtime);
                body.overlay_rgba8 = render_model::pack_overlay_rgba8(BEDROCK_112.hurt_overlay);
                body.light = render_api::ACTOR_LIGHT_DIRECTIONAL;
                assert_ne!(body.overlay_rgba8, 0);
                let items = session_items(
                    vec![(
                        "test:crown",
                        protocol::ItemComponents {
                            wearable_slot: Some("slot.armor.head".into()),
                            ..Default::default()
                        },
                    )],
                    vec![],
                );
                runtime.set_session_items(Some(&items), None, Vec::new());
                let mut owner = owner();
                owner.status.hurt_time = client_world::HURT_DURATION_TICKS;
                assert!(owner.hurt_overlay_active());
                let names = [Box::<str>::from("head")];
                let rig = owner_rig(&owner, &names, 7);
                let evaluated = evaluator
                    .evaluate(
                        "test:crown",
                        &owner,
                        &rig,
                        actor_animation::AttachableAnimationInput {
                            worn: true,
                            worn_slot: assets::ArmorSlot::Helmet as u8,
                            frame_alpha: 0.5,
                            delta_seconds: Some(0.025),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let selected = &evaluated.render[0];
                assert_eq!(selected.material, kind);
                assert_eq!(selected.overlay, overlay);
                assert_eq!(selected.color, [1.0; 4]);
                if authored_state {
                    assert!(!selected.material_state.unwrap().disable_overlay);
                } else {
                    assert_eq!(
                        selected.material_state, None,
                        "alias without an authored material"
                    );
                }
                for enchanted in [true, false] {
                    let mut input = ActorEquipmentInput {
                        armor: [Some(stack("test:crown", true, enchanted)), None, None, None],
                        ..Default::default()
                    };
                    for java in [false, true] {
                        input.java = java.then_some(JavaGrip { blocking: false });
                        for profile in [CombatVisuals::Legacy112, CombatVisuals::Modern] {
                            let layers = runtime
                                .layers_for_profile(
                                    &body,
                                    &input,
                                    profile,
                                    Some(EquipmentAnimation {
                                        owner: &owner,
                                        rig: &rig,
                                        frame_alpha: 0.5,
                                        delta_seconds: 0.025,
                                    }),
                                )
                                .layers;
                            assert_eq!(layers.len(), 1);
                            let worn = &layers[0].submission;
                            let legacy = profile == CombatVisuals::Legacy112;
                            assert_eq!(
                                worn.material.kind,
                                if legacy && enchanted && !authored_state {
                                    Glint
                                } else if legacy {
                                    kind
                                } else {
                                    Ordinary
                                }
                            );
                            assert_eq!(
                                worn.material.state,
                                if legacy {
                                    selected.material_state
                                } else {
                                    None
                                }
                            );
                            assert_eq!(
                                worn.material.dissolve_multiplier,
                                if legacy { alpha } else { 1.0 }
                            );
                            assert_eq!(worn.material.color_mask, legacy && authored_state);
                            assert_eq!(
                                worn.tint,
                                if authored_state {
                                    pack_tint(input.armor[0].as_ref().unwrap().dye_rgb.unwrap())
                                } else {
                                    0
                                }
                            );
                            assert_eq!(worn.light, body.light);
                            assert_eq!(
                                worn.material.glint,
                                render_model::ActorGlint {
                                    profile,
                                    time_seconds: if legacy {
                                        (rig.completed_tick as f32 + 0.5)
                                            * simulation_clock::TICK_DURATION.as_secs_f32()
                                    } else {
                                        0.0
                                    },
                                    ..Default::default()
                                }
                            );
                            let expected_overlay = if legacy {
                                if kind == Ordinary {
                                    if inherits_owner_overlay {
                                        body.overlay_rgba8
                                    } else {
                                        render_model::pack_overlay_rgba8(selected.overlay)
                                    }
                                } else {
                                    0
                                }
                            } else if java {
                                body.overlay_rgba8
                            } else {
                                0
                            };
                            assert_eq!(
                                worn.overlay_rgba8, expected_overlay,
                                "material {material}, authored state {authored_state}, authored overlay {overlay:?}, enchanted {enchanted}, Java {java}, profile {profile:?}"
                            );
                            let suppressed = legacy && kind != Ordinary;
                            assert_eq!(worn.material.allows_overlay(), !suppressed);
                            assert_eq!(
                                worn.material.gpu_word()
                                    & assets::EntityRenderMaterialState::DISABLE_OVERLAY
                                    != 0,
                                suppressed
                            );
                            assert_eq!(
                                worn.material.gpu_word()
                                    & assets::EntityRenderMaterialState::AUTHORED
                                    != 0,
                                legacy && authored_state
                            );
                            if legacy && kind != Ordinary {
                                assert_eq!(
                                    render_api::fancy_actor_shade(
                                        [0.0, 1.0, 0.0],
                                        (worn.overlay_rgba8 >> 24) as f32 / 255.0
                                    ),
                                    render_api::fancy_actor_shade([0.0, 1.0, 0.0], 0.0)
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

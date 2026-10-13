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
        use_facts: Default::default(),
        grip: assets::item::grip::builtin_item_grip(identifier),
        identifier: identifier.into(),
        metadata: 0,
        damage: None,
        kind: HeldKind::Other,
        dye_rgb: leather.then_some(0x00_33_cc_66),
        enchanted,
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
                    let layers = runtime.layers_for_profile(&body, &input, profile, None);
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
        let layers = runtime.layers_for_profile(
            &body,
            &input,
            CombatVisuals::Legacy112,
            Some(EquipmentAnimation {
                owner: &owner,
                rig: &rig,
                frame_alpha: 0.5,
                delta_seconds: 0.025,
            }),
        );
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

use super::*;
use crate::presentation::equipment::display::{LAYER_MAIN_HAND, LAYER_OFF_HAND};
use render_model::ActorRigSubmission;

/// Authors one selected texture with independent controller shading and raster state.
fn styled_pack() -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = held_pack();
    for (path, bytes) in &mut files {
        if path.as_ref() == "attachables/held.json" {
            let mut definition: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            definition["minecraft:attachable"]["description"]["materials"]["default"] =
                "entity_dissolve_layer1".into();
            *bytes = serde_json::to_vec(&definition).unwrap();
        } else if path.as_ref() == "render_controllers/held.json" {
            let mut definition: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let controller = &mut definition["render_controllers"]["controller.render.held"];
            controller["color"] = serde_json::json!({"r": 1.0, "g": 0.25, "b": 0.5, "a": 1.0});
            controller["overlay_color"] =
                serde_json::json!({"r": 0.1, "g": 0.2, "b": 0.3, "a": 0.4});
            controller["uv_anim"] =
                serde_json::json!({"offset": [0.25, 0.125], "scale": [0.5, 0.75]});
            controller["ignore_lighting"] = true.into();
            controller["light_color_multiplier"] = 0.6.into();
            *bytes = serde_json::to_vec(&definition).unwrap();
        }
    }
    files.push((
        "materials/held.material".into(),
        br#"{"materials":{"version":"1.0.0","entity_dissolve_layer1:entity_alphatest":{
            "+states":["DisableCulling"],"+defines":["USE_OVERLAY"],"depthFunc":"Always"
        }}}"#
            .to_vec(),
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
    assert_eq!(
        submission.material.glint.texture,
        if enchanted {
            render_model::GlintTexture::Item
        } else {
            render_model::GlintTexture::Actor
        }
    );
    assert_eq!(
        submission.material.glint.profile,
        render_api::CombatVisuals::Modern
    );
}

#[test]
fn evaluated_held_style_reaches_both_hands_and_third_person_for_startup_and_pack_models() {
    for from_pack in [false, true] {
        let files = styled_pack();
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
                    assert_eq!(
                        selected.material,
                        assets::EntityRenderMaterial::DissolveColor
                    );
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

//! Held definitions retain owner selection through animated and literal drawing.
use super::*;

/// Builds two original held cubes whose mapping, geometry and placement are independent.
fn variant_pack(item: &str, mapped: &str, external_ordinary: bool) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = held_pack();
    files.retain(|(path, _)| {
        !path.starts_with("attachables/")
            && !path.starts_with("models/")
            && !path.starts_with("animations/")
    });
    for (name, identifier, offset, size) in [("ordinary", item, 4, 2), ("mapped", mapped, 20, 6)] {
        let mut definition = serde_json::json!({"format_version":"1.10.0","minecraft:attachable":{"description":{
            "identifier":identifier,"materials":{"default":"entity_alphatest"},
            "textures":{"default":"textures/items/held"},"geometry":{"default":format!("geometry.{name}")},
            "animations":{"wield_third_person":format!("animation.{name}.wield_third_person")},
            "scripts":{"animate":["wield_third_person"]},"render_controllers":["controller.render.held"]
        }}});
        if name == "mapped" {
            definition["minecraft:attachable"]["description"]["item"] = serde_json::json!({(item):"q.owner_identifier == 'minecraft:player' && v.is_enchanted && c.item_slot == 'main_hand' && q.main_hand_item_max_duration == 32"});
        }
        files.push((
            format!("attachables/{name}.json").into(),
            serde_json::to_vec(&definition).unwrap(),
        ));
        if name != "ordinary" || !external_ordinary {
            files.push((format!("models/entity/{name}.json").into(), serde_json::to_vec(&serde_json::json!({
                "format_version":"1.16.0","minecraft:geometry":[{
                    "description":{"identifier":format!("geometry.{name}"),"texture_width":16,"texture_height":16},
                    "bones":[{"name":"rightitem","cubes":[{"origin":[0,0,0],"size":[size,size,size],"uv":[0,0]}]}]
                }]
            })).unwrap()));
        }
        files.push((format!("animations/{name}.json").into(), serde_json::to_vec(&serde_json::json!({
            "format_version":"1.10.0","animations":{format!("animation.{name}.wield_third_person"):{
                "loop":true,"bones":{"rightitem":{"position":[offset,0,0],"rotation":[0,0,0]}}
            }}
        })).unwrap()));
    }
    files
}

#[test]
fn partial_artwork_held_variants_select_owner_geometry_and_placement_in_both_orders() {
    for (item, mapped) in [
        ("fixture:z_tool", "fixture:a_tool"),
        ("fixture:a_tool", "fixture:z_tool"),
    ] {
        for java in [true, false] {
            for (player, enchanted, external) in [
                (true, true, false),
                (false, true, false),
                (true, false, false),
                (true, true, true),
            ] {
                let files = variant_pack(item, mapped, external);
                let (entities, catalog) = pack_catalog(files.clone());
                let (mut runtime, _) = startup_runtime(files);
                let body = player_body(&mut runtime);
                let mut owner = owner();
                if player {
                    owner.kind = super::super::elytra_tests::owner().kind;
                }
                let names = [Box::from("rightItem")];
                let rig = owner_rig(&owner, &names);
                let mut input = held(item);
                input.java = java.then_some(JavaGrip::default());
                let stack = input.main.as_mut().unwrap();
                stack.enchanted = enchanted;
                stack.use_facts.ticks = Some(32);
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
                assert_eq!(
                    drawn.len(),
                    1,
                    "{item} java={java} player={player} enchanted={enchanted} external={external}"
                );
                let selected = if player && enchanted {
                    "mapped"
                } else {
                    "ordinary"
                };
                let geometry =
                    render_model::find_geometry_index(&entities, &format!("geometry.{selected}"))
                        .unwrap();
                let expected_rig = render_model::equipment_rig_id(geometry);
                if java {
                    assert_eq!(drawn[0].submission.input.rig, expected_rig);
                } else {
                    let texture = catalog.textures().first().unwrap();
                    let expected = render_model::attachable_geometry(
                        &entities,
                        geometry as usize,
                        drawn[0].submission.input.rig,
                        texture,
                    )
                    .unwrap();
                    let meshes = runtime.take_pending_geometries();
                    let actual = meshes
                        .iter()
                        .find(|mesh| mesh.id == drawn[0].submission.input.rig)
                        .unwrap();
                    assert_eq!(actual.vertices, expected.vertices);
                }
                let offset = if selected == "mapped" { 20.0 } else { 4.0 };
                let expected = render_model::equipment::attach(
                    body.input.current_bones[5],
                    [0.0; 3],
                    render_model::equipment::BoneChannels {
                        translation: [offset, 0.0, 0.0],
                        rotation: [0.0; 3],
                        scale: [1.0; 3],
                    },
                    false,
                )
                .unwrap();
                assert_eq!(drawn[0].submission.input.current_bones[0], expected);
            }
        }
    }
}

#[test]
fn owner_context_held_enchantments_select_geometry_without_changing_intrinsic_glint() {
    let item = "fixture:z_tool";
    let files = variant_pack(item, "fixture:a_tool", false);
    let (entities, catalog) = pack_catalog(files.clone());
    for java in [true, false] {
        let (mut runtime, _) = startup_runtime(files.clone());
        let body = player_body(&mut runtime);
        let owner = super::super::elytra_tests::owner();
        let names = [Box::from("rightItem")];
        let rig = owner_rig(&owner, &names);
        let mut meshes = Vec::new();
        for (enchanted, intrinsic) in [
            (false, false),
            (true, false),
            (false, true),
            (true, true),
            (false, false),
        ] {
            let stack = crate::presentation::equipment::input::canonical_fixture(
                item, enchanted, intrinsic,
            );
            let worn = crate::presentation::equipment::input::worn_item(
                &stack,
                None,
                client_world::EffectiveItemUse {
                    ticks: Some(32),
                    ..Default::default()
                },
                false,
            )
            .unwrap();
            let input = ActorEquipmentInput {
                main: Some(worn),
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
            let selected = if enchanted { "mapped" } else { "ordinary" };
            let geometry =
                render_model::find_geometry_index(&entities, &format!("geometry.{selected}"))
                    .unwrap();
            if java {
                assert_eq!(
                    drawn[0].submission.input.rig,
                    render_model::equipment_rig_id(geometry),
                    "enchanted={enchanted} intrinsic={intrinsic}"
                );
            } else {
                let expected = render_model::attachable_geometry(
                    &entities,
                    geometry as usize,
                    drawn[0].submission.input.rig,
                    &catalog.textures()[0],
                )
                .unwrap();
                meshes.extend(runtime.take_pending_geometries());
                let actual = meshes
                    .iter()
                    .find(|mesh| mesh.id == drawn[0].submission.input.rig)
                    .unwrap();
                assert_eq!(actual.vertices, expected.vertices);
            }
            let expected = render_model::equipment::attach(
                body.input.current_bones[5],
                [0.0; 3],
                render_model::equipment::BoneChannels {
                    translation: [if enchanted { 20.0 } else { 4.0 }, 0.0, 0.0],
                    ..Default::default()
                },
                false,
            )
            .unwrap();
            assert_eq!(drawn[0].submission.input.current_bones[0], expected);
            assert_eq!(
                drawn[0].submission.material.glint.texture,
                if enchanted || intrinsic {
                    render_model::GlintTexture::Item
                } else {
                    render_model::GlintTexture::Actor
                }
            );
        }
    }
}

#[path = "selection_consumer_tests.rs"]
mod consumers;

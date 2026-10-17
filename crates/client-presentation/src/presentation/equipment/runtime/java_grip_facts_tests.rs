use super::*;

#[test]
fn java_sprite_tools_use_the_admitted_upright_grip() {
    use super::super::runtime::{ActorEquipmentInput, HeldKind, JavaGrip, WornItem};
    use render_model::java_animation::{JavaHeldItem, JavaItemMesh, third_person_item};
    let facts = assets::facts::ItemFacts::builtin().unwrap();
    let identifiers = ["minecraft:blaze_rod", "minecraft:breeze_rod"];
    let mut files = crown_pack();
    let mut attachable: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    let helmet = "minecraft:leather_helmet";
    assert!(facts.by_identifier(helmet).unwrap().armour_slot.is_some());
    attachable["minecraft:attachable"]["description"]["identifier"] = helmet.into();
    files[0].1 = serde_json::to_vec(&attachable).unwrap();
    let (mut runtime, pages) = pack_runtime(files);
    let items = session_items(vec![], identifiers.to_vec());
    let staged = super::super::runtime::StagedSessionIcons::stage(Some(&items)).unwrap();
    let (_, locations) = pages.with_equipment_rasters(staged.rasters());
    runtime.set_session_items(Some(&items), Some(staged), locations);
    let body = player_body(&mut runtime);
    for identifier in identifiers {
        let input = ActorEquipmentInput {
            main: Some(WornItem {
                identifier: Arc::from(identifier),
                metadata: 0,
                variant: Default::default(),
                damage: None,
                kind: HeldKind::Other,
                grip: facts.by_identifier(identifier).unwrap().grip,
                use_facts: client_world::EffectiveItemUse::default(),
                charged_projectile: None,
                dye_rgb: None,
                enchanted: false,
                glint: Default::default(),
            }),
            java: Some(JavaGrip::default()),
            ..Default::default()
        };
        let layers = runtime.layers_for(&body, &input, None);
        assert_eq!(layers.len(), 1, "{identifier}");
        let expected = attach_to_bone(
            bone([0.0; 3], 1.0),
            ItemDisplay::from_matrix(third_person_item(
                JavaHeldItem::Tool {
                    rotate_around: facts.by_identifier(identifier).unwrap().grip.mirrored_art(),
                    blocking: false,
                },
                JavaItemMesh::Sprite,
            )),
        )
        .unwrap();
        let actual = layers[0].submission.input.current_bones[0];
        for (actual, expected) in actual
            .translation_scale
            .iter()
            .chain(&actual.rotation)
            .zip(expected.translation_scale.iter().chain(&expected.rotation))
        {
            assert!((actual - expected).abs() < 1e-5, "{identifier}");
        }
    }
}

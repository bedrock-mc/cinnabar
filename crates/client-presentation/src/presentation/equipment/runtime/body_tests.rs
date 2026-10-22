//! Original body-slot fixtures keep authored artwork and owner-root placement independent of biped armor.
use super::elytra_tests::{layers, owner, selected_pixel};
use super::{pack_runtime, player_body};
use crate::presentation::equipment::{
    ActorEquipmentInput, BODY_EQUIPMENT_LAYER, HeldKind, WornItem,
};
use std::sync::Arc;

/// Creates two unrelated original attachables that share an authored model and controller.
fn files() -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = vec![
        ("models/entity/body.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture.body","texture_width":8,"texture_height":8},
            "bones":[{"name":"body","cubes":[{"origin":[-1,0,-1],"size":[2,2,2],"uv":[0,0]}]}]
        }]}"#.to_vec()),
        ("render_controllers/body.json".into(), br#"{"format_version":"1.8.0","render_controllers":{
            "controller.render.fixture.body":{"geometry":"Geometry.default","textures":["Texture.default"],
            "materials":[{"*":"Material.default"}],"part_visibility":[{"*":"context.item_slot == 'body'"}]}
        }}"#.to_vec()),
    ];
    for (name, red) in [("a", 40), ("b", 210)] {
        let texture = format!("textures/fixture/{name}");
        let image = image::RgbaImage::from_pixel(8, 8, image::Rgba([red, 30, 50, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        files.push((format!("{texture}.png").into(), bytes.into_inner()));
        files.push((format!("attachables/{name}.json").into(), serde_json::to_vec(&serde_json::json!({
            "format_version":"1.10.0","minecraft:attachable":{"description":{
                "identifier":format!("fixture:{name}"),"geometry":{"default":"geometry.fixture.body"},
                "materials":{"default":"entity_alphatest"},"textures":{"default":texture},
                "render_controllers":["controller.render.fixture.body"]
            }}
        })).unwrap()));
    }
    files
}

/// Supplies one non-humanoid wearable without a fallback sprite or built-in armor classification.
fn item(name: &str) -> WornItem {
    WornItem {
        identifier: format!("fixture:{name}").into(),
        kind: HeldKind::Other,
        metadata: 0,
        damage: None,
        dye_rgb: None,
        enchanted: false,
        glint: Default::default(),
        variant: Default::default(),
        charged_projectile: None,
        use_facts: Default::default(),
        grip: assets::item::grip::ItemGrip::Unresolved,
    }
}

#[test]
fn body_slot_draws_authored_attachable_changes_artwork_and_clears() {
    let (mut runtime, pages) = pack_runtime(files());
    let mut body = player_body(&mut runtime);
    let mut previous = body.input.previous_bones.to_vec();
    previous[1].translation_scale[0] = 2.0;
    body.input.previous_bones = Arc::from(previous.clone());
    body.input.current_bones = previous.into();
    let owner = owner();
    let mut input = ActorEquipmentInput::default();
    assert!(layers(&mut runtime, &body, &owner, &input, 1).is_empty());
    for (tick, name, red) in [(2, "a", 40), (3, "b", 210)] {
        input.body = Some(item(name));
        let drawn = layers(&mut runtime, &body, &owner, &input, tick);
        assert_eq!(drawn.len(), 1, "a body-slot attachable needs its own draw");
        assert_eq!(
            drawn[0].submission.input.identity.layer,
            BODY_EQUIPMENT_LAYER
        );
        assert_eq!(selected_pixel(&pages, &drawn[0])[0], red);
        let root = drawn[0].submission.input.current_bones[0];
        assert!(
            root.translation_scale[3] > 0.0,
            "the controller must receive the body slot context"
        );
        assert!(
            (root.translation_scale[0] - 2.0).abs() < 1e-6,
            "the root must inherit its named owner frame"
        );
    }
    input.body = None;
    assert!(layers(&mut runtime, &body, &owner, &input, 4).is_empty());
    input.armor[1] = Some(item("a"));
    assert!(
        layers(&mut runtime, &body, &owner, &input, 5).is_empty(),
        "an ordinary held definition must not acquire a humanoid chest route"
    );
}

#[test]
fn body_slot_pack_override_and_removal_restore_startup_artwork() {
    let (mut runtime, mut pages) = super::startup_runtime(files());
    let body = player_body(&mut runtime);
    let owner = owner();
    let input = ActorEquipmentInput {
        body: Some(item("a")),
        ..Default::default()
    };
    let drawn = layers(&mut runtime, &body, &owner, &input, 1);
    assert_eq!(drawn.len(), 1, "startup body equipment must draw");
    assert_eq!(selected_pixel(&pages, &drawn[0])[0], 40);
    let mut replacement = files();
    let (_, pixels) = replacement
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "textures/fixture/a.png")
        .unwrap();
    let image = image::RgbaImage::from_pixel(8, 8, image::Rgba([99, 30, 50, 255]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    *pixels = encoded.into_inner();
    let (entities, catalog) = super::pack_catalog(replacement);
    let (next, locations) = pages.with_equipment_rasters(
        &crate::presentation::equipment::EquipmentRuntime::pack_rasters(&catalog),
    );
    pages = next;
    runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
    let drawn = layers(&mut runtime, &body, &owner, &input, 2);
    assert_eq!(drawn.len(), 1, "replacement body equipment must draw");
    assert_eq!(selected_pixel(&pages, &drawn[0])[0], 99);
    runtime.set_pack_layer(None);
    let drawn = layers(&mut runtime, &body, &owner, &input, 3);
    assert_eq!(
        drawn.len(),
        1,
        "removing a pack must restore the startup draw"
    );
    assert_eq!(selected_pixel(&pages, &drawn[0])[0], 40);
}

#[test]
fn harness_binding_draws_on_the_body_without_becoming_a_held_model() {
    let facts = assets::facts::ItemFacts::builtin().unwrap();
    let tag = facts.tag(assets::facts::HARNESS_TAG).unwrap();
    let identifier = &facts
        .payload()
        .records
        .iter()
        .find(|record| record.fact.tags.contains(tag))
        .unwrap()
        .identifier;
    let mut pack = files();
    for (path, bytes) in &mut pack {
        if path.as_ref() == "attachables/a.json" {
            let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            value["minecraft:attachable"]["description"]["identifier"] =
                serde_json::json!(identifier);
            *bytes = serde_json::to_vec(&value).unwrap();
        }
    }
    let (mut runtime, _) = pack_runtime(pack);
    let body = player_body(&mut runtime);
    let owner = owner();
    let names = [Box::<str>::from("body")];
    let rig = super::elytra_tests::owner_rig(&owner, &names, 1);
    let stack = WornItem {
        identifier: identifier.as_ref().into(),
        ..item("a")
    };
    assert!(
        runtime
            .first_person_attachable(
                &body,
                &stack,
                &owner,
                &rig,
                actor_animation::AttachableAnimationInput {
                    first_person: true,
                    ..Default::default()
                },
                None,
            )
            .is_none(),
        "a worn harness model must leave the held-item fallback in charge"
    );
    let input = ActorEquipmentInput {
        body: Some(stack),
        ..Default::default()
    };
    assert_eq!(layers(&mut runtime, &body, &owner, &input, 2).len(), 1);
}

#[test]
fn startup_body_slot_submits_a_registered_mesh() {
    let (mut runtime, _) = super::startup_runtime(files());
    let body = player_body(&mut runtime);
    let owner = owner();
    runtime.take_pending_geometries();
    let input = ActorEquipmentInput {
        body: Some(item("a")),
        ..Default::default()
    };
    let drawn = layers(&mut runtime, &body, &owner, &input, 1);
    assert_eq!(drawn.len(), 1);
    let queued = runtime.take_pending_geometries();
    assert!(
        queued
            .iter()
            .any(|mesh| mesh.id == drawn[0].submission.input.rig && !mesh.vertices.is_empty()),
        "the scene needs geometry for a startup body definition that has no held literal pose"
    );
    layers(&mut runtime, &body, &owner, &input, 2);
    assert!(
        runtime.take_pending_geometries().is_empty(),
        "unchanged geometry stays registered"
    );
}

#[test]
fn body_slot_controller_reads_the_worn_stack_enchantment() {
    let mut source = files();
    for (path, bytes) in &mut source {
        if path.as_ref() == "attachables/a.json" {
            let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            value["minecraft:attachable"]["description"]["textures"]["enchanted"] =
                serde_json::json!("textures/fixture/b");
            *bytes = serde_json::to_vec(&value).unwrap();
        } else if path.as_ref() == "render_controllers/body.json" {
            let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            value["render_controllers"]["controller.render.fixture.body"]["textures"] =
                serde_json::json!(["variable.is_enchanted ? Texture.enchanted : Texture.default"]);
            *bytes = serde_json::to_vec(&value).unwrap();
        }
    }
    for startup in [false, true] {
        let (mut runtime, pages) = if startup {
            super::startup_runtime(source.clone())
        } else {
            pack_runtime(source.clone())
        };
        let body = player_body(&mut runtime);
        let owner = owner();
        for (tick, enchanted, expected) in [(1, false, 40), (2, true, 210), (3, false, 40)] {
            let input = ActorEquipmentInput {
                body: Some(WornItem {
                    enchanted,
                    ..item("a")
                }),
                ..Default::default()
            };
            let drawn = layers(&mut runtime, &body, &owner, &input, tick);
            assert_eq!(drawn.len(), 1);
            assert_eq!(
                selected_pixel(&pages, &drawn[0])[0],
                expected,
                "startup={startup}, enchanted={enchanted}"
            );
        }
    }
}

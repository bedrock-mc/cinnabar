//! Worn pack armour draws the texture its render controller selects from owner state.

use super::{
    elytra_tests::{layers, owner, owner_rig, selected_pixel},
    pack_catalog, pack_runtime, player_body, startup_runtime,
};
use crate::presentation::equipment::runtime::{
    ActorEquipmentInput, EquipmentAnimation, EquipmentRuntime, HeldKind, WornItem,
};
use protocol::ActorMetadataValue;
use std::sync::Arc;

const PLAIN: [u8; 4] = [170, 170, 170, 255];
const PAINTED: [u8; 4] = [30, 150, 70, 255];
/// Actor flag bit read by `query.is_charged`.
const CHARGED_FLAG: u32 = 27;

/// Builds an original armour definition with an optional restriction to stand owners.
fn owner_armour_pack(item: &str, stand_only: bool) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = coat_pack("v.coat=0;");
    let mut definition: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    let description = &mut definition["minecraft:attachable"]["description"];
    description["identifier"] = if stand_only {
        "fixture:stand_armour"
    } else {
        item
    }
    .into();
    if stand_only {
        let native_slot = if item == "minecraft:iron_chestplate" {
            "torso"
        } else {
            ""
        };
        description["item"] = serde_json::json!({(item): format!("query.owner_identifier == 'minecraft:armor_stand' && context.item_slot == '{native_slot}'")});
        description["geometry"]["default"] = "geometry.test.stand_armour".into();
        let mut geometry: serde_json::Value = serde_json::from_slice(&files[1].1).unwrap();
        geometry["minecraft:geometry"][0]["description"]["identifier"] =
            "geometry.test.stand_armour".into();
        geometry["minecraft:geometry"][0]["bones"][0]["name"] = "stand_shell".into();
        files[0].0 = "attachables/stand_armour.json".into();
        files[1].0 = "models/entity/stand_armour.json".into();
        files[1].1 = serde_json::to_vec(&geometry).unwrap();
        for path in description["textures"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            *path = path.as_str().unwrap().replace("/coat_", "/stand_").into();
        }
        for (path, _) in &mut files {
            if path.starts_with("textures/models/coat_") {
                *path = path.replace("/coat_", "/stand_").into();
            }
        }
        files[3].1 = texture(PAINTED);
    }
    files[0].1 = serde_json::to_vec(&definition).unwrap();
    files
}

/// Equips one piece in the slot the compiled item facts admit.
fn worn_armour(item: &str, slot: usize) -> ActorEquipmentInput {
    let mut input = helmet();
    let mut armour = input.armor[0].take().unwrap();
    armour.identifier = Arc::from(item);
    input.armor[slot] = Some(armour);
    input
}

#[test]
fn armour_owner_conditions_keep_player_chestplates_and_leggings() {
    for (item, slot) in [
        ("minecraft:iron_chestplate", 1),
        ("minecraft:iron_leggings", 2),
    ] {
        let (mut runtime, base_pages) = startup_runtime(owner_armour_pack(item, false));
        let body = player_body(&mut runtime);
        let (entities, catalog) = pack_catalog(owner_armour_pack(item, true));
        let (pages, locations) =
            base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
        runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
        let owner = owner();
        let mut input = worn_armour(item, slot);
        for local in [false, true] {
            input.is_local_player = local;
            let drawn = layers(&mut runtime, &body, &owner, &input, 1);
            assert_eq!(drawn.len(), 1);
            assert_eq!(
                selected_pixel(&pages, &drawn[0]),
                PLAIN,
                "{item} local={local}"
            );
            assert!(
                drawn[0]
                    .submission
                    .input
                    .current_bones
                    .iter()
                    .any(|bone| { bone.translation_scale[3] > 0.0 })
            );
        }
    }
}

#[test]
fn armour_with_an_unsupported_owner_condition_keeps_the_startup_definition() {
    let item = "minecraft:iron_chestplate";
    let (mut runtime, base_pages) = startup_runtime(owner_armour_pack(item, false));
    let body = player_body(&mut runtime);
    let mut files = owner_armour_pack(item, true);
    let mut definition: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    definition["minecraft:attachable"]["description"]["item"][item] =
        "query.owner_identifier ==".into();
    files[0].1 = serde_json::to_vec(&definition).unwrap();
    let (entities, catalog) = pack_catalog(files);
    let (pages, locations) =
        base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
    let drawn = layers(&mut runtime, &body, &owner(), &worn_armour(item, 1), 1);
    assert_eq!(drawn.len(), 1);
    assert_eq!(selected_pixel(&pages, &drawn[0]), PLAIN);
}

#[test]
fn armour_stand_chestplates_and_leggings_keep_their_authored_bones() {
    for (item, slot) in [
        ("minecraft:iron_chestplate", 1),
        ("minecraft:iron_leggings", 2),
    ] {
        let mut files = owner_armour_pack(item, false);
        files.extend(
            owner_armour_pack(item, true)
                .into_iter()
                .filter(|(path, _)| !path.starts_with("render_controllers/")),
        );
        let (mut runtime, pages) = pack_runtime(files);
        let body = player_body(&mut runtime);
        let mut stand = owner();
        stand.kind = protocol::ActorKind::Entity {
            identifier: "minecraft:armor_stand".into(),
        };
        let drawn = layers(&mut runtime, &body, &stand, &worn_armour(item, slot), 1);
        assert_eq!(drawn.len(), 1);
        assert_eq!(selected_pixel(&pages, &drawn[0]), PAINTED);
        assert!(
            drawn[0]
                .submission
                .input
                .current_bones
                .iter()
                .any(|bone| bone.translation_scale[3] > 0.0),
            "{item} must have a visible armour shell"
        );
    }
}

#[test]
fn native_external_armour_keeps_its_startup_mesh_for_stands() {
    let item = "minecraft:iron_chestplate";
    let mut files = owner_armour_pack(item, false);
    let mut ordinary: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    let mut player = ordinary.clone();
    ordinary["minecraft:attachable"]["description"]["geometry"]["default"] =
        "geometry.humanoid.armor.chestplate".into();
    files[0].1 = serde_json::to_vec(&ordinary).unwrap();
    player["minecraft:attachable"]["description"]["identifier"] =
        "minecraft:iron_chestplate.player".into();
    player["minecraft:attachable"]["description"]["item"] =
        serde_json::json!({(item): "query.owner_identifier == 'minecraft:player'"});
    player["minecraft:attachable"]["description"]["scripts"]["pre_animation"] =
        serde_json::json!(["v.coat=1;"]);
    files.push((
        "attachables/player_armour.json".into(),
        serde_json::to_vec(&player).unwrap(),
    ));
    let (mut runtime, pages) = startup_runtime(files);
    let body = player_body(&mut runtime);
    let mut stand = owner();
    stand.kind = protocol::ActorKind::Entity {
        identifier: "minecraft:armor_stand".into(),
    };
    let drawn = layers(&mut runtime, &body, &stand, &worn_armour(item, 1), 1);
    assert_eq!(
        drawn.len(),
        1,
        "native geometry keeps its static startup fallback"
    );
    assert_eq!(selected_pixel(&pages, &drawn[0]), PLAIN);
    assert!(
        drawn[0]
            .submission
            .input
            .current_bones
            .iter()
            .any(|bone| { bone.translation_scale[3] > 0.0 })
    );
}

#[test]
fn armour_controllers_select_geometry_and_hide_only_their_own_bones() {
    let mut files = coat_pack("v.coat=0;");
    let mut model: serde_json::Value = serde_json::from_slice(&files[1].1).unwrap();
    let mut alternate = model["minecraft:geometry"][0].clone();
    alternate["description"]["identifier"] = "geometry.test.alternate".into();
    alternate["bones"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name":"extra_shell", "parent":"head", "pivot":[0,24,0],
            "cubes":[{"origin":[-5,24,-5],"size":[10,10,10],"uv":[0,0]}]
        }));
    model["minecraft:geometry"]
        .as_array_mut()
        .unwrap()
        .push(alternate);
    files[1].1 = serde_json::to_vec(&model).unwrap();
    let mut definition: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    definition["minecraft:attachable"]["description"]["geometry"]["alternate"] =
        "geometry.test.alternate".into();
    files[0].1 = serde_json::to_vec(&definition).unwrap();
    let mut controller: serde_json::Value = serde_json::from_slice(&files[2].1).unwrap();
    controller["render_controllers"]["controller.render.test_coat"]["geometry"] =
        "Geometry.alternate".into();
    controller["render_controllers"]["controller.render.test_coat"]["part_visibility"] =
        serde_json::json!([{"extra_shell":"0"}]);
    files[2].1 = serde_json::to_vec(&controller).unwrap();
    let (mut runtime, _) = pack_runtime(files);
    let body = player_body(&mut runtime);
    let drawn = layers(&mut runtime, &body, &owner(), &helmet(), 1);
    assert_eq!(drawn.len(), 1);
    let poses = &drawn[0].submission.input.current_bones;
    assert_eq!(poses.len(), 2);
    assert!(poses[0].translation_scale[3] > 0.0);
    assert_eq!(poses[1].translation_scale[3], 0.0);
    assert!(
        runtime
            .take_pending_geometries()
            .iter()
            .any(|geometry| geometry.id == drawn[0].submission.input.rig)
    );
}

/// Encodes an original solid-colour texture for material selection tests.
fn texture(rgba: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(16, 16, image::Rgba(rgba));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// A helmet that copies an owner flag into a variable in `pre_animation`, which its render
/// controller uses to pick between two coats.
fn coat_pack(script: &str) -> Vec<(Box<str>, Vec<u8>)> {
    vec![
        ("attachables/coat_helmet.json".into(), serde_json::to_vec(&serde_json::json!({
            "format_version":"1.10.0","minecraft:attachable":{"description":{
                "identifier":"minecraft:iron_helmet","materials":{"default":"armor"},
                "textures":{"default":"textures/models/coat_plain","plain":"textures/models/coat_plain",
                    "painted":"textures/models/coat_painted"},
                "geometry":{"default":"geometry.test.coat_helmet"},
                "scripts":{"initialize":["v.coat=0;"],
                    "pre_animation":[script]},
                "render_controllers":["controller.render.test_coat"]
            }}
        })).unwrap()),
        ("models/entity/coat_helmet.json".into(), serde_json::to_vec(&serde_json::json!({
            "format_version":"1.12.0","minecraft:geometry":[{
                "description":{"identifier":"geometry.test.coat_helmet","texture_width":16,"texture_height":16},
                "bones":[{"name":"head","pivot":[0,24,0],
                    "cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}]
            }]
        })).unwrap()),
        ("render_controllers/coat.json".into(), br#"{"format_version":"1.8.0","render_controllers":{
            "controller.render.test_coat":{"arrays":{"textures":{
                "Array.coat":["Texture.plain","Texture.painted"]}},
                "geometry":"Geometry.default","materials":[{"*":"Material.default"}],
                "textures":["Array.coat[v.coat]"]}
        }}"#.to_vec()),
        ("textures/models/coat_plain.png".into(), texture(PLAIN)),
        ("textures/models/coat_painted.png".into(), texture(PAINTED)),
    ]
}

/// Equips the fixture helmet in its authored armour slot.
fn helmet() -> ActorEquipmentInput {
    ActorEquipmentInput {
        armor: [
            Some(WornItem {
                variant: Default::default(),
                map_id: None,
                charged_projectile: None,
                use_facts: client_world::EffectiveItemUse::default(),
                grip: assets::item::grip::builtin_item_grip("minecraft:iron_helmet"),
                identifier: Arc::from("minecraft:iron_helmet"),
                metadata: 0,
                damage: None,
                kind: HeldKind::Other,
                dye_rgb: None,
                enchanted: false,
                glint: Default::default(),
            }),
            None,
            None,
            None,
        ],
        ..Default::default()
    }
}

#[test]
fn worn_pack_armour_follows_the_owner_driven_render_controller_texture() {
    let (mut runtime, pages) = pack_runtime(coat_pack("v.coat=c.owning_entity->q.is_charged;"));
    let body = player_body(&mut runtime);
    let mut owner = owner();
    let input = helmet();
    let plain = layers(&mut runtime, &body, &owner, &input, 1);
    assert_eq!(plain.len(), 1);
    assert_eq!(selected_pixel(&pages, &plain[0]), PLAIN);
    owner
        .metadata
        .insert(0, ActorMetadataValue::Flags(1 << CHARGED_FLAG));
    let painted = layers(&mut runtime, &body, &owner, &input, 2);
    assert_eq!(painted.len(), 1);
    assert_eq!(selected_pixel(&pages, &painted[0]), PAINTED);
}

#[test]
fn armour_uses_a_controller_alternative_before_startup_fallback() {
    let (mut runtime, base_pages) = startup_runtime(coat_pack("v.coat=0;"));
    let body = player_body(&mut runtime);
    let owner = owner();
    let input = helmet();
    let mut files = coat_pack("v.coat=1;");
    files.retain(|(path, _)| path.as_ref() != "textures/models/coat_plain.png");
    let (_, controller) = files
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "render_controllers/coat.json")
        .unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(controller).unwrap();
    let controller = &mut document["render_controllers"]["controller.render.test_coat"];
    controller.as_object_mut().unwrap().remove("arrays");
    controller["textures"] = serde_json::json!(["Texture.painted"]);
    let (_, bytes) = files
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "render_controllers/coat.json")
        .unwrap();
    *bytes = serde_json::to_vec(&document).unwrap();
    let (entities, catalog) = pack_catalog(files);
    let binding = catalog.binding("minecraft:iron_helmet").unwrap();
    assert!(catalog.texture(&binding.texture.identifier).is_none());
    let (pages, locations) =
        base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
    let drawn = layers(&mut runtime, &body, &owner, &input, 1);
    assert_eq!(drawn.len(), 1);
    assert_eq!(selected_pixel(&pages, &drawn[0]), PAINTED);
}

#[test]
fn armour_uses_the_pack_default_when_it_is_available() {
    let (mut runtime, base_pages) = startup_runtime(coat_pack("v.coat=0;"));
    let body = player_body(&mut runtime);
    let owner = owner();
    let input = helmet();
    let mut files = coat_pack("v.coat=0;");
    let (_, bytes) = files
        .iter_mut()
        .find(|(path, _)| path.as_ref() == "textures/models/coat_plain.png")
        .unwrap();
    *bytes = texture(PAINTED);
    let (entities, catalog) = pack_catalog(files);
    let (pages, locations) =
        base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
    let drawn = layers(&mut runtime, &body, &owner, &input, 1);
    assert_eq!(drawn.len(), 1);
    assert_eq!(selected_pixel(&pages, &drawn[0]), PAINTED);
}

#[test]
fn worn_pack_armour_observes_owner_item_use() {
    let (mut runtime, pages) = pack_runtime(coat_pack("v.coat=c.owning_entity->q.is_using_item;"));
    let body = player_body(&mut runtime);
    let mut owner = owner();
    let input = helmet();
    let idle = layers(&mut runtime, &body, &owner, &input, 1);
    assert_eq!(selected_pixel(&pages, &idle[0]), PLAIN);
    owner.metadata.insert(0, ActorMetadataValue::Flags(1 << 4));
    assert!(owner.is_using_item());
    let using = layers(&mut runtime, &body, &owner, &input, 2);
    assert_eq!(selected_pixel(&pages, &using[0]), PAINTED);
}

#[test]
fn worn_pack_armour_receives_the_render_delta() {
    for (delta_seconds, expected) in [(0.01, PAINTED), (0.035, PLAIN)] {
        let (mut runtime, pages) = pack_runtime(coat_pack("v.coat=q.delta_time < 0.02;"));
        let body = player_body(&mut runtime);
        let owner = owner();
        let names = [Box::<str>::from("body")];
        let rig = owner_rig(&owner, &names, 1);
        let input = helmet();
        let layer = runtime
            .layers_for(
                &body,
                &input,
                Some(EquipmentAnimation {
                    owner: &owner,
                    rig: &rig,
                    frame_alpha: 1.0,
                    delta_seconds,
                }),
            )
            .to_vec();
        assert_eq!(selected_pixel(&pages, &layer[0]), expected);
    }
}

#[test]
fn worn_pack_armour_receives_its_native_slot_context() {
    let (mut runtime, pages) = pack_runtime(coat_pack("v.coat=c.item_slot == 'head';"));
    let body = player_body(&mut runtime);
    let owner = owner();
    let input = helmet();
    let layer = layers(&mut runtime, &body, &owner, &input, 1);
    assert_eq!(selected_pixel(&pages, &layer[0]), PAINTED);
}

#[test]
fn worn_elytra_receives_the_attachable_render_delta() {
    for (delta_seconds, expected) in [(0.01, PAINTED), (0.035, PLAIN)] {
        let mut pack = coat_pack("v.coat=q.delta_time < 0.02;");
        let (_, bytes) = &mut pack[0];
        let mut attachable: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        attachable["minecraft:attachable"]["description"]["identifier"] = "minecraft:elytra".into();
        *bytes = serde_json::to_vec(&attachable).unwrap();
        let (mut runtime, pages) = pack_runtime(pack);
        let body = player_body(&mut runtime);
        let owner = owner();
        let names = [Box::<str>::from("body")];
        let rig = owner_rig(&owner, &names, 1);
        let mut input = helmet();
        let mut wings = input.armor[0].take().unwrap();
        wings.identifier = Arc::from("minecraft:elytra");
        input.armor[1] = Some(wings);
        let layer = runtime
            .layers_for(
                &body,
                &input,
                Some(EquipmentAnimation {
                    owner: &owner,
                    rig: &rig,
                    frame_alpha: 1.0,
                    delta_seconds,
                }),
            )
            .to_vec();
        assert_eq!(selected_pixel(&pages, &layer[0]), expected);
    }
}

#[path = "worn_texture_tests/review_tests.rs"]
mod review_tests;

#[path = "worn_texture_tests/artwork_state_tests.rs"]
mod artwork_state_tests;

//! Worn pack armour draws the texture its render controller selects from owner state.

use super::{
    elytra_tests::{layers, owner, owner_rig, selected_pixel},
    pack_runtime, player_body,
};
use crate::presentation::equipment::runtime::{
    ActorEquipmentInput, EquipmentAnimation, HeldKind, WornItem,
};
use protocol::ActorMetadataValue;
use std::sync::Arc;

const PLAIN: [u8; 4] = [170, 170, 170, 255];
const PAINTED: [u8; 4] = [30, 150, 70, 255];
/// Actor flag bit read by `query.is_charged`.
const CHARGED_FLAG: u32 = 27;

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
                "identifier":"test:coat_helmet","materials":{"default":"armor"},
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
                identifier: Arc::from("test:coat_helmet"),
                metadata: 0,
                damage: None,
                kind: HeldKind::Other,
                dye_rgb: None,
                enchanted: false,
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

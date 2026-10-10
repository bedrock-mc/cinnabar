//! Worn pack armour draws the texture its render controller selects from owner state.

use super::{
    elytra_tests::{layers, owner, selected_pixel},
    pack_runtime, player_body,
};
use crate::presentation::equipment::runtime::{ActorEquipmentInput, HeldKind, WornItem};
use protocol::ActorMetadataValue;
use std::sync::Arc;

const PLAIN: [u8; 4] = [170, 170, 170, 255];
const PAINTED: [u8; 4] = [30, 150, 70, 255];
/// Actor flag bit read by `query.is_charged`.
const CHARGED_FLAG: u32 = 27;

fn texture(rgba: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(16, 16, image::Rgba(rgba));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// A helmet that copies an owner flag into a variable in `pre_animation`, which its render
/// controller uses to pick between two coats.
fn coat_pack() -> Vec<(Box<str>, Vec<u8>)> {
    vec![
        ("attachables/coat_helmet.json".into(), serde_json::to_vec(&serde_json::json!({
            "format_version":"1.10.0","minecraft:attachable":{"description":{
                "identifier":"test:coat_helmet","materials":{"default":"armor"},
                "textures":{"default":"textures/models/coat_plain","plain":"textures/models/coat_plain",
                    "painted":"textures/models/coat_painted"},
                "geometry":{"default":"geometry.test.coat_helmet"},
                "scripts":{"initialize":["v.coat=0;"],
                    "pre_animation":["v.coat=c.owning_entity->q.is_charged;"]},
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
    let (mut runtime, pages) = pack_runtime(coat_pack());
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

//! Worn pack armour draws the texture its render controller selects from owner state.

use super::{
    elytra_tests::{layers, owner, selected_pixel},
    pack_runtime, player_body,
};
use crate::presentation::equipment::runtime::{ActorEquipmentInput, HeldKind, WornItem};
use protocol::ActorMetadataValue;
use std::sync::Arc;

const BLUE: [u8; 4] = [40, 40, 200, 255];
const RED: [u8; 4] = [200, 40, 40, 255];

fn texture(rgba: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(16, 16, image::Rgba(rgba));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// A helmet whose render controller indexes a team texture array by a variable the
/// attachable reads from its owner's powered flag.
fn team_pack() -> Vec<(Box<str>, Vec<u8>)> {
    vec![
        ("attachables/team_helmet.json".into(), serde_json::to_vec(&serde_json::json!({
            "format_version":"1.10.0","minecraft:attachable":{"description":{
                "identifier":"test:team_helmet","materials":{"default":"armor"},
                "textures":{"default":"textures/models/team_blue","blue":"textures/models/team_blue",
                    "red":"textures/models/team_red"},
                "geometry":{"default":"geometry.test.team_helmet"},
                "scripts":{"initialize":["v.team=0;"],
                    "pre_animation":["v.team=c.owning_entity->q.is_powered;"]},
                "render_controllers":["controller.render.test_team"]
            }}
        })).unwrap()),
        ("models/entity/team_helmet.json".into(), serde_json::to_vec(&serde_json::json!({
            "format_version":"1.12.0","minecraft:geometry":[{
                "description":{"identifier":"geometry.test.team_helmet","texture_width":16,"texture_height":16},
                "bones":[{"name":"head","pivot":[0,24,0],
                    "cubes":[{"origin":[-4,24,-4],"size":[8,8,8],"uv":[0,0]}]}]
            }]
        })).unwrap()),
        ("render_controllers/team.json".into(), br#"{"format_version":"1.8.0","render_controllers":{
            "controller.render.test_team":{"arrays":{"textures":{
                "Array.team":["Texture.blue","Texture.red"]}},
                "geometry":"Geometry.default","materials":[{"*":"Material.default"}],
                "textures":["Array.team[v.team]"]}
        }}"#.to_vec()),
        ("textures/models/team_blue.png".into(), texture(BLUE)),
        ("textures/models/team_red.png".into(), texture(RED)),
    ]
}

fn helmet() -> ActorEquipmentInput {
    ActorEquipmentInput {
        armor: [
            Some(WornItem {
                identifier: Arc::from("test:team_helmet"),
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
    let (mut runtime, pages) = pack_runtime(team_pack());
    let body = player_body(&mut runtime);
    let mut owner = owner();
    let input = helmet();
    let blue = layers(&mut runtime, &body, &owner, &input, 1);
    assert_eq!(blue.len(), 1);
    assert_eq!(selected_pixel(&pages, &blue[0]), BLUE);
    owner.metadata.insert(0, ActorMetadataValue::Flags(1 << 9));
    let red = layers(&mut runtime, &body, &owner, &input, 2);
    assert_eq!(red.len(), 1);
    assert_eq!(selected_pixel(&pages, &red[0]), RED);
}

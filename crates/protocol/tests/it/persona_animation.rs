use protocol::{ActorEvent, PlayerListEntry, PlayerSkin, WorldEvent, into_world_event};
use valentine::bedrock::version::v1_26_51::{
    AnimatedImageData, EnumspersonaAnimatedTextureType, EnumspersonaAnimationExpression,
    PlayerListPacket, PlayerListPacketEntriesItem, PlayerListPacketPayloadAddEntry,
    SerializedSkinRef, SkinImage,
};

#[test]
fn persona_animation_raster_survives_player_list_normalization() {
    let patch = r#"{"geometry":{"default":"geometry.fixture","animated_face":"geometry.face"}}"#;
    let geometry = r#"{"format_version":"1.14.0","minecraft:geometry":[]}"#;
    let animation_bytes = 32 * 64 * 4;
    let packet = PlayerListPacket {
        entries: vec![PlayerListPacketEntriesItem::AddEntry(Box::new(
            PlayerListPacketPayloadAddEntry {
                serialized_skin: SerializedSkinRef {
                    is_persona: true,
                    resource_patch: patch.into(),
                    geometry_data: geometry.into(),
                    image_data: SkinImage {
                        width: 256,
                        height: 256,
                        image_bytes: vec![255; 256 * 256 * 4],
                    },
                    animated_image_data: vec![AnimatedImageData {
                        skin_image: SkinImage {
                            width: 32,
                            height: 64,
                            image_bytes: vec![255; animation_bytes],
                        },
                        animated_texture_type: EnumspersonaAnimatedTextureType::Face,
                        frames: 2.0,
                        animation_expression: EnumspersonaAnimationExpression::Blinking,
                    }],
                    ..Default::default()
                },
                ..Default::default()
            },
        ))],
    };
    let Some(WorldEvent::Actor(ActorEvent::PlayerList(update))) =
        into_world_event(packet.into(), 0).unwrap()
    else {
        panic!("player list");
    };
    let PlayerListEntry::Add {
        skin: PlayerSkin::Standard(skin),
        ..
    } = &update.entries[0]
    else {
        panic!("persona skin");
    };
    assert!(
        skin.geometry.as_ref().unwrap().byte_len()
            >= patch.len() + geometry.len() + animation_bytes,
        "the separate animated face image must survive with the geometry source"
    );
}

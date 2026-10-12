use bedrock_protocol_1_26_51::enums::{PersonaAnimatedTextureType, PersonaAnimationExpression};
use bedrock_protocol_1_26_51::packets::PlayerList as PlayerListPacket;
use bedrock_protocol_1_26_51::types::{
    ActorUniqueID, AnimatedImageData, PlayerListData, SerializedSkinRef, SkinImage,
};
use bedrock_protocol_1_26_51::wire;
use protocol::{ActorEvent, PlayerListEntry, PlayerSkin, WorldEvent, into_world_event};

#[test]
fn persona_animation_raster_survives_player_list_normalization() {
    let patch = r#"{"geometry":{"default":"geometry.fixture","animated_face":"geometry.face"}}"#;
    let geometry = r#"{"format_version":"1.14.0","minecraft:geometry":[]}"#;
    let animation_bytes = 32 * 64 * 4;
    let packet = PlayerListPacket {
        entries: vec![PlayerListData::AddEntry {
            action: Default::default(),
            uuid: Default::default(),
            actor_unique_id: ActorUniqueID(0),
            player_name: Default::default(),
            xbl_xuid: Default::default(),
            platform_online_id: Default::default(),
            build_platform: Default::default(),
            serialized_skin: Box::new(SerializedSkinRef {
                is_persona: true,
                resource_patch: patch.into(),
                geometry_data: geometry.into(),
                image_data: SkinImage {
                    width: wire::U32LE(256),
                    height: wire::U32LE(256),
                    image_bytes: vec![255; 256 * 256 * 4].into(),
                },
                animated_image_data: vec![AnimatedImageData {
                    skin_image: SkinImage {
                        width: wire::U32LE(32),
                        height: wire::U32LE(64),
                        image_bytes: vec![255; animation_bytes].into(),
                    },
                    animated_texture_type: PersonaAnimatedTextureType::Face,
                    frames: wire::F32LE(2.0),
                    animation_expression: PersonaAnimationExpression::Blinking,
                }],
                ..Default::default()
            }),
            is_teacher: false,
            is_host: false,
            is_sub_client: false,
            player_color: Default::default(),
        }],
    };
    let Some(WorldEvent::Actor(ActorEvent::PlayerList(update))) =
        into_world_event(protocol::Packet::new(packet), 0).unwrap()
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

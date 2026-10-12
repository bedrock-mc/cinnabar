use bedrock_protocol_1_26_51::packets::PlayerSkin as PlayerSkinPacket;
use bedrock_protocol_1_26_51::types::{SerializedSkinRef, SkinImage};
use bedrock_protocol_1_26_51::wire;
use protocol::{ActorEvent, PlayerSkin, WorldEvent, into_world_event};
use render_api::CLASSIC_SKIN_SIDE;

#[test]
fn player_skin_packet_reaches_the_actor_pipeline() {
    let packet = PlayerSkinPacket {
        uuid: uuid::Uuid::from_bytes([7; 16]),
        serialized_skin: SerializedSkinRef {
            image_data: SkinImage {
                width: wire::U32LE(CLASSIC_SKIN_SIDE as u32),
                height: wire::U32LE(CLASSIC_SKIN_SIDE as u32),
                image_bytes: vec![255; CLASSIC_SKIN_SIDE * CLASSIC_SKIN_SIDE * 4].into(),
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let Some(WorldEvent::Actor(ActorEvent::Skin { uuid, skin })) =
        into_world_event(protocol::Packet::new(packet), 0).unwrap()
    else {
        panic!("a skin update must reach the actor pipeline");
    };
    assert_eq!(uuid, [7; 16]);
    assert!(
        matches!(skin, PlayerSkin::Standard(skin) if skin.rgba8.iter().all(|pixel| *pixel == 255))
    );
}

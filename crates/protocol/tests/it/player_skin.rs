use protocol::{ActorEvent, CLASSIC_SKIN_SIDE, PlayerSkin, WorldEvent, into_world_event};
use valentine::bedrock::version::v1_26_51::{PlayerSkinPacket, SerializedSkinRef, SkinImage};

#[test]
fn player_skin_packet_reaches_the_actor_pipeline() {
    let packet = PlayerSkinPacket {
        uuid: uuid::Uuid::from_bytes([7; 16]),
        serialized_skin: SerializedSkinRef {
            image_data: SkinImage {
                width: CLASSIC_SKIN_SIDE as u32,
                height: CLASSIC_SKIN_SIDE as u32,
                image_bytes: vec![255; CLASSIC_SKIN_SIDE * CLASSIC_SKIN_SIDE * 4],
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let Some(WorldEvent::Actor(ActorEvent::Skin { uuid, skin })) =
        into_world_event(packet.into(), 0).unwrap()
    else {
        panic!("a skin update must reach the actor pipeline");
    };
    assert_eq!(uuid, [7; 16]);
    assert!(
        matches!(skin, PlayerSkin::Standard(skin) if skin.rgba8.iter().all(|pixel| *pixel == 255))
    );
}

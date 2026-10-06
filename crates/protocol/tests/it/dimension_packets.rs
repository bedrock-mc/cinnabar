use protocol::{
    BedrockSession, LoadingScreenPhase, WorldEvent, dimension_change_done_packet, into_world_event,
    loading_screen_packet,
};
use valentine::bedrock::version::v1_26_51::{
    BlockPos, ChangeDimensionPacket, DimensionType, EnumsPlayerActionType,
    EnumsServerboundLoadingScreenPacketType, McpePacketData, Vec3,
};

#[test]
fn dimension_change_retains_respawn_and_optional_loading_identifier() {
    for respawn in [false, true] {
        for loading_screen_id in [None, Some(0), Some(u32::MAX)] {
            let packet = ChangeDimensionPacket {
                dimension_id: DimensionType { value: 1 },
                position: Vec3 {
                    x: 0.0,
                    y: 4_000.0,
                    z: -0.5,
                },
                respawn,
                loading_screen_id,
            };
            let Some(WorldEvent::ChangeDimension(event)) =
                into_world_event(packet.into(), 0).unwrap()
            else {
                panic!("dimension transfer event");
            };
            assert_eq!(event.dimension, 1);
            assert_eq!(event.position, [0.0, 4_000.0, -0.5]);
            assert_eq!(event.respawn, respawn);
            assert_eq!(event.loading_screen_id, loading_screen_id);
        }
    }
}

#[test]
fn dimension_completion_packet_uses_action_fourteen_and_empty_block_coordinates() {
    let packet = dimension_change_done_packet(42);
    let session = BedrockSession { shield_item_id: 0 };
    let encoded = protocol::encode(&packet, &session).unwrap();
    let decoded = protocol::decode_batch(encoded, &session).unwrap();
    let McpePacketData::PlayerActionPacket(action) = &decoded[0].data else {
        panic!("player action packet");
    };
    assert_eq!(action.player_runtime_id.actor_runtime_id, 42);
    assert_eq!(action.action, EnumsPlayerActionType::Changedimensionack);
    assert_eq!(action.block_position, BlockPos::default());
    assert_eq!(action.result_pos, BlockPos::default());
    assert_eq!(action.face, 0);
}

#[test]
fn dimension_loading_pair_preserves_absent_zero_and_full_width_identifiers() {
    let session = BedrockSession { shield_item_id: 0 };
    for loading_screen_id in [None, Some(0), Some(u32::MAX)] {
        for phase in [LoadingScreenPhase::Start, LoadingScreenPhase::End] {
            let packet = loading_screen_packet(phase, loading_screen_id);
            let encoded = protocol::encode(&packet, &session).unwrap();
            let decoded = protocol::decode_batch(encoded, &session).unwrap();
            let McpePacketData::ServerboundLoadingScreenPacket(loading) = &decoded[0].data else {
                panic!("loading screen packet");
            };
            assert_eq!(loading.loading_screen_id, loading_screen_id);
            assert_eq!(
                loading.loading_screen_packet_type,
                if phase == LoadingScreenPhase::Start {
                    EnumsServerboundLoadingScreenPacketType::Startloadingscreen
                } else {
                    EnumsServerboundLoadingScreenPacketType::Endloadingscreen
                }
            );
        }
    }
}

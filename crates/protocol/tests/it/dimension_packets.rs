use bedrock_protocol_1_26_51::enums::{PlayerActionType, ServerboundLoadingScreenType};
use bedrock_protocol_1_26_51::packets::{self, ChangeDimension};
use bedrock_protocol_1_26_51::types::{BlockPos, DimensionType};
use bedrock_protocol_1_26_51::wire;
use protocol::{
    LoadingScreenPhase, WorldEvent, dimension_change_done_packet, into_world_event,
    loading_screen_packet,
};

#[test]
fn dimension_change_retains_respawn_and_optional_loading_identifier() {
    for respawn in [false, true] {
        for loading_screen_id in [None, Some(0), Some(u32::MAX)] {
            let packet = ChangeDimension {
                dimension_id: DimensionType(1),
                position: [0.0, 4_000.0, -0.5].into(),
                respawn,
                loading_screen_id: loading_screen_id.map(wire::U32LE),
            };
            let Some(WorldEvent::ChangeDimension(event)) =
                into_world_event(protocol::Packet::new(packet), 0).unwrap()
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
    let encoded = protocol::encode(&packet).unwrap();
    let decoded = protocol::decode_batch(encoded).unwrap();
    let packets::Packet::PlayerAction(action) = &*decoded[0].data else {
        panic!("player action packet");
    };
    assert_eq!(action.player_runtime_id.0, 42);
    assert_eq!(action.action, PlayerActionType::Changedimensionack);
    assert_eq!(action.block_position, BlockPos::default());
    assert_eq!(action.result_pos, BlockPos::default());
    assert_eq!(action.face.0, 0);
}

#[test]
fn dimension_loading_pair_preserves_absent_zero_and_full_width_identifiers() {
    for loading_screen_id in [None, Some(0), Some(u32::MAX)] {
        for phase in [LoadingScreenPhase::Start, LoadingScreenPhase::End] {
            let packet = loading_screen_packet(phase, loading_screen_id);
            let encoded = protocol::encode(&packet).unwrap();
            let decoded = protocol::decode_batch(encoded).unwrap();
            let packets::Packet::ServerboundLoadingScreen(loading) = &*decoded[0].data else {
                panic!("loading screen packet");
            };
            assert_eq!(loading.loading_screen_id.map(|id| id.0), loading_screen_id);
            assert_eq!(
                loading.loading_screen_packet_type,
                if phase == LoadingScreenPhase::Start {
                    ServerboundLoadingScreenType::Startloadingscreen
                } else {
                    ServerboundLoadingScreenType::Endloadingscreen
                }
            );
        }
    }
}

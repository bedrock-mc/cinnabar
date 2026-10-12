use super::*;
use crate::raw_packet::decode_packet_raw;
use bedrock_protocol_1_26_51::packets::UpdateBlockSynced;
use bedrock_protocol_1_26_51::types::BlockPos;
use bytes::Buf;

#[test]
fn synced_block_update_is_admitted_from_the_raw_world_lane() {
    let packet: Packet = crate::Packet::new(UpdateBlockSynced {
        block_position: BlockPos {
            x: wire::ZigZag32(17),
            y: wire::ZigZag32(64),
            z: wire::ZigZag32(-3),
        },
        block_runtime_id: wire::VarUInt(crate::SEQUENTIAL_AIR_NETWORK_ID),
        flags: wire::VarUInt(3),
        layer: wire::VarUInt(0),
        unique_actor_id: wire::VarULong(u64::MAX - 1),
        actor_sync_message: wire::VarULong(1),
    });
    let mut batch = crate::encode(&packet).unwrap();
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).unwrap();
    assert!(
        decode_world_raw_with(raw, 2, super::decode_test_raw)
            .unwrap()
            .is_some()
    );
}

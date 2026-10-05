use super::*;
use jolyne::raw::decode_packet_raw;
use valentine::bedrock::{context::BedrockSession, version::v1_26_51::BlockPos};

#[test]
fn synced_block_update_is_admitted_from_the_raw_world_lane() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = valentine::bedrock::version::v1_26_51::UpdateBlockSyncedPacket {
        block_position: BlockPos {
            x: 17,
            y: 64,
            z: -3,
        },
        block_runtime_id: crate::SEQUENTIAL_AIR_NETWORK_ID,
        flags: 3,
        layer: 0,
        unique_actor_id: u64::MAX - 1,
        actor_sync_message: 1,
    }
    .into();
    let mut batch = crate::encode(&packet, &session).unwrap();
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).unwrap();
    assert!(
        decode_world_raw_with(raw, 2, |raw| raw.decode(&session))
            .unwrap()
            .is_some()
    );
}

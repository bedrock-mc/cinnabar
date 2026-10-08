use super::*;
use protocol::{ActorBlockSyncMessage, BlockUpdateEvent, SyncedBlockUpdateEvent};
use valentine::bedrock::version::v1_26_51::UpdateBlockSyncedPacket;

#[test]
fn synced_falling_block_source_update_is_admitted_as_world_data() {
    let packet = UpdateBlockSyncedPacket {
        block_position: BlockPos {
            x: -17,
            y: 64,
            z: 3,
        },
        block_runtime_id: SEQUENTIAL_AIR_NETWORK_ID,
        flags: 2,
        layer: 0,
        unique_actor_id: u64::MAX - 1,
        actor_sync_message: 1,
    };
    let mut wire = BytesMut::new();
    packet.encode(&mut wire).unwrap();
    let mut wire = wire.freeze();
    let decoded = UpdateBlockSyncedPacket::decode(&mut wire, ()).unwrap();
    assert!(!wire.has_remaining());
    assert!(
        into_world_event(decoded.into(), 2).unwrap().is_some(),
        "the block-to-entity source update must reach the ordered world owner"
    );
}

#[test]
fn synced_block_update_preserves_flags_and_signed_actor_identity() {
    let packet = UpdateBlockSyncedPacket {
        block_position: BlockPos { x: 2, y: 64, z: -3 },
        block_runtime_id: 55,
        flags: 0x102,
        layer: 0,
        unique_actor_id: u64::MAX - 1,
        actor_sync_message: 2,
    };
    assert_eq!(
        into_world_event(packet.into(), 2).unwrap(),
        Some(WorldEvent::SyncedBlockUpdates(vec![
            SyncedBlockUpdateEvent {
                update: BlockUpdateEvent {
                    dimension: 2,
                    position: [2, 64, -3],
                    layer: 0,
                    network_id: 55,
                },
                flags: 0x102,
                sync: ActorBlockSyncMessage {
                    actor_unique_id: -2,
                    message: 2
                },
            }
        ]))
    );
}

#[test]
fn batched_block_update_preserves_each_actor_transition_and_layer() {
    let entry = |runtime_id, flags, unique_id, message| UpdateSubChunkNetworkBlockInfo {
        pos: BlockPos { x: 4, y: 65, z: 6 },
        runtime_id,
        update_flags: flags,
        sync_message_entity_unique_id: unique_id,
        sync_message_message: message,
    };
    let packet = UpdateSubChunkBlocksPacket {
        sub_chunk_block_position: BlockPos { x: 0, y: 4, z: 0 },
        blocks_changed: UpdateSubChunkBlocksChangedInfo {
            blocks_changed_standards: vec![entry(57, 3, u64::MAX - 1, 1)],
            blocks_changed_extras: vec![entry(58, 2, 7, 99)],
        },
    };
    let Some(WorldEvent::SyncedBlockUpdates(updates)) = into_world_event(packet.into(), 2).unwrap()
    else {
        panic!("batched terrain updates must preserve their synchronization")
    };
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].update.layer, 0);
    assert_eq!(updates[0].flags, 3);
    assert_eq!(
        updates[0].sync,
        ActorBlockSyncMessage {
            actor_unique_id: -2,
            message: 1
        }
    );
    assert_eq!(updates[1].update.layer, 1);
    assert_eq!(updates[1].flags, 2);
    assert_eq!(
        updates[1].sync,
        ActorBlockSyncMessage {
            actor_unique_id: 7,
            message: 99
        }
    );
}

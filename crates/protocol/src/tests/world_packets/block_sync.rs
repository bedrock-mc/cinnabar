use super::*;
use bedrock_protocol_1_26_51::packets::UpdateBlockSynced;
use bedrock_protocol_1_26_51::wire::{Decode, Encode};
use protocol::{ActorBlockSyncMessage, BlockUpdateEvent, SyncedBlockUpdateEvent};

#[test]
fn synced_falling_block_source_update_is_admitted_as_world_data() {
    let packet = UpdateBlockSynced {
        block_position: block_pos(-17, 64, 3),
        block_runtime_id: wire::VarUInt(SEQUENTIAL_AIR_NETWORK_ID),
        flags: wire::VarUInt(2),
        layer: wire::VarUInt(0),
        unique_actor_id: wire::VarULong(u64::MAX - 1),
        actor_sync_message: wire::VarULong(1),
    };
    let encoded = packet.encode_to_vec().unwrap();
    // `decode_exact` rejects any byte the packet leaves unread.
    let decoded = UpdateBlockSynced::decode_exact(&encoded).unwrap();
    assert!(
        into_world_event(protocol::Packet::new(decoded), 2)
            .unwrap()
            .is_some(),
        "the block-to-entity source update must reach the ordered world owner"
    );
}

#[test]
fn synced_block_update_preserves_flags_and_signed_actor_identity() {
    let packet = UpdateBlockSynced {
        block_position: block_pos(2, 64, -3),
        block_runtime_id: wire::VarUInt(55),
        flags: wire::VarUInt(0x102),
        layer: wire::VarUInt(0),
        unique_actor_id: wire::VarULong(u64::MAX - 1),
        actor_sync_message: wire::VarULong(2),
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 2).unwrap(),
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
        pos: block_pos(4, 65, 6),
        runtime_id: wire::VarUInt(runtime_id),
        update_flags: wire::VarUInt(flags),
        sync_message_entity_unique_id: wire::VarULong(unique_id),
        sync_message_message: wire::VarUInt(message),
    };
    let packet = UpdateSubChunkBlocks {
        sub_chunk_block_position: block_pos(0, 4, 0),
        blocks_changed: UpdateSubChunkBlocksChangedInfo {
            blocks_changed_standards: vec![entry(57, 3, u64::MAX - 1, 1)],
            blocks_changed_extras: vec![entry(58, 2, 7, 99)],
        },
    };
    let Some(WorldEvent::SyncedBlockUpdates(updates)) =
        into_world_event(protocol::Packet::new(packet), 2).unwrap()
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

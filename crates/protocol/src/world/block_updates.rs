use valentine::bedrock::version::v1_26_51::McpePacketData;

use super::{
    ActorBlockSyncMessage, BlockUpdateEvent, SyncedBlockUpdateEvent, WorldEvent, WorldPacketError,
    requests::normalize_layer,
};

pub(super) fn normalize(
    packet: McpePacketData,
    current_dimension: i32,
) -> Result<Option<WorldEvent>, WorldPacketError> {
    let event = match packet {
        McpePacketData::UpdateBlockPacket(packet) => {
            let layer = normalize_layer(packet.layer)?;
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: current_dimension,
                position: [
                    packet.block_position.x,
                    packet.block_position.y,
                    packet.block_position.z,
                ],
                layer,
                network_id: packet.block_runtime_id,
            }])
        }
        McpePacketData::UpdateBlockSyncedPacket(packet) => {
            if packet.layer > 1 {
                return Ok(None);
            }
            let event = SyncedBlockUpdateEvent {
                update: BlockUpdateEvent {
                    dimension: current_dimension,
                    position: [
                        packet.block_position.x,
                        packet.block_position.y,
                        packet.block_position.z,
                    ],
                    layer: packet.layer as usize,
                    network_id: packet.block_runtime_id,
                },
                flags: packet.flags,
                sync: ActorBlockSyncMessage {
                    actor_unique_id: packet.unique_actor_id as i64,
                    message: packet.actor_sync_message,
                },
            };
            WorldEvent::SyncedBlockUpdates(vec![event])
        }
        McpePacketData::UpdateSubChunkBlocksPacket(packet) => {
            // The two block lists moved into a nested `blocks_changed` struct;
            // gophertunnel packet/update_sub_chunk_blocks.go still writes
            // Blocks (layer 0) then Extra (layer 1) back to back.
            let standards = packet.blocks_changed.blocks_changed_standards;
            let extras = packet.blocks_changed.blocks_changed_extras;
            let mut updates = Vec::with_capacity(standards.len() + extras.len());
            updates.extend(standards.into_iter().map(|update| SyncedBlockUpdateEvent {
                update: BlockUpdateEvent {
                    dimension: current_dimension,
                    position: [update.pos.x, update.pos.y, update.pos.z],
                    layer: 0,
                    network_id: update.runtime_id,
                },
                flags: update.update_flags,
                sync: ActorBlockSyncMessage {
                    actor_unique_id: update.sync_message_entity_unique_id as i64,
                    message: u64::from(update.sync_message_message),
                },
            }));
            updates.extend(extras.into_iter().map(|update| SyncedBlockUpdateEvent {
                update: BlockUpdateEvent {
                    dimension: current_dimension,
                    position: [update.pos.x, update.pos.y, update.pos.z],
                    layer: 1,
                    network_id: update.runtime_id,
                },
                flags: update.update_flags,
                sync: ActorBlockSyncMessage {
                    actor_unique_id: update.sync_message_entity_unique_id as i64,
                    message: u64::from(update.sync_message_message),
                },
            }));
            WorldEvent::SyncedBlockUpdates(updates)
        }
        _ => unreachable!("block normalization receives only block-update packets"),
    };
    Ok(Some(event))
}

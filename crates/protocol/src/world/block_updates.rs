use bedrock_protocol_1_26_51::packets::{UpdateBlock, UpdateBlockSynced, UpdateSubChunkBlocks};
use bedrock_protocol_1_26_51::types::{BlockPos, UpdateSubChunkNetworkBlockInfo};

use super::{
    ActorBlockSyncMessage, BlockUpdateEvent, SyncedBlockUpdateEvent, WorldEvent, WorldPacketError,
    requests::normalize_layer,
};

fn position(position: &BlockPos) -> [i32; 3] {
    [position.x.0, position.y.0, position.z.0]
}

pub(super) fn normalize_update(
    packet: UpdateBlock,
    current_dimension: i32,
) -> Result<WorldEvent, WorldPacketError> {
    let layer = normalize_layer(packet.layer.0)?;
    Ok(WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
        dimension: current_dimension,
        position: position(&packet.block_position),
        layer,
        network_id: packet.block_runtime_id.0,
    }]))
}

/// A synced update on a layer past the second is skipped.
pub(super) fn normalize_synced(
    packet: UpdateBlockSynced,
    current_dimension: i32,
) -> Option<WorldEvent> {
    if packet.layer.0 > 1 {
        return None;
    }
    let event = SyncedBlockUpdateEvent {
        update: BlockUpdateEvent {
            dimension: current_dimension,
            position: position(&packet.block_position),
            layer: packet.layer.0 as usize,
            network_id: packet.block_runtime_id.0,
        },
        flags: packet.flags.0,
        sync: ActorBlockSyncMessage {
            actor_unique_id: packet.unique_actor_id.0 as i64,
            message: packet.actor_sync_message.0,
        },
    };
    Some(WorldEvent::SyncedBlockUpdates(vec![event]))
}

/// Layer 0 (`blocks_changed_standards`) and layer 1 (`blocks_changed_extras`) in wire order.
pub(super) fn normalize_sub_chunk(
    packet: UpdateSubChunkBlocks,
    current_dimension: i32,
) -> WorldEvent {
    let standards = packet.blocks_changed.blocks_changed_standards;
    let extras = packet.blocks_changed.blocks_changed_extras;
    let synced = |layer: usize| {
        move |update: UpdateSubChunkNetworkBlockInfo| SyncedBlockUpdateEvent {
            update: BlockUpdateEvent {
                dimension: current_dimension,
                position: position(&update.pos),
                layer,
                network_id: update.runtime_id.0,
            },
            flags: update.update_flags.0,
            sync: ActorBlockSyncMessage {
                actor_unique_id: update.sync_message_entity_unique_id.0 as i64,
                message: u64::from(update.sync_message_message.0),
            },
        }
    };
    let mut updates = Vec::with_capacity(standards.len() + extras.len());
    updates.extend(standards.into_iter().map(synced(0)));
    updates.extend(extras.into_iter().map(synced(1)));
    WorldEvent::SyncedBlockUpdates(updates)
}

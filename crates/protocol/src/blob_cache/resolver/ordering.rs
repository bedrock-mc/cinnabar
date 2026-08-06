use super::*;

pub(super) fn pending_packet_columns(packet: &PendingPacket) -> Vec<ColumnKey> {
    match packet {
        PendingPacket::LevelChunk(packet) => vec![ColumnKey {
            dimension: dimension_id(&packet.dimension_id),
            x: packet.chunk_position.x,
            z: packet.chunk_position.z,
        }],
        PendingPacket::SubChunk(packet) => {
            if !packet.cache_enabled {
                return Vec::new();
            }
            stable_unique_columns(packet.sub_chunk_data.iter().map(|entry| {
                pending_sub_chunk_column(
                    packet,
                    entry.sub_chunk_pos_offset.subchunk_offset_x,
                    entry.sub_chunk_pos_offset.subchunk_offset_z,
                )
            }))
        }
    }
}

fn pending_sub_chunk_column(packet: &SubChunkPacket, dx: i8, dz: i8) -> ColumnKey {
    ColumnKey {
        dimension: dimension_id(&packet.dimension_type),
        x: packet
            .center_pos
            .subchunk_position_x
            .saturating_add(i32::from(dx)),
        z: packet
            .center_pos
            .subchunk_position_z
            .saturating_add(i32::from(dz)),
    }
}

pub(super) fn ready_value_columns(value: &BlobCacheReady) -> Vec<ColumnKey> {
    let BlobCacheReady::WorldEvent(event) = value else {
        return Vec::new();
    };
    match event {
        WorldEvent::LevelChunk(event) => vec![ColumnKey {
            dimension: event.dimension,
            x: event.x,
            z: event.z,
        }],
        WorldEvent::SubChunks(event) => {
            stable_unique_columns(event.entries.iter().map(|entry| ColumnKey {
                dimension: event.dimension,
                x: entry.position[0],
                z: entry.position[2],
            }))
        }
        WorldEvent::BlockUpdates(events) => {
            stable_unique_columns(events.iter().map(|event| ColumnKey {
                dimension: event.dimension,
                x: event.position[0].div_euclid(16),
                z: event.position[2].div_euclid(16),
            }))
        }
        WorldEvent::BlockEntityUpdate(event) => vec![ColumnKey {
            dimension: event.dimension,
            x: event.position[0].div_euclid(16),
            z: event.position[2].div_euclid(16),
        }],
        _ => Vec::new(),
    }
}

fn stable_unique_columns(columns: impl IntoIterator<Item = ColumnKey>) -> Vec<ColumnKey> {
    let mut seen = HashSet::new();
    columns
        .into_iter()
        .filter(|column| seen.insert(*column))
        .collect()
}

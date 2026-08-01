use super::*;

pub(super) fn pending_packet_columns(packet: &PendingPacket) -> Vec<ColumnKey> {
    match packet {
        PendingPacket::LevelChunk(packet) => vec![ColumnKey {
            dimension: packet.dimension,
            x: packet.x,
            z: packet.z,
        }],
        PendingPacket::SubChunk(packet) => {
            let SubchunkPacketEntries::SubChunkEntryWithCaching(entries) = &packet.entries else {
                return Vec::new();
            };
            stable_unique_columns(
                entries
                    .iter()
                    .map(|entry| pending_sub_chunk_column(packet, entry.dx, entry.dz)),
            )
        }
    }
}

pub(super) fn pending_packet_sub_chunks(packet: &PendingPacket) -> Vec<(ColumnKey, i32)> {
    let PendingPacket::SubChunk(packet) = packet else {
        return Vec::new();
    };
    let SubchunkPacketEntries::SubChunkEntryWithCaching(entries) = &packet.entries else {
        return Vec::new();
    };
    stable_unique_sub_chunks(entries.iter().map(|entry| {
        (
            pending_sub_chunk_column(packet, entry.dx, entry.dz),
            packet.origin.y.saturating_add(i32::from(entry.dy)),
        )
    }))
}

fn pending_sub_chunk_column(packet: &SubchunkPacket, dx: i8, dz: i8) -> ColumnKey {
    ColumnKey {
        dimension: packet.dimension,
        x: packet.origin.x.saturating_add(i32::from(dx)),
        z: packet.origin.z.saturating_add(i32::from(dz)),
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

fn stable_unique_sub_chunks(
    sub_chunks: impl IntoIterator<Item = (ColumnKey, i32)>,
) -> Vec<(ColumnKey, i32)> {
    let mut seen = HashSet::new();
    sub_chunks
        .into_iter()
        .filter(|sub_chunk| seen.insert(*sub_chunk))
        .collect()
}

use super::*;

pub(super) fn projected_reconstruction_bytes(
    cache: &ClientBlobCache,
    packet: &PendingPacket,
    hashes: &[u64],
) -> Result<Option<usize>, BlobCacheError> {
    let store = cache.lock();
    match packet {
        PendingPacket::LevelChunk(packet) => {
            let mut bytes = packet.payload.len();
            for hash in hashes {
                let Some(entry) = store.entries.get(hash) else {
                    return Ok(None);
                };
                bytes = bytes
                    .checked_add(entry.payload.len())
                    .ok_or(BlobCacheError::ByteCountOverflow)?;
            }
            Ok(Some(bytes))
        }
        PendingPacket::SubChunk(packet) => {
            let SubchunkPacketEntries::SubChunkEntryWithCaching(entries) = &packet.entries else {
                return Err(BlobCacheError::NotCachedPacket);
            };
            let mut bytes = 0_usize;
            for entry in entries {
                if entry.result != SubChunkEntryWithCachingItemResult::Success {
                    continue;
                }
                let Some(blob) = store.entries.get(&entry.blob_id) else {
                    return Ok(None);
                };
                bytes = bytes
                    .checked_add(blob.payload.len())
                    .and_then(|bytes| bytes.checked_add(entry.payload.as_ref().map_or(0, Vec::len)))
                    .ok_or(BlobCacheError::ByteCountOverflow)?;
            }
            Ok(Some(bytes))
        }
    }
}

pub(super) fn reconstruct(
    cache: &ClientBlobCache,
    transaction: &PendingTransaction,
    stats: &mut BlobCacheStats,
) -> Result<BlobCacheReady, BlobCacheError> {
    match &transaction.packet {
        PendingPacket::LevelChunk(packet) => {
            let mut packet = (**packet).clone();
            let payload_len =
                transaction
                    .hashes
                    .iter()
                    .try_fold(packet.payload.len(), |bytes, hash| {
                        let blob = cache
                            .get(*hash)
                            .ok_or(BlobCacheError::MissingResolvedBlob(*hash))?;
                        bytes
                            .checked_add(blob.len())
                            .ok_or(BlobCacheError::ByteCountOverflow)
                    })?;
            let mut payload = Vec::with_capacity(payload_len);
            for &hash in &transaction.hashes {
                let blob = cache
                    .get(hash)
                    .ok_or(BlobCacheError::MissingResolvedBlob(hash))?;
                payload.extend_from_slice(&blob);
            }
            payload.extend_from_slice(&packet.payload);
            packet.payload = payload;
            packet.blobs = None;
            stats.reconstructed_level_chunks = stats.reconstructed_level_chunks.saturating_add(1);
            Ok(BlobCacheReady::Packet(packet.into()))
        }
        PendingPacket::SubChunk(packet) => {
            let mut packet = (**packet).clone();
            let SubchunkPacketEntries::SubChunkEntryWithCaching(entries) = packet.entries else {
                return Err(BlobCacheError::NotCachedPacket);
            };
            let mut ordinary = Vec::with_capacity(entries.len());
            for entry in entries {
                let result = match entry.result {
                    SubChunkEntryWithCachingItemResult::Undefined => {
                        SubChunkEntryWithoutCachingItemResult::Undefined
                    }
                    SubChunkEntryWithCachingItemResult::Success => {
                        SubChunkEntryWithoutCachingItemResult::Success
                    }
                    SubChunkEntryWithCachingItemResult::ChunkNotFound => {
                        SubChunkEntryWithoutCachingItemResult::ChunkNotFound
                    }
                    SubChunkEntryWithCachingItemResult::InvalidDimension => {
                        SubChunkEntryWithoutCachingItemResult::InvalidDimension
                    }
                    SubChunkEntryWithCachingItemResult::PlayerNotFound => {
                        SubChunkEntryWithoutCachingItemResult::PlayerNotFound
                    }
                    SubChunkEntryWithCachingItemResult::YIndexOutOfBounds => {
                        SubChunkEntryWithoutCachingItemResult::YIndexOutOfBounds
                    }
                    SubChunkEntryWithCachingItemResult::SuccessAllAir => {
                        SubChunkEntryWithoutCachingItemResult::SuccessAllAir
                    }
                    SubChunkEntryWithCachingItemResult::Unknown(value) => {
                        SubChunkEntryWithoutCachingItemResult::Unknown(value)
                    }
                };
                let payload = if entry.result == SubChunkEntryWithCachingItemResult::Success {
                    let blob = cache
                        .get(entry.blob_id)
                        .ok_or(BlobCacheError::MissingResolvedBlob(entry.blob_id))?;
                    let tail = entry.payload.unwrap_or_default();
                    let payload_len = blob
                        .len()
                        .checked_add(tail.len())
                        .ok_or(BlobCacheError::ByteCountOverflow)?;
                    let mut payload = Vec::with_capacity(payload_len);
                    payload.extend_from_slice(&blob);
                    payload.extend_from_slice(&tail);
                    payload
                } else {
                    entry.payload.unwrap_or_default()
                };
                ordinary.push(SubChunkEntryWithoutCachingItem {
                    dx: entry.dx,
                    dy: entry.dy,
                    dz: entry.dz,
                    result,
                    payload,
                    heightmap_type: entry.heightmap_type,
                    heightmap: entry.heightmap,
                    render_heightmap_type: entry.render_heightmap_type,
                    render_heightmap: entry.render_heightmap,
                });
            }
            packet.entries = SubchunkPacketEntries::SubChunkEntryWithoutCaching(ordinary);
            stats.reconstructed_sub_chunks = stats.reconstructed_sub_chunks.saturating_add(1);
            Ok(BlobCacheReady::Packet(packet.into()))
        }
    }
}

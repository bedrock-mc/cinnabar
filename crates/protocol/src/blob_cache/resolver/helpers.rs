use super::*;

pub(super) fn classified_cached_status(
    cache: &ClientBlobCache,
    packet: &Packet,
) -> ClientCacheBlobStatusPacket {
    let hashes = match &packet.data {
        McpePacketData::PacketLevelChunk(packet) => packet
            .blobs
            .as_ref()
            .map_or_else(Vec::new, |blobs| stable_unique(&blobs.hashes)),
        McpePacketData::PacketSubchunk(packet) => {
            let SubchunkPacketEntries::SubChunkEntryWithCaching(entries) = &packet.entries else {
                return ClientCacheBlobStatusPacket::default();
            };
            stable_unique(
                &entries
                    .iter()
                    .filter(|entry| entry.result == SubChunkEntryWithCachingItemResult::Success)
                    .map(|entry| entry.blob_id)
                    .collect::<Vec<_>>(),
            )
        }
        _ => Vec::new(),
    };
    let (have, missing) = cache.classify(&hashes);
    ClientCacheBlobStatusPacket { missing, have }
}

pub(super) fn level_chunk_resync(packet: &Packet) -> Option<ChunkResyncEvent> {
    let McpePacketData::PacketLevelChunk(packet) = &packet.data else {
        return None;
    };
    level_chunk_resync_packet(packet)
}

pub(super) fn level_chunk_resync_packet(packet: &LevelChunkPacket) -> Option<ChunkResyncEvent> {
    let requested_sub_chunks = match packet.sub_chunk_count {
        count if count >= 0 => usize::try_from(count).ok().map(Some)?,
        -2 => packet.highest_subchunk_count.map(usize::from).map(Some)?,
        -1 => None,
        _ => return None,
    };
    Some(ChunkResyncEvent {
        dimension: packet.dimension,
        x: packet.x,
        z: packet.z,
        requested_sub_chunks,
    })
}

pub(super) fn reconstructed_accounted_bytes(
    cache: &ClientBlobCache,
    transaction: &PendingTransaction,
) -> Result<usize, BlobCacheError> {
    match &transaction.packet {
        PendingPacket::LevelChunk(packet) => {
            let base = size_of::<LevelChunkPacket>()
                .checked_add(packet.payload.len())
                .ok_or(BlobCacheError::ByteCountOverflow)?;
            transaction.hashes.iter().try_fold(base, |bytes, hash| {
                let blob = cache
                    .get(*hash)
                    .ok_or(BlobCacheError::MissingResolvedBlob(*hash))?;
                bytes
                    .checked_add(blob.len())
                    .ok_or(BlobCacheError::ByteCountOverflow)
            })
        }
        PendingPacket::SubChunk(packet) => {
            let SubchunkPacketEntries::SubChunkEntryWithCaching(entries) = &packet.entries else {
                return Err(BlobCacheError::NotCachedPacket);
            };
            let base = entries
                .len()
                .checked_mul(size_of::<SubChunkEntryWithoutCachingItem>())
                .and_then(|bytes| bytes.checked_add(size_of::<SubchunkPacket>()))
                .ok_or(BlobCacheError::ByteCountOverflow)?;
            entries.iter().try_fold(base, |bytes, entry| {
                let bytes = bytes
                    .checked_add(entry.payload.as_ref().map_or(0, Vec::len))
                    .ok_or(BlobCacheError::ByteCountOverflow)?;
                if entry.result == SubChunkEntryWithCachingItemResult::Success {
                    let blob = cache
                        .get(entry.blob_id)
                        .ok_or(BlobCacheError::MissingResolvedBlob(entry.blob_id))?;
                    bytes
                        .checked_add(blob.len())
                        .ok_or(BlobCacheError::ByteCountOverflow)
                } else {
                    Ok(bytes)
                }
            })
        }
    }
}

pub(super) fn decrement_authorization(authorizations: &mut Vec<(u64, usize)>, hash: u64) -> bool {
    let Some(index) = authorizations
        .iter()
        .position(|(candidate, count)| *candidate == hash && *count > 0)
    else {
        return false;
    };
    authorizations[index].1 -= 1;
    if authorizations[index].1 == 0 {
        authorizations.remove(index);
    }
    true
}

pub(super) fn increment_authorization(
    authorizations: &mut Vec<(u64, usize)>,
    hash: u64,
) -> Result<(), BlobCacheError> {
    if let Some((_, count)) = authorizations
        .iter_mut()
        .find(|(candidate, _)| *candidate == hash)
    {
        *count = count
            .checked_add(1)
            .ok_or(BlobCacheError::ByteCountOverflow)?;
    } else {
        authorizations.push((hash, 1));
    }
    Ok(())
}

pub(super) fn stable_unique(hashes: &[u64]) -> Vec<u64> {
    let mut seen = HashSet::with_capacity(hashes.len());
    hashes
        .iter()
        .copied()
        .filter(|hash| seen.insert(*hash))
        .collect()
}

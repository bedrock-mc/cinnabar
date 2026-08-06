use super::*;

pub(super) fn projected_reconstruction_bytes(
    cache: &ClientBlobCache,
    packet: &PendingPacket,
    hashes: &[u64],
) -> Result<Option<usize>, BlobCacheError> {
    let store = cache.lock();
    match packet {
        PendingPacket::LevelChunk(packet) => {
            let mut bytes = packet.serialized_chunk_data.len();
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
            if !packet.cache_enabled {
                return Err(BlobCacheError::NotCachedPacket);
            }
            let mut bytes = 0_usize;
            for entry in &packet.sub_chunk_data {
                if entry.sub_chunk_request_result != SubChunkRequestResult::Success {
                    continue;
                }
                let Some(blob_id) = entry.blob_id else {
                    continue;
                };
                let Some(blob) = store.entries.get(&blob_id) else {
                    return Ok(None);
                };
                bytes = bytes
                    .checked_add(blob.payload.len())
                    .and_then(|bytes| {
                        bytes.checked_add(
                            entry.serialized_sub_chunk.as_ref().map_or(0, Vec::len),
                        )
                    })
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
            let payload_len = transaction.hashes.iter().try_fold(
                packet.serialized_chunk_data.len(),
                |bytes, hash| {
                    let blob = cache
                        .get(*hash)
                        .ok_or(BlobCacheError::MissingResolvedBlob(*hash))?;
                    bytes
                        .checked_add(blob.len())
                        .ok_or(BlobCacheError::ByteCountOverflow)
                },
            )?;
            let mut payload = Vec::with_capacity(payload_len);
            for &hash in &transaction.hashes {
                let blob = cache
                    .get(hash)
                    .ok_or(BlobCacheError::MissingResolvedBlob(hash))?;
                payload.extend_from_slice(&blob);
            }
            payload.extend_from_slice(&packet.serialized_chunk_data);
            packet.serialized_chunk_data = payload;
            // The hashes are inlined now, so the reconstructed packet is no
            // longer cache-backed. 1.26.40 says that with the flag plus an empty
            // metadata list rather than by dropping an optional.
            packet.cache_enabled = false;
            packet.cache_metadata.clear();
            stats.reconstructed_level_chunks = stats.reconstructed_level_chunks.saturating_add(1);
            Ok(BlobCacheReady::Packet(packet.into()))
        }
        PendingPacket::SubChunk(packet) => {
            let mut packet = (**packet).clone();
            if !packet.cache_enabled {
                return Err(BlobCacheError::NotCachedPacket);
            }
            // Cached and uncached sub-chunk entries are one type in 1.26.40,
            // distinguished only by whether blob_id is set, so resolving an
            // entry means splicing the blob in front of its inline tail and
            // clearing the ID -- no result-enum translation between two entry
            // shapes is needed any more.
            for entry in &mut packet.sub_chunk_data {
                let tail = entry.serialized_sub_chunk.take().unwrap_or_default();
                let resolved = match entry.blob_id.take() {
                    Some(blob_id)
                        if entry.sub_chunk_request_result == SubChunkRequestResult::Success =>
                    {
                        let blob = cache
                            .get(blob_id)
                            .ok_or(BlobCacheError::MissingResolvedBlob(blob_id))?;
                        let payload_len = blob
                            .len()
                            .checked_add(tail.len())
                            .ok_or(BlobCacheError::ByteCountOverflow)?;
                        let mut payload = Vec::with_capacity(payload_len);
                        payload.extend_from_slice(&blob);
                        payload.extend_from_slice(&tail);
                        payload
                    }
                    _ => tail,
                };
                entry.serialized_sub_chunk = Some(resolved);
            }
            packet.cache_enabled = false;
            stats.reconstructed_sub_chunks = stats.reconstructed_sub_chunks.saturating_add(1);
            Ok(BlobCacheReady::Packet(packet.into()))
        }
    }
}

use super::*;

pub(super) fn classified_cached_status(
    cache: &ClientBlobCache,
    packet: &Packet,
) -> BlobCacheStatus {
    let hashes = match &packet.data {
        McpePacketData::PacketLevelChunk(packet) => packet
            .blobs
            .as_ref()
            .map_or_else(Vec::new, |blobs| stable_unique(&blobs.hashes)),
        McpePacketData::PacketSubchunk(packet) => {
            let SubchunkPacketEntries::SubChunkEntryWithCaching(entries) = &packet.entries else {
                return BlobCacheStatus::default();
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
    BlobCacheStatus { missing, have }
}

pub(super) fn stable_unique(hashes: &[u64]) -> Vec<u64> {
    let mut seen = HashSet::with_capacity(hashes.len());
    hashes
        .iter()
        .copied()
        .filter(|hash| seen.insert(*hash))
        .collect()
}

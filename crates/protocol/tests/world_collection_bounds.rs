use bytes::{Bytes, BytesMut};
use valentine::bedrock::{
    codec::{BedrockCodec, U32LE, VarInt},
    error::DecodeError,
    version::v1_26_30::{
        BlockUpdate, DimensionDataPacket, DimensionDataPacketDefinitionsItem, LevelChunkPacket,
        LevelChunkPacketBlobs, NetworkChunkPublisherUpdatePacket,
        NetworkChunkPublisherUpdatePacketSavedChunksItem, SubChunkEntryWithCachingItem,
        SubChunkEntryWithCachingItemResult, SubChunkEntryWithoutCachingItem, SubchunkPacket,
        SubchunkPacketEntries, UpdateSubchunkBlocksPacket,
    },
};

const MAX_SUB_CHUNK_ENTRIES: usize = 256;
const MAX_WORLD_BLOCK_UPDATES: usize = 4096;
const MAX_PACKET_BYTE_ARRAY_BYTES: usize = 16 * 1024 * 1024;
const MAX_WORLD_COLLECTION_ELEMENTS: usize = 4096;
const MAX_DIMENSION_DEFINITIONS: usize = 64;

fn assert_limit_error(error: DecodeError, declared: usize, available: usize) {
    assert!(
        matches!(
            error,
            DecodeError::ArrayLengthExceeded {
                declared: actual_declared,
                available: actual_available,
            } if actual_declared == declared && actual_available == available
        ),
        "unexpected decode error: {error:?}",
    );
}

fn malicious_collection_prefix<T: BedrockCodec>(
    empty: &T,
    one: &T,
    encode_count: impl FnOnce(&mut BytesMut),
) -> Bytes {
    let mut empty_bytes = BytesMut::new();
    empty.encode(&mut empty_bytes).expect("encode empty value");
    let mut one_bytes = BytesMut::new();
    one.encode(&mut one_bytes).expect("encode one-item value");
    let count_offset = empty_bytes
        .iter()
        .zip(one_bytes.iter())
        .position(|(empty, one)| empty != one)
        .expect("one-item value must differ at its collection count");

    let mut prefix = BytesMut::from(&empty_bytes[..count_offset]);
    encode_count(&mut prefix);
    prefix.freeze()
}

fn oversized_varint(bytes: &mut BytesMut, limit: usize) {
    VarInt((limit + 1) as i32)
        .encode(bytes)
        .expect("oversized varint length");
}

#[test]
fn level_chunk_rejects_oversized_payload_before_allocation() {
    let empty = LevelChunkPacket::default();
    let mut one = empty.clone();
    one.payload.push(0);
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_PACKET_BYTE_ARRAY_BYTES)
    });

    let error = LevelChunkPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_PACKET_BYTE_ARRAY_BYTES + 1,
        MAX_PACKET_BYTE_ARRAY_BYTES,
    );
}

#[test]
fn level_chunk_rejects_payload_longer_than_remaining_buffer_before_allocation() {
    let empty = LevelChunkPacket::default();
    let mut one = empty.clone();
    one.payload.push(0);
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        VarInt(1).encode(bytes).expect("one-byte payload length")
    });

    let error = LevelChunkPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(error, 1, 0);
}

#[test]
fn update_subchunk_blocks_rejects_oversized_primary_updates_before_allocation() {
    let empty = UpdateSubchunkBlocksPacket::default();
    let mut one = empty.clone();
    one.blocks.push(BlockUpdate::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_WORLD_BLOCK_UPDATES)
    });

    let error = UpdateSubchunkBlocksPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(error, MAX_WORLD_BLOCK_UPDATES + 1, MAX_WORLD_BLOCK_UPDATES);
}

#[test]
fn update_subchunk_blocks_rejects_oversized_extra_updates_before_allocation() {
    let empty = UpdateSubchunkBlocksPacket::default();
    let mut one = empty.clone();
    one.extra.push(BlockUpdate::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_WORLD_BLOCK_UPDATES)
    });

    let error = UpdateSubchunkBlocksPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(error, MAX_WORLD_BLOCK_UPDATES + 1, MAX_WORLD_BLOCK_UPDATES);
}

#[test]
fn subchunk_rejects_oversized_uncached_entry_count_before_allocation() {
    let empty = SubchunkPacket::default();
    let mut one = empty.clone();
    one.entries = SubchunkPacketEntries::SubChunkEntryWithoutCaching(vec![
        SubChunkEntryWithoutCachingItem::default(),
    ]);
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        U32LE((MAX_SUB_CHUNK_ENTRIES + 1) as u32)
            .encode(bytes)
            .expect("oversized uncached entry count")
    });

    let error = SubchunkPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(error, MAX_SUB_CHUNK_ENTRIES + 1, MAX_SUB_CHUNK_ENTRIES);
}

#[test]
fn subchunk_rejects_oversized_cached_entry_count_before_allocation() {
    let empty = SubchunkPacket {
        entries: SubchunkPacketEntries::SubChunkEntryWithCaching(Vec::new()),
        ..Default::default()
    };
    let mut one = empty.clone();
    one.entries =
        SubchunkPacketEntries::SubChunkEntryWithCaching(vec![SubChunkEntryWithCachingItem {
            result: SubChunkEntryWithCachingItemResult::SuccessAllAir,
            ..Default::default()
        }]);
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        U32LE((MAX_SUB_CHUNK_ENTRIES + 1) as u32)
            .encode(bytes)
            .expect("oversized cached entry count")
    });

    let error = SubchunkPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(error, MAX_SUB_CHUNK_ENTRIES + 1, MAX_SUB_CHUNK_ENTRIES);
}

#[test]
fn uncached_subchunk_entry_rejects_oversized_payload_before_allocation() {
    let empty = SubChunkEntryWithoutCachingItem::default();
    let mut one = empty.clone();
    one.payload.push(0);
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_PACKET_BYTE_ARRAY_BYTES)
    });

    let error = SubChunkEntryWithoutCachingItem::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_PACKET_BYTE_ARRAY_BYTES + 1,
        MAX_PACKET_BYTE_ARRAY_BYTES,
    );
}

#[test]
fn cached_subchunk_entry_rejects_oversized_payload_before_allocation() {
    let empty = SubChunkEntryWithCachingItem {
        result: SubChunkEntryWithCachingItemResult::Success,
        payload: Some(Vec::new()),
        ..Default::default()
    };
    let mut one = empty.clone();
    one.payload = Some(vec![0]);
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_PACKET_BYTE_ARRAY_BYTES)
    });

    let error = SubChunkEntryWithCachingItem::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_PACKET_BYTE_ARRAY_BYTES + 1,
        MAX_PACKET_BYTE_ARRAY_BYTES,
    );
}

#[test]
fn level_chunk_blobs_reject_oversized_hash_count_before_allocation() {
    let empty = LevelChunkPacketBlobs::default();
    let mut one = empty.clone();
    one.hashes.push(0);
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_WORLD_COLLECTION_ELEMENTS)
    });

    let error = LevelChunkPacketBlobs::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_WORLD_COLLECTION_ELEMENTS + 1,
        MAX_WORLD_COLLECTION_ELEMENTS,
    );
}

#[test]
fn chunk_publisher_rejects_oversized_saved_chunk_count_before_allocation() {
    let empty = NetworkChunkPublisherUpdatePacket::default();
    let mut one = empty.clone();
    one.saved_chunks
        .push(NetworkChunkPublisherUpdatePacketSavedChunksItem::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        U32LE((MAX_WORLD_COLLECTION_ELEMENTS + 1) as u32)
            .encode(bytes)
            .expect("oversized saved-chunk count")
    });

    let error = NetworkChunkPublisherUpdatePacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_WORLD_COLLECTION_ELEMENTS + 1,
        MAX_WORLD_COLLECTION_ELEMENTS,
    );
}

#[test]
fn dimension_data_rejects_oversized_definition_count_before_allocation() {
    let empty = DimensionDataPacket::default();
    let mut one = empty.clone();
    one.definitions
        .push(DimensionDataPacketDefinitionsItem::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_DIMENSION_DEFINITIONS)
    });

    let error = DimensionDataPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_DIMENSION_DEFINITIONS + 1,
        MAX_DIMENSION_DEFINITIONS,
    );
}

//! Decode bounds on world-packet collections.
//!
//! Generated decoders grow collections fallibly instead of reserving directly
//! from untrusted counts. Declared-but-absent elements therefore fail as
//! truncated wire, without imposing a global element ceiling.

use bytes::{Bytes, BytesMut};
use valentine::bedrock::{
    codec::{BedrockCodec, U32LE, VarInt},
    error::DecodeError,
    version::v1_26_44::{
        ChunkPos, DimensionDataPacket, DimensionDataPacketDefinitionsItem,
        EnumsSubChunkPacketPayloadSubChunkRequestResult, LevelChunkPacket,
        LevelChunkPacketPayloadSubChunkMetadata, NetworkChunkPublisherUpdatePacket, SubChunkPacket,
        SubChunkPacketPayloadSubChunkPacketData, UpdateSubChunkBlocksPacket,
        UpdateSubChunkNetworkBlockInfo,
    },
};

const MAX_SUB_CHUNK_ENTRIES: usize = 256;
const MAX_WORLD_BLOCK_UPDATES: usize = 4096;
const MAX_PACKET_BYTE_ARRAY_BYTES: usize = 16 * 1024 * 1024;
const MAX_WORLD_COLLECTION_ELEMENTS: usize = 4096;
const MAX_DIMENSION_DEFINITIONS: usize = 64;

/// Asserts a declared-but-absent collection fails without an allocation ceiling.
#[track_caller]
fn assert_rejected_without_a_length_ceiling(error: DecodeError) {
    match error {
        DecodeError::ArrayLengthExceeded {
            declared,
            available,
        } if declared > available => {}
        DecodeError::UnexpectedEof { .. } => {}
        DecodeError::Io(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {}
        error => panic!("unexpected decode error: {error:?}"),
    }
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
fn level_chunk_rejects_oversized_payload() {
    let empty = LevelChunkPacket::default();
    let mut one = empty.clone();
    one.serialized_chunk_data.push(0);
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_PACKET_BYTE_ARRAY_BYTES)
    });

    assert_rejected_without_a_length_ceiling(LevelChunkPacket::decode(&mut bytes, ()).unwrap_err());
}

#[test]
fn level_chunk_rejects_payload_longer_than_remaining_buffer() {
    let empty = LevelChunkPacket::default();
    let mut one = empty.clone();
    one.serialized_chunk_data.push(0);
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        VarInt(1).encode(bytes).expect("one-byte payload length")
    });

    assert_rejected_without_a_length_ceiling(LevelChunkPacket::decode(&mut bytes, ()).unwrap_err());
}

/// The 1.26.30 `LevelChunkPacketBlobs` sub-struct is gone: gophertunnel
/// `packet/level_chunk.go` writes `BlobHashes []uint64` inline and
/// unconditionally, which the generated crate models as `cache_metadata`.
#[test]
fn level_chunk_rejects_oversized_cache_metadata_hash_count() {
    let empty = LevelChunkPacket::default();
    let mut one = empty.clone();
    one.cache_metadata
        .push(LevelChunkPacketPayloadSubChunkMetadata::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_WORLD_COLLECTION_ELEMENTS)
    });

    assert_rejected_without_a_length_ceiling(LevelChunkPacket::decode(&mut bytes, ()).unwrap_err());
}

#[test]
fn update_subchunk_blocks_rejects_oversized_primary_updates() {
    let empty = UpdateSubChunkBlocksPacket::default();
    let mut one = empty.clone();
    one.blocks_changed
        .blocks_changed_standards
        .push(UpdateSubChunkNetworkBlockInfo::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_WORLD_BLOCK_UPDATES)
    });

    assert_rejected_without_a_length_ceiling(
        UpdateSubChunkBlocksPacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

#[test]
fn update_subchunk_blocks_rejects_oversized_extra_updates() {
    let empty = UpdateSubChunkBlocksPacket::default();
    let mut one = empty.clone();
    one.blocks_changed
        .blocks_changed_extras
        .push(UpdateSubChunkNetworkBlockInfo::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_WORLD_BLOCK_UPDATES)
    });

    assert_rejected_without_a_length_ceiling(
        UpdateSubChunkBlocksPacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

/// 1.26.40 merged the cached and non-cached SubChunk entry lists into one
/// `sub_chunk_data` slice, and its count is a varuint32 rather than the u32 the
/// 1.26.30 model used (gophertunnel `packet/sub_chunk.go` uses `protocol.Slice`).
#[test]
fn subchunk_rejects_oversized_entry_count() {
    let empty = SubChunkPacket::default();
    let mut one = empty.clone();
    one.sub_chunk_data
        .push(SubChunkPacketPayloadSubChunkPacketData::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_SUB_CHUNK_ENTRIES)
    });

    assert_rejected_without_a_length_ceiling(SubChunkPacket::decode(&mut bytes, ()).unwrap_err());
}

#[test]
fn subchunk_entry_rejects_oversized_payload() {
    let empty = SubChunkPacketPayloadSubChunkPacketData {
        sub_chunk_request_result: EnumsSubChunkPacketPayloadSubChunkRequestResult::Success,
        serialized_sub_chunk: Some(Vec::new()),
        ..Default::default()
    };
    let mut one = empty.clone();
    one.serialized_sub_chunk = Some(vec![0]);
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_PACKET_BYTE_ARRAY_BYTES)
    });

    assert_rejected_without_a_length_ceiling(
        SubChunkPacketPayloadSubChunkPacketData::decode(&mut bytes, ()).unwrap_err(),
    );
}

#[test]
fn chunk_publisher_rejects_oversized_saved_chunk_count() {
    let empty = NetworkChunkPublisherUpdatePacket::default();
    let mut one = empty.clone();
    one.server_built_chunks_list.push(ChunkPos::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        U32LE((MAX_WORLD_COLLECTION_ELEMENTS + 1) as u32)
            .encode(bytes)
            .expect("oversized saved-chunk count")
    });

    assert_rejected_without_a_length_ceiling(
        NetworkChunkPublisherUpdatePacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

#[test]
fn dimension_data_rejects_oversized_definition_count() {
    let empty = DimensionDataPacket::default();
    let mut one = empty.clone();
    one.definitions
        .push(DimensionDataPacketDefinitionsItem::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_DIMENSION_DEFINITIONS)
    });

    assert_rejected_without_a_length_ceiling(
        DimensionDataPacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

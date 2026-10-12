//! Decode bounds on world-packet collections.
//!
//! Generated decoders check every declared count against the remaining input
//! before allocating. Declared-but-absent elements therefore fail as
//! truncated wire, without imposing a global element ceiling.

use bedrock_protocol_1_26_51::enums::SubChunkRequestResult;
use bedrock_protocol_1_26_51::packets::{
    DimensionData, LevelChunk, NetworkChunkPublisherUpdate, SubChunk, UpdateSubChunkBlocks,
};
use bedrock_protocol_1_26_51::types::{
    ChunkPos, DimensionDefinition, SubChunkData, SubChunkMetadata, UpdateSubChunkNetworkBlockInfo,
};
use bedrock_protocol_1_26_51::wire::{self, Decode, Encode};

const MAX_SUB_CHUNK_ENTRIES: usize = 256;
const MAX_WORLD_BLOCK_UPDATES: usize = 4096;
const MAX_PACKET_BYTE_ARRAY_BYTES: usize = 16 * 1024 * 1024;
const MAX_WORLD_COLLECTION_ELEMENTS: usize = 4096;
const MAX_DIMENSION_DEFINITIONS: usize = 64;

/// Asserts a declared-but-absent collection fails without an allocation ceiling.
#[track_caller]
fn assert_rejected_without_a_length_ceiling(error: wire::DecodeError) {
    match error {
        wire::DecodeError::LengthNotRepresentable {
            needed, remaining, ..
        } if needed > remaining => {}
        wire::DecodeError::UnexpectedEof { .. } => {}
        error => panic!("unexpected decode error: {error:?}"),
    }
}

fn malicious_collection_prefix<T: Encode>(
    empty: &T,
    one: &T,
    encode_count: impl FnOnce(&mut Vec<u8>),
) -> Vec<u8> {
    let empty_bytes = empty.encode_to_vec().expect("encode empty value");
    let one_bytes = one.encode_to_vec().expect("encode one-item value");
    let count_offset = empty_bytes
        .iter()
        .zip(one_bytes.iter())
        .position(|(empty, one)| empty != one)
        .expect("one-item value must differ at its collection count");

    let mut prefix = empty_bytes[..count_offset].to_vec();
    encode_count(&mut prefix);
    prefix
}

fn oversized_varint(bytes: &mut Vec<u8>, limit: usize) {
    wire::Writer::new(bytes)
        .write_var_u32((limit + 1) as u32)
        .expect("oversized varint length");
}

#[test]
fn level_chunk_rejects_oversized_payload() {
    let empty = LevelChunk::default();
    let mut one = empty.clone();
    one.serialized_chunk_data = vec![0].into();
    let bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_PACKET_BYTE_ARRAY_BYTES)
    });

    assert_rejected_without_a_length_ceiling(LevelChunk::decode_exact(&bytes).unwrap_err());
}

#[test]
fn level_chunk_rejects_payload_longer_than_remaining_buffer() {
    let empty = LevelChunk::default();
    let mut one = empty.clone();
    one.serialized_chunk_data = vec![0].into();
    let bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        wire::Writer::new(bytes)
            .write_var_u32(1)
            .expect("one-byte payload length")
    });

    assert_rejected_without_a_length_ceiling(LevelChunk::decode_exact(&bytes).unwrap_err());
}

/// gophertunnel `packet/level_chunk.go` writes `BlobHashes []uint64` inline and
/// unconditionally, which the generated crate models as `cache_metadata`. A declared count the
/// input cannot hold is truncation, which stays fatal.
#[test]
fn level_chunk_rejects_oversized_cache_metadata_hash_count() {
    let empty = LevelChunk::default();
    let mut one = empty.clone();
    one.cache_metadata.push(SubChunkMetadata::default());
    let bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_WORLD_COLLECTION_ELEMENTS)
    });

    assert_rejected_without_a_length_ceiling(LevelChunk::decode_exact(&bytes).unwrap_err());
}

#[test]
fn update_subchunk_blocks_rejects_oversized_primary_updates() {
    let empty = UpdateSubChunkBlocks::default();
    let mut one = empty.clone();
    one.blocks_changed
        .blocks_changed_standards
        .push(UpdateSubChunkNetworkBlockInfo::default());
    let bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_WORLD_BLOCK_UPDATES)
    });

    assert_rejected_without_a_length_ceiling(
        UpdateSubChunkBlocks::decode_exact(&bytes).unwrap_err(),
    );
}

#[test]
fn update_subchunk_blocks_rejects_oversized_extra_updates() {
    let empty = UpdateSubChunkBlocks::default();
    let mut one = empty.clone();
    one.blocks_changed
        .blocks_changed_extras
        .push(UpdateSubChunkNetworkBlockInfo::default());
    let bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_WORLD_BLOCK_UPDATES)
    });

    assert_rejected_without_a_length_ceiling(
        UpdateSubChunkBlocks::decode_exact(&bytes).unwrap_err(),
    );
}

/// The `sub_chunk_data` count is a varuint32 (gophertunnel `packet/sub_chunk.go` uses
/// `protocol.Slice`).
#[test]
fn subchunk_rejects_oversized_entry_count() {
    let empty = SubChunk::default();
    let mut one = empty.clone();
    one.sub_chunk_data.push(SubChunkData::default());
    let bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_SUB_CHUNK_ENTRIES)
    });

    assert_rejected_without_a_length_ceiling(SubChunk::decode_exact(&bytes).unwrap_err());
}

#[test]
fn subchunk_entry_rejects_oversized_payload() {
    let empty = SubChunkData {
        sub_chunk_request_result: SubChunkRequestResult::Success,
        serialized_sub_chunk: Some(bytes::Bytes::new()),
        ..Default::default()
    };
    let mut one = empty.clone();
    one.serialized_sub_chunk = Some(vec![0].into());
    let bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_PACKET_BYTE_ARRAY_BYTES)
    });

    assert_rejected_without_a_length_ceiling(SubChunkData::decode_exact(&bytes).unwrap_err());
}

#[test]
fn chunk_publisher_rejects_oversized_saved_chunk_count() {
    let empty = NetworkChunkPublisherUpdate::default();
    let mut one = empty.clone();
    one.server_built_chunks_list.push(ChunkPos::default());
    let bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        wire::Writer::new(bytes)
            .write_all(&((MAX_WORLD_COLLECTION_ELEMENTS + 1) as u32).to_le_bytes())
            .expect("oversized saved-chunk count")
    });

    assert_rejected_without_a_length_ceiling(
        NetworkChunkPublisherUpdate::decode_exact(&bytes).unwrap_err(),
    );
}

#[test]
fn dimension_data_rejects_oversized_definition_count() {
    let empty = DimensionData::default();
    let mut one = empty.clone();
    one.definitions
        .push((Default::default(), DimensionDefinition::default()));
    let bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        oversized_varint(bytes, MAX_DIMENSION_DEFINITIONS)
    });

    assert_rejected_without_a_length_ceiling(DimensionData::decode_exact(&bytes).unwrap_err());
}

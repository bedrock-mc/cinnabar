use std::sync::{Arc, mpsc};

use world::{ChunkKey, ChunkStore, DecodeError, DecodedLevelChunk, SubChunk, SubChunkKey};

fn zig_zag_i32(value: i32) -> Vec<u8> {
    let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

fn uniform(y: i8, runtime_id: u32) -> Vec<u8> {
    let mut bytes = vec![9, 1, y as u8, 1];
    bytes.extend(zig_zag_i32(runtime_id as i32));
    bytes
}

/// Builds one uniform biome storage for transaction-scanning tests.
fn uniform_biome(runtime_id: u32) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend(zig_zag_i32(runtime_id as i32));
    bytes
}

#[test]
fn public_prefix_decode_can_be_committed_without_redecoding() {
    let key = SubChunkKey::new(0, 4, -4, 7);
    let encoded = uniform(-4, 91);
    let mut payload = encoded.clone();
    payload.extend_from_slice(&[0x0a, 0x00, 0x00]);

    let (decoded, consumed) = SubChunk::decode_prefix(&payload).expect("pure prefix decode");
    assert_eq!(consumed, encoded.len());
    assert_eq!(decoded.runtime_id(0, 0, 0, 0), Some(91));

    let mut store = ChunkStore::new();
    assert_eq!(store.commit_sub_chunk(key, decoded).unwrap(), Some(key));
    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(91)
    );
}

#[test]
fn level_chunk_decodes_on_a_rayon_worker_then_commits_atomically() {
    let chunk_key = ChunkKey::new(2, -8, 11);
    let lower_key = SubChunkKey::from_chunk(chunk_key, -4);
    let upper_key = SubChunkKey::from_chunk(chunk_key, -3);
    let payload = [uniform(-4, 20), uniform(-3, 30)].concat();
    let expected_consumed = payload.len();
    let (send, receive) = mpsc::sync_channel(1);

    rayon::spawn(move || {
        send.send(DecodedLevelChunk::decode(-4, 2, &payload))
            .expect("send worker result");
    });
    let decoded = receive
        .recv()
        .expect("receive worker result")
        .expect("decode level chunk");

    assert_eq!(decoded.bytes_consumed(), expected_consumed);
    assert_eq!(
        decoded.sub_chunk(-4).unwrap().runtime_id(0, 0, 0, 0),
        Some(20)
    );
    assert_eq!(
        decoded.sub_chunk(-3).unwrap().runtime_id(0, 0, 0, 0),
        Some(30)
    );

    let mut store = ChunkStore::new();
    assert!(store.chunk(chunk_key).is_none(), "decode must be pure");
    let committed = store.commit_level_chunk(chunk_key, decoded).unwrap();
    assert!(committed.dirty.contains(&lower_key));
    assert!(committed.dirty.contains(&upper_key));
    assert_eq!(committed.bytes_consumed, expected_consumed);
    assert_eq!(
        store.sub_chunk(lower_key).unwrap().runtime_id(0, 0, 0, 0),
        Some(20)
    );
}

#[test]
fn later_malformed_sub_chunk_produces_no_committable_column() {
    let mut store = ChunkStore::new();
    let chunk_key = ChunkKey::new(0, 1, 2);
    let lower_key = SubChunkKey::from_chunk(chunk_key, -4);
    store
        .apply_level_chunk(chunk_key, -4, 1, &uniform(-4, 7))
        .unwrap();
    let before = store.sub_chunk(lower_key).unwrap();

    let mut malformed = uniform(-4, 99);
    malformed.push(9);
    assert!(matches!(
        DecodedLevelChunk::decode(-4, 2, &malformed),
        Err(DecodeError::UnexpectedEof { .. })
    ));

    let after = store.sub_chunk(lower_key).unwrap();
    assert!(Arc::ptr_eq(&before, &after));
    assert_eq!(after.runtime_id(0, 0, 0, 0), Some(7));
}

#[test]
fn semantic_index_mismatch_cannot_hide_later_malformed_chunk_wire() {
    let mut later_block_is_truncated = uniform(-3, 11);
    later_block_is_truncated.push(9);
    let block_error = DecodedLevelChunk::decode(-4, 2, &later_block_is_truncated).unwrap_err();
    assert!(block_error.wire_error_reason().is_some());

    let mut later_biome_is_truncated = uniform(-3, 11);
    later_biome_is_truncated.push(1);
    let biome_error =
        DecodedLevelChunk::decode_with_biomes(-4, 1, -4, 1, &later_biome_is_truncated).unwrap_err();
    assert!(biome_error.wire_error_reason().is_some());

    let mut reserved_prefix_is_missing = uniform(-3, 11);
    reserved_prefix_is_missing.extend(uniform_biome(4));
    let tail_error = DecodedLevelChunk::decode_with_biomes_and_block_entities(
        ChunkKey::new(0, 0, 0),
        -4,
        1,
        -4,
        1,
        &reserved_prefix_is_missing,
    )
    .unwrap_err();
    assert!(tail_error.wire_error_reason().is_some());
}

#[test]
fn complete_index_mismatch_remains_survivable_decode_policy() {
    let error = DecodedLevelChunk::decode(-4, 1, &uniform(-3, 11)).unwrap_err();
    assert_eq!(
        error,
        DecodeError::SubChunkIndexMismatch {
            expected: -4,
            actual: -3,
        }
    );
    assert!(error.wire_error_reason().is_none());
}

#[test]
fn commit_reuses_equal_worker_snapshots_and_rejects_wrong_y_without_mutation() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 3, -4, 5);
    let (first, _) = SubChunk::decode_prefix(&uniform(-4, 12)).unwrap();
    store.commit_sub_chunk(key, first).unwrap();
    let before = store.sub_chunk(key).unwrap();

    let (equal, _) = SubChunk::decode_prefix(&uniform(-4, 12)).unwrap();
    assert_eq!(store.commit_sub_chunk(key, equal).unwrap(), None);
    assert!(Arc::ptr_eq(&before, &store.sub_chunk(key).unwrap()));

    let (wrong_y, _) = SubChunk::decode_prefix(&uniform(-3, 99)).unwrap();
    assert_eq!(
        store.commit_sub_chunk(key, wrong_y),
        Err(DecodeError::SubChunkIndexMismatch {
            expected: -4,
            actual: -3,
        })
    );
    assert!(Arc::ptr_eq(&before, &store.sub_chunk(key).unwrap()));
}

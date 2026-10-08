use std::sync::{Arc, mpsc};

use world::{ChunkKey, ChunkStore, DecodedLevelChunk, RawBlockIds, SubChunk, SubChunkKey};

const IDS: RawBlockIds = RawBlockIds { air: 0 };

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

#[test]
fn public_prefix_decode_can_be_committed_without_redecoding() {
    let key = SubChunkKey::new(0, 4, -4, 7);
    let encoded = uniform(-4, 91);
    let mut payload = encoded.clone();
    payload.extend_from_slice(&[0x0a, 0x00, 0x00]);

    let (decoded, consumed) = SubChunk::decode_prefix(&payload, &IDS);
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
        send.send(DecodedLevelChunk::decode(-4, 2, &payload, &IDS))
            .expect("send worker result");
    });
    let decoded = receive.recv().expect("receive worker result");

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
fn truncated_level_chunk_decode_is_pure_until_committed() {
    let mut store = ChunkStore::new();
    let chunk_key = ChunkKey::new(0, 1, 2);
    let lower_key = SubChunkKey::from_chunk(chunk_key, -4);
    store
        .apply_level_chunk(chunk_key, -4, 1, &uniform(-4, 7), &IDS)
        .unwrap();
    let before = store.sub_chunk(lower_key).unwrap();

    let mut truncated = uniform(-4, 99);
    truncated.push(9);
    let decoded = DecodedLevelChunk::decode(-4, 2, &truncated, &IDS);
    assert_eq!(decoded.bytes_consumed(), truncated.len());
    assert!(decoded.sub_chunk(-3).is_none());
    assert!(Arc::ptr_eq(&before, &store.sub_chunk(lower_key).unwrap()));

    store.commit_level_chunk(chunk_key, decoded).unwrap();
    assert_eq!(
        store.sub_chunk(lower_key).unwrap().runtime_id(0, 0, 0, 0),
        Some(99)
    );
}

#[test]
fn misplaced_version_nine_sub_chunk_decodes_to_an_empty_column() {
    let payload = uniform(-3, 11);
    let decoded = DecodedLevelChunk::decode(-4, 1, &payload, &IDS);
    assert_eq!(decoded.sub_chunks().len(), 0);
    assert_eq!(decoded.bytes_consumed(), payload.len());
}

#[test]
fn commit_reuses_equal_worker_snapshots_and_ignores_the_y_byte() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 3, -4, 5);
    let (first, _) = SubChunk::decode_prefix(&uniform(-4, 12), &IDS);
    store.commit_sub_chunk(key, first).unwrap();
    let before = store.sub_chunk(key).unwrap();

    let (equal, _) = SubChunk::decode_prefix(&uniform(-4, 12), &IDS);
    assert_eq!(store.commit_sub_chunk(key, equal).unwrap(), None);
    assert!(Arc::ptr_eq(&before, &store.sub_chunk(key).unwrap()));

    let (other_y, _) = SubChunk::decode_prefix(&uniform(-3, 99), &IDS);
    assert_eq!(store.commit_sub_chunk(key, other_y), Ok(Some(key)));
    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(99)
    );
}

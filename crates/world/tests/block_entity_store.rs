use std::sync::Arc;

use world::{
    BlockEntityError, BlockEntityKey, ChunkKey, ChunkStore, DecodedBlockEntities,
    DecodedLevelChunk, DecodedSubChunk, MAX_BLOCK_ENTITIES_PER_CHUNK,
    MAX_BLOCK_ENTITY_BYTES_PER_CHUNK, SubChunkKey,
};

fn var_u32(mut value: u32, out: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn zigzag_i32(value: i32, out: &mut Vec<u8>) {
    var_u32(((value << 1) ^ (value >> 31)) as u32, out);
}

fn string(value: &str, out: &mut Vec<u8>) {
    var_u32(value.len() as u32, out);
    out.extend_from_slice(value.as_bytes());
}

fn block_entity(id: &str, position: [i32; 3]) -> Vec<u8> {
    let mut out = vec![10, 0];
    out.push(8);
    string("id", &mut out);
    string(id, &mut out);
    for (name, value) in [("x", position[0]), ("y", position[1]), ("z", position[2])] {
        out.push(3);
        string(name, &mut out);
        zigzag_i32(value, &mut out);
    }
    out.push(0);
    out
}

fn large_positionless_block_entity() -> Vec<u8> {
    let mut out = vec![10, 0];
    for _ in 0..63 {
        out.push(7);
        string("b", &mut out);
        zigzag_i32(16_384, &mut out);
        out.resize(out.len() + 16_384, 0);
    }
    out.push(0);
    out
}

fn large_block_entity(position: [i32; 3]) -> Vec<u8> {
    let mut out = block_entity("Big", position);
    assert_eq!(out.pop(), Some(0));
    for _ in 0..63 {
        out.push(7);
        string("b", &mut out);
        zigzag_i32(16_384, &mut out);
        out.resize(out.len() + 16_384, 0);
    }
    out.push(0);
    out
}

fn uniform_sub_chunk(y: i8, runtime_id: u32) -> Vec<u8> {
    let mut bytes = vec![9, 1, y as u8, 1];
    zigzag_i32(runtime_id as i32, &mut bytes);
    bytes
}

fn uniform_biome(id: u32) -> Vec<u8> {
    let mut bytes = vec![1];
    zigzag_i32(id as i32, &mut bytes);
    bytes
}

#[test]
fn chunk_tail_requires_zero_border_prefix_and_in_scope_unique_positions() {
    let chunk = ChunkKey::new(0, -2, 3);
    let first_key = BlockEntityKey::new(0, -31, 64, 49);
    let second_key = BlockEntityKey::new(0, -18, -1, 63);
    let first = block_entity("Chest", first_key.position());
    let second = block_entity("Sign", second_key.position());
    let mut tail = vec![0];
    tail.extend_from_slice(&first);
    tail.extend_from_slice(&second);

    let decoded = DecodedBlockEntities::decode_level_chunk_tail(chunk, &tail).unwrap();
    assert_eq!(decoded.bytes_consumed(), tail.len());
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded.get(first_key).unwrap().id(), Some("Chest"));
    assert_eq!(decoded.get(second_key).unwrap().id(), Some("Sign"));

    assert_eq!(
        DecodedBlockEntities::decode_level_chunk_tail(chunk, &[1]),
        Err(BlockEntityError::UnsupportedBorderBlocks { count: 1 })
    );

    let mut duplicate = vec![0];
    duplicate.extend_from_slice(&first);
    duplicate.extend_from_slice(&first);
    assert_eq!(
        DecodedBlockEntities::decode_level_chunk_tail(chunk, &duplicate),
        Err(BlockEntityError::DuplicatePosition { key: first_key })
    );

    let mut foreign = vec![0];
    foreign.extend(block_entity("Chest", [-17, 64, 64]));
    assert!(matches!(
        DecodedBlockEntities::decode_level_chunk_tail(chunk, &foreign),
        Err(BlockEntityError::OutsideChunk { .. })
    ));
}

#[test]
fn inline_level_chunk_decode_and_commit_are_atomic_with_entity_tail() {
    let chunk = ChunkKey::new(0, 1, 2);
    let sub_chunk = SubChunkKey::from_chunk(chunk, -4);
    let old_key = BlockEntityKey::new(0, 17, -63, 33);
    let new_key = BlockEntityKey::new(0, 18, -62, 34);
    let mut store = ChunkStore::new();
    let old = DecodedBlockEntities::decode_live(old_key, &block_entity("Old", old_key.position()))
        .unwrap();
    store.commit_block_entity_update(old_key, old).unwrap();
    store
        .apply_sub_chunk(sub_chunk, &uniform_sub_chunk(-4, 7))
        .unwrap();
    let before_block = store.sub_chunk(sub_chunk).unwrap();
    let before_entity = store.block_entity(old_key).unwrap();

    let mut malformed = uniform_sub_chunk(-4, 9);
    malformed.extend(uniform_biome(5));
    malformed.push(0);
    malformed.extend_from_slice(&[10, 0, 8, 2, b'i']);
    assert!(
        DecodedLevelChunk::decode_with_biomes_and_block_entities(chunk, -4, 1, -4, 1, &malformed)
            .is_err()
    );
    assert!(Arc::ptr_eq(
        &before_block,
        &store.sub_chunk(sub_chunk).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &before_entity,
        &store.block_entity(old_key).unwrap()
    ));

    let mut valid = uniform_sub_chunk(-4, 9);
    valid.extend(uniform_biome(6));
    valid.push(0);
    valid.extend(block_entity("New", new_key.position()));
    let decoded =
        DecodedLevelChunk::decode_with_biomes_and_block_entities(chunk, -4, 1, -4, 1, &valid)
            .unwrap();
    assert_eq!(decoded.bytes_consumed(), valid.len());
    store.commit_level_chunk(chunk, decoded);
    assert_eq!(
        store.sub_chunk(sub_chunk).unwrap().runtime_id(0, 0, 0, 0),
        Some(9)
    );
    assert_eq!(store.biome_id(sub_chunk, 0, 0, 0), Some(6));
    assert!(store.block_entity(old_key).is_none());
    assert_eq!(store.block_entity(new_key).unwrap().id(), Some("New"));
}

#[test]
fn sub_chunk_payload_replaces_only_its_sparse_entity_slice_atomically() {
    let key = SubChunkKey::new(0, 4, -4, -7);
    let here = BlockEntityKey::new(0, 65, -63, -111);
    let other = BlockEntityKey::new(0, 66, -47, -110);
    let mut store = ChunkStore::new();
    let other_nbt =
        DecodedBlockEntities::decode_live(other, &block_entity("Other", other.position())).unwrap();
    store.commit_block_entity_update(other, other_nbt).unwrap();

    let mut first_payload = uniform_sub_chunk(-4, 20);
    first_payload.extend(block_entity("Chest", here.position()));
    let first = DecodedSubChunk::decode(key, &first_payload).unwrap();
    store.commit_decoded_sub_chunk(key, first).unwrap();
    let before_block = store.sub_chunk(key).unwrap();
    let before_here = store.block_entity(here).unwrap();

    let mut malformed = uniform_sub_chunk(-4, 21);
    malformed.extend_from_slice(&[10, 0, 8]);
    assert!(DecodedSubChunk::decode(key, &malformed).is_err());
    assert!(Arc::ptr_eq(&before_block, &store.sub_chunk(key).unwrap()));
    assert!(Arc::ptr_eq(
        &before_here,
        &store.block_entity(here).unwrap()
    ));
    assert_eq!(store.block_entity(other).unwrap().id(), Some("Other"));

    let empty = DecodedSubChunk::decode(key, &uniform_sub_chunk(-4, 22)).unwrap();
    store.commit_decoded_sub_chunk(key, empty).unwrap();
    assert!(store.block_entity(here).is_none());
    assert_eq!(store.block_entity(other).unwrap().id(), Some("Other"));

    let mut restored = uniform_sub_chunk(-4, 23);
    restored.extend(block_entity("Chest", here.position()));
    store
        .commit_decoded_sub_chunk(key, DecodedSubChunk::decode(key, &restored).unwrap())
        .unwrap();
    assert!(store.block_entity(here).is_some());
    assert_eq!(store.apply_all_air(key), Some(key));
    assert!(store.block_entity(here).is_none());
    assert_eq!(store.block_entity(other).unwrap().id(), Some("Other"));
}

#[test]
fn live_update_uses_packet_position_and_rejects_conflicting_embedded_position() {
    let key = BlockEntityKey::new(2, -1, 70, 31);
    let mut store = ChunkStore::new();
    let decoded =
        DecodedBlockEntities::decode_live(key, &block_entity("Sign", key.position())).unwrap();
    assert!(store.commit_block_entity_update(key, decoded).unwrap());
    let before = store.block_entity(key).unwrap();

    let equal =
        DecodedBlockEntities::decode_live(key, &block_entity("Sign", key.position())).unwrap();
    assert!(!store.commit_block_entity_update(key, equal).unwrap());
    assert!(Arc::ptr_eq(&before, &store.block_entity(key).unwrap()));

    let conflict = block_entity("Sign", [-1, 71, 31]);
    assert!(matches!(
        DecodedBlockEntities::decode_live(key, &conflict),
        Err(BlockEntityError::PositionMismatch { .. })
    ));
    assert!(Arc::ptr_eq(&before, &store.block_entity(key).unwrap()));
}

#[test]
fn entity_only_columns_are_sparse_and_evict_with_the_chunk() {
    let chunk = ChunkKey::new(1, -4, 8);
    let key = BlockEntityKey::new(1, -64, 90, 128);
    let mut store = ChunkStore::new();
    let decoded =
        DecodedBlockEntities::decode_live(key, &block_entity("Note", key.position())).unwrap();
    store.commit_block_entity_update(key, decoded).unwrap();

    assert!(store.chunk(chunk).is_some());
    assert!(store.sub_chunk(SubChunkKey::from_chunk(chunk, 5)).is_none());
    assert!(store.block_entity(key).is_some());
    assert!(store.evict_chunk(chunk).is_empty());
    assert!(store.chunk(chunk).is_none());
    assert!(store.block_entity(key).is_none());
}

#[test]
fn live_updates_enforce_per_chunk_record_and_cumulative_byte_limits() {
    let mut store = ChunkStore::new();
    for y in 0..MAX_BLOCK_ENTITIES_PER_CHUNK {
        let y = i32::try_from(y).unwrap();
        let key = BlockEntityKey::new(0, 0, y, 0);
        let decoded = DecodedBlockEntities::decode_live(key, &[10, 0, 0]).unwrap();
        store.commit_block_entity_update(key, decoded).unwrap();
    }
    let overflow_key = BlockEntityKey::new(0, 0, MAX_BLOCK_ENTITIES_PER_CHUNK as i32, 0);
    let overflow = DecodedBlockEntities::decode_live(overflow_key, &[10, 0, 0]).unwrap();
    assert_eq!(
        store
            .commit_block_entity_update(overflow_key, overflow)
            .unwrap_err(),
        BlockEntityError::TooManyEntities {
            max: MAX_BLOCK_ENTITIES_PER_CHUNK
        }
    );

    let encoded = large_positionless_block_entity();
    assert!(encoded.len() < MAX_BLOCK_ENTITY_BYTES_PER_CHUNK);
    let mut store = ChunkStore::new();
    let mut committed = 0_usize;
    loop {
        let key = BlockEntityKey::new(0, 0, committed as i32, 0);
        let decoded = DecodedBlockEntities::decode_live(key, &encoded).unwrap();
        match store.commit_block_entity_update(key, decoded) {
            Ok(true) => committed += 1,
            Err(BlockEntityError::ChunkEntityBytesTooLarge { len, max }) => {
                assert!(len > max);
                assert_eq!(max, MAX_BLOCK_ENTITY_BYTES_PER_CHUNK);
                break;
            }
            result => panic!("unexpected bounded live-update result: {result:?}"),
        }
    }
    assert!(committed > 1);
    assert!(committed < MAX_BLOCK_ENTITIES_PER_CHUNK);
}

#[test]
fn cumulative_sub_chunk_tails_cannot_bypass_the_chunk_byte_limit() {
    let mut store = ChunkStore::new();
    for sub_y in [-4, -3] {
        let key = SubChunkKey::new(0, 0, sub_y, 0);
        let mut payload = uniform_sub_chunk(sub_y as i8, 7);
        for local_x in 0..4 {
            payload.extend(large_block_entity([local_x, sub_y * 16, 0]));
        }
        store
            .commit_decoded_sub_chunk(key, DecodedSubChunk::decode(key, &payload).unwrap())
            .unwrap();
    }

    let rejected_key = SubChunkKey::new(0, 0, -2, 0);
    let rejected_entity = BlockEntityKey::new(0, 0, -32, 0);
    let mut payload = uniform_sub_chunk(-2, 9);
    payload.extend(large_block_entity(rejected_entity.position()));
    let error = store
        .commit_decoded_sub_chunk(
            rejected_key,
            DecodedSubChunk::decode(rejected_key, &payload).unwrap(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        world::DecodeError::BlockEntity(BlockEntityError::ChunkEntityBytesTooLarge { .. })
    ));
    assert!(store.sub_chunk(rejected_key).is_none());
    assert!(store.block_entity(rejected_entity).is_none());
}

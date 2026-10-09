use std::cell::Cell;

use render::BlockEntityKind;
use world::{BLOCKS_PER_SUB_CHUNK, ChunkStore, RawBlockIds, SubChunkKey};

use super::*;

/// The sub-chunk holding every test entity, at blocks y 64..80.
const SECTION: SubChunkKey = SubChunkKey::new(0, 0, 4, 0);

/// A sub-chunk whose every cell is `id`.
fn uniform_sub_chunk(id: u8) -> SubChunk {
    let mut bytes = vec![8, 1, 3];
    bytes.resize(bytes.len() + BLOCKS_PER_SUB_CHUNK / 8, 0);
    // Palette of two zigzag ids; every cell selects the first.
    bytes.extend_from_slice(&[4, id * 2, (id + 1) * 2]);
    SubChunk::decode(&bytes, &RawBlockIds { air: 0 })
}

fn varint(mut value: u32, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn string(value: &str, out: &mut Vec<u8>) {
    varint(value.len() as u32, out);
    out.extend_from_slice(value.as_bytes());
}

/// Network NBT for a block entity of `id` at `position` carrying `text`.
fn entity_nbt(id: &str, position: [i32; 3], text: &str) -> BlockEntityNbt {
    let mut out = vec![10, 0];
    for (name, value) in [("id", id), ("Text", text)] {
        out.push(8);
        string(name, &mut out);
        string(value, &mut out);
    }
    for (name, value) in ["x", "y", "z"].into_iter().zip(position) {
        out.push(3);
        string(name, &mut out);
        varint(((value << 1) ^ (value >> 31)) as u32, &mut out);
    }
    out.push(0);
    BlockEntityNbt::decode_prefix(&out).unwrap().0
}

fn key(x: i32) -> BlockEntityKey {
    BlockEntityKey::new(0, x, 65, 1)
}

fn store_with(entities: &[(BlockEntityKey, &str, &str)]) -> ChunkStore {
    let mut store = ChunkStore::new();
    store
        .commit_sub_chunk(SECTION, uniform_sub_chunk(1))
        .unwrap();
    for &(key, id, text) in entities {
        store
            .commit_block_entity_update(key, entity_nbt(id, key.position(), text))
            .unwrap();
    }
    store
}

/// Signs get a template; every other routed entity describes to none.
fn counting(
    calls: &Cell<usize>,
) -> impl FnMut(&str, u32, &BlockEntityNbt, [i32; 3]) -> Option<Template> + '_ {
    |id, _, _, _| {
        calls.set(calls.get() + 1);
        (id == "Sign").then_some(Template::Static(BlockEntityKind::EndPortal))
    }
}

#[test]
fn a_column_scan_stays_current_until_its_blocks_or_block_entities_change() {
    let sign = key(1);
    let mut store = store_with(&[(sign, "Sign", "first")]);
    let column = sign.chunk();
    let calls = Cell::new(0);
    let chunk = store.chunk(column).unwrap();
    let scan = ColumnScan::new(
        chunk,
        Vec::new(),
        routed_entities(chunk, None, counting(&calls)),
    );
    assert!(scan.is_current(store.chunk(column).unwrap()));

    store
        .commit_block_entity_update(sign, entity_nbt("Sign", sign.position(), "first"))
        .unwrap();
    assert!(
        scan.is_current(store.chunk(column).unwrap()),
        "an identical update keeps the scan"
    );

    store
        .commit_block_entity_update(sign, entity_nbt("Sign", sign.position(), "second"))
        .unwrap();
    assert!(!scan.is_current(store.chunk(column).unwrap()));

    let chunk = store.chunk(column).unwrap();
    let scan = ColumnScan::new(
        chunk,
        Vec::new(),
        routed_entities(chunk, Some(scan), counting(&calls)),
    );
    assert!(scan.is_current(store.chunk(column).unwrap()));
    store
        .commit_sub_chunk(SECTION, uniform_sub_chunk(2))
        .unwrap();
    assert!(!scan.is_current(store.chunk(column).unwrap()));
}

#[test]
fn rebuilding_a_column_describes_only_the_entities_that_changed() {
    let (first, second, banner) = (key(1), key(2), key(3));
    let mut store = store_with(&[
        (first, "Sign", "first"),
        (second, "Sign", "second"),
        (banner, "Banner", ""),
        (key(4), "NotABlockEntity", ""),
    ]);
    let column = first.chunk();
    let calls = Cell::new(0);
    let chunk = store.chunk(column).unwrap();
    let scan = ColumnScan::new(
        chunk,
        Vec::new(),
        routed_entities(chunk, None, counting(&calls)),
    );
    assert_eq!(calls.get(), 3, "unrouted entities are never described");
    assert_eq!(
        scan.entities
            .iter()
            .map(|entity| entity.key)
            .collect::<Vec<_>>(),
        [first, second, banner]
    );

    store
        .commit_block_entity_update(first, entity_nbt("Sign", first.position(), "edited"))
        .unwrap();
    calls.set(0);
    let rebuilt = routed_entities(store.chunk(column).unwrap(), Some(scan), counting(&calls));
    assert_eq!(calls.get(), 1, "only the edited sign is described again");
    let drawn: Vec<_> = rebuilt
        .iter()
        .filter(|entity| entity.template.is_some())
        .map(|entity| entity.key)
        .collect();
    assert_eq!(drawn, [first, second]);
}

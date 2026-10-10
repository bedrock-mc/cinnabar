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

/// Streaming a lobby in invalidates every nearby column at once: a frame rescans a bounded
/// number, the rest keep their previous scan or wait, and later frames finish the rest.
#[test]
fn a_frame_rescans_a_bounded_number_of_columns() {
    let columns = 3 * MAX_COLUMN_RESCANS_PER_FRAME;
    let mut store = ChunkStore::new();
    let commit_all = |store: &mut ChunkStore, id: u8| {
        for x in 0..columns as i32 {
            store
                .commit_sub_chunk(SubChunkKey::new(0, x, 4, 0), uniform_sub_chunk(id))
                .unwrap();
        }
    };
    commit_all(&mut store, 1);
    let mut scans: Vec<Option<ColumnScan>> = (0..columns).map(|_| None).collect();
    let rescans = Cell::new(0);
    let frame = |store: &ChunkStore, scans: &mut Vec<Option<ColumnScan>>| {
        let mut left = MAX_COLUMN_RESCANS_PER_FRAME;
        for (x, scan) in scans.iter_mut().enumerate() {
            let chunk = store.chunk(world::ChunkKey::new(0, x as i32, 0)).unwrap();
            *scan = frame_scan(scan.take(), chunk, false, &mut left, |previous| {
                rescans.set(rescans.get() + 1);
                ColumnScan::new(
                    chunk,
                    Vec::new(),
                    routed_entities(chunk, previous, |_, _, _, _| None),
                )
            });
        }
        rescans.replace(0)
    };
    assert_eq!(frame(&store, &mut scans), MAX_COLUMN_RESCANS_PER_FRAME);
    assert_eq!(scans.iter().flatten().count(), MAX_COLUMN_RESCANS_PER_FRAME);
    assert_eq!(frame(&store, &mut scans), MAX_COLUMN_RESCANS_PER_FRAME);
    assert_eq!(frame(&store, &mut scans), MAX_COLUMN_RESCANS_PER_FRAME);
    assert_eq!(frame(&store, &mut scans), 0, "every column is current");
    commit_all(&mut store, 2);
    assert_eq!(frame(&store, &mut scans), MAX_COLUMN_RESCANS_PER_FRAME);
    assert!(
        scans.iter().all(Option::is_some),
        "stale columns keep their scan"
    );
}

/// The player's own columns rescan on every change even after streaming spent the budget, so an
/// edit within reach shows in the frame it commits.
#[test]
fn near_columns_rescan_past_a_spent_budget() {
    let mut store = ChunkStore::new();
    store
        .commit_sub_chunk(SECTION, uniform_sub_chunk(1))
        .unwrap();
    let key = world::ChunkKey::new(0, 0, 0);
    let rescan = |chunk: &Chunk, previous| {
        ColumnScan::new(
            chunk,
            Vec::new(),
            routed_entities(chunk, previous, |_, _, _, _| None),
        )
    };
    let mut spent = 0;
    let chunk = store.chunk(key).unwrap();
    let scan = frame_scan(None, chunk, true, &mut spent, |previous| {
        rescan(chunk, previous)
    })
    .expect("a near column scans without budget");
    store
        .commit_sub_chunk(SECTION, uniform_sub_chunk(2))
        .unwrap();
    let chunk = store.chunk(key).unwrap();
    assert!(!scan.is_current(chunk));
    let scan = frame_scan(Some(scan), chunk, true, &mut spent, |previous| {
        rescan(chunk, previous)
    })
    .unwrap();
    assert!(scan.is_current(chunk), "the edited near column rescans");
    let far = frame_scan(None, chunk, false, &mut spent, |previous| {
        rescan(chunk, previous)
    });
    assert!(far.is_none(), "a far column waits for budget");
    assert!(is_near_column([0, 0], 1, -1));
    assert!(!is_near_column([0, 0], 2, 0));
}

/// Repeated edits to early columns cannot starve later columns waiting for their first scan.
#[test]
fn continuously_changed_columns_do_not_starve_later_rescans() {
    let count = 3 * MAX_COLUMN_RESCANS_PER_FRAME;
    let mut store = ChunkStore::new();
    for x in 0..count as i32 {
        store
            .commit_sub_chunk(SubChunkKey::new(0, x, 4, 0), uniform_sub_chunk(1))
            .unwrap();
    }
    let mut scans: Vec<Option<ColumnScan>> = (0..count).map(|_| None).collect();
    let mut order = RescanOrder::default();
    for frame in 0..count.div_ceil(MAX_COLUMN_RESCANS_PER_FRAME) {
        for x in 0..MAX_COLUMN_RESCANS_PER_FRAME as i32 {
            store
                .commit_sub_chunk(
                    SubChunkKey::new(0, x, 4, 0),
                    uniform_sub_chunk(2 + frame as u8),
                )
                .unwrap();
        }
        let selected = order.select(
            (0..count as i32)
                .map(|x| world::ChunkKey::new(0, x, 0))
                .filter(|key| {
                    let chunk = store.chunk(*key).unwrap();
                    scans[key.x as usize]
                        .as_ref()
                        .is_none_or(|scan| !scan.is_current(chunk))
                }),
        );
        let mut left = MAX_COLUMN_RESCANS_PER_FRAME;
        for (x, scan) in scans.iter_mut().enumerate() {
            let key = world::ChunkKey::new(0, x as i32, 0);
            let chunk = store.chunk(key).unwrap();
            let mut deferred = 0;
            let budget = if selected.contains(&Some(key)) {
                &mut left
            } else {
                &mut deferred
            };
            *scan = frame_scan(scan.take(), chunk, false, budget, |previous| {
                ColumnScan::new(
                    chunk,
                    Vec::new(),
                    routed_entities(chunk, previous, |_, _, _, _| None),
                )
            });
        }
    }
    assert!(
        scans.iter().all(Option::is_some),
        "every distant column eventually appears"
    );
}

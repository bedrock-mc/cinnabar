use std::{fs, num::NonZeroUsize, path::Path};

use lod::registry::MaterialKey;

use super::*;
use crate::{
    access::RegionRef,
    epochs::{EpochId, EpochRef, EpochState, MAX_EPOCHS_PER_STORE},
    error::StoreError,
    identity::Dimension,
    lock::LOCK_FILE,
    minute::Minute,
    region_id::RegionId,
    test_support::{flat_surface, flip_byte, key, record, surface},
};

const OVERWORLD: Dimension = Dimension(0);
const NETHER: Dimension = Dimension(1);

fn open(dir: &Path, now: u32) -> Store {
    Store::open(dir, Minute::new(now), StoreOptions::default()).expect("open store")
}

fn epoch(dimension: Dimension, id: u32) -> EpochRef {
    EpochRef {
        dimension,
        id: EpochId(id),
    }
}

/// Records world `seed`'s surface over `regions` signature regions in a row, every
/// chunk of each, returning the last status.
fn record_world(store: &mut Store, dimension: Dimension, seed: u32, regions: i32) -> EpochStatus {
    let mut status = store.epoch_status(dimension).expect("session");
    for region in 0..regions {
        for chunk in 0..16 {
            let (x, z) = (region * 4 + chunk % 4, chunk / 4);
            status = store
                .record_surface(dimension, x, z, surface(seed, x, z), Minute::new(1))
                .expect("record surface");
        }
    }
    status
}

/// A store whose overworld holds world 1 over `regions` signature regions and one record.
fn seeded_store(dir: &Path, regions: i32) {
    let mut store = open(dir, 1);
    assert_eq!(
        store
            .begin_session(OVERWORLD, Minute::new(1))
            .expect("begin"),
        EpochStatus::Established(epoch(OVERWORLD, 0))
    );
    record_world(&mut store, OVERWORLD, 1, regions);
    store
        .put(OVERWORLD, &record(key(0, 1, 2, 3), 1, 4), Minute::new(1))
        .expect("put");
    assert!(store.flush());
    store.close();
}

#[test]
fn records_hashes_and_registry_survive_reopening() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = open(dir.path(), 5);
    assert_eq!(store.access(), Access::ReadWrite);
    let material = store
        .registry()
        .write()
        .expect("registry")
        .get_or_insert(&MaterialKey::Network(77))
        .expect("material");
    store
        .begin_session(OVERWORLD, Minute::new(5))
        .expect("begin");
    let bytes = record(key(0, -40, 1, 9), 3, 6);
    store.put(OVERWORLD, &bytes, Minute::new(6)).expect("put");
    store
        .set_chunk_hash(OVERWORLD, -80, 18, Some(99))
        .expect("hash");
    let mut out = Vec::new();
    assert!(
        store
            .get_into(OVERWORLD, key(0, -40, 1, 9), &mut out, Minute::new(6))
            .expect("get")
    );
    assert_eq!(out, bytes, "readable before a flush");
    assert!(store.flush());
    store.close();

    let mut store = open(dir.path(), 9);
    assert_eq!(
        store
            .registry()
            .read()
            .expect("registry")
            .get(&MaterialKey::Network(77)),
        Some(material)
    );
    assert_eq!(store.opened(), Minute::new(9));
    // A second visit is a candidate until its votes decide, reading the stored epoch's
    // records through.
    assert!(
        store
            .begin_session(OVERWORLD, Minute::new(9))
            .expect("begin")
            .is_candidate()
    );
    let established = epoch(OVERWORLD, 0);
    assert_eq!(
        store.access_table().region(RegionRef {
            epoch: established,
            region: RegionId::of_key(key(0, -40, 1, 9)).0
        }),
        Minute::new(6)
    );
    assert!(
        store
            .get_into(OVERWORLD, key(0, -40, 1, 9), &mut out, Minute::new(9))
            .expect("get")
    );
    assert_eq!(out, bytes);
    let regions = store.files(established).expect("files");
    assert_eq!(
        regions.regions.get(key(0, -40, 1, 9)).expect("get"),
        Some(bytes)
    );
    assert_eq!(regions.regions.chunk_hash(-80, 18).expect("hash"), Some(99));
    assert_eq!(
        store.access_table().region(RegionRef {
            epoch: established,
            region: RegionId::of_key(key(0, -40, 1, 9)).0
        }),
        Minute::new(9),
        "the read-through used the stored region"
    );
}

#[test]
fn a_second_opener_is_read_only_and_never_writes() {
    let dir = tempfile::tempdir().expect("temp dir");
    seeded_store(dir.path(), 1);
    let writer = open(dir.path(), 2);
    let listing = |dir: &Path| {
        let mut names: Vec<_> = walk(dir);
        names.sort();
        names
    };
    let before = listing(dir.path());
    let mut reader = open(dir.path(), 3);
    assert_eq!(reader.access(), Access::ReadOnly);
    reader
        .registry()
        .write()
        .expect("registry")
        .get_or_insert(&MaterialKey::Network(5))
        .expect("material");
    let status = reader
        .begin_session(OVERWORLD, Minute::new(3))
        .expect("begin");
    assert!(status.is_candidate());
    assert!(matches!(
        reader.put(OVERWORLD, &record(key(0, 0, 0, 0), 1, 2), Minute::new(3)),
        Err(StoreError::ReadOnly)
    ));
    assert!(matches!(
        reader.set_chunk_hash(OVERWORLD, 0, 0, Some(1)),
        Err(StoreError::ReadOnly)
    ));
    // A read-only session still votes in memory and reads the epoch it merges into.
    assert_eq!(
        record_world(&mut reader, OVERWORLD, 1, 1),
        EpochStatus::Candidate(status.epoch()),
        "one region cannot decide"
    );
    assert!(reader.flush());
    assert_eq!(reader.evict_regions(0, None).expect("evict"), 0);
    reader.close();
    assert_eq!(
        listing(dir.path()),
        before,
        "the reader changed nothing on disk"
    );
    drop(writer);
}

/// Every file below `dir` with its length.
fn walk(dir: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push((
                path.display().to_string(),
                entry.metadata().expect("metadata").len(),
            ));
        }
    }
    out
}

#[test]
fn the_same_world_merges_into_its_stored_epoch() {
    let dir = tempfile::tempdir().expect("temp dir");
    seeded_store(dir.path(), 12);
    let mut store = open(dir.path(), 10);
    let candidate = store
        .begin_session(OVERWORLD, Minute::new(10))
        .expect("begin");
    assert_eq!(candidate, EpochStatus::Candidate(epoch(OVERWORLD, 1)));
    let fresh = record(key(0, 5, 0, 5), 9, 3);
    store.put(OVERWORLD, &fresh, Minute::new(10)).expect("put");
    store
        .set_chunk_hash(OVERWORLD, 7, 7, Some(3))
        .expect("hash");
    assert_eq!(
        record_world(&mut store, OVERWORLD, 1, 12),
        EpochStatus::Merged {
            from: epoch(OVERWORLD, 1),
            into: epoch(OVERWORLD, 0)
        }
    );
    assert!(!epoch(OVERWORLD, 1).dir(dir.path()).exists());
    assert_eq!(store.catalog().entries().len(), 1);
    let mut out = Vec::new();
    for (key, expected) in [
        (key(0, 5, 0, 5), fresh),
        (key(0, 1, 2, 3), record(key(0, 1, 2, 3), 1, 4)),
    ] {
        assert!(
            store
                .get_into(OVERWORLD, key, &mut out, Minute::new(10))
                .expect("get")
        );
        assert_eq!(out, expected, "candidate and stored records both readable");
    }
    assert_eq!(store.chunk_hash(OVERWORLD, 7, 7).expect("hash"), Some(3));
    let events = store.drain_events();
    assert!(events.iter().any(|event| matches!(
        event,
        StoreEvent::Epoch {
            status: EpochStatus::Merged { .. },
            ..
        }
    )));
    store.close();
    let reopened = open(dir.path(), 11);
    assert_eq!(reopened.catalog().entries().len(), 1, "the merge persisted");
}

/// A level-0 record of `key` that knows only the block columns from `from_x` up to half
/// the section's width, filled with material `material` under biome `biome`.
fn half_record(
    key: lod::geometry::SectionKey,
    from_x: usize,
    material: u32,
    biome: u32,
) -> Vec<u8> {
    use lod::{
        biome_grid::UNKNOWN_BIOME,
        geometry::SECTION_SIDE,
        section::{ChildMask, Section},
        storage::{DenseSection, PackScratch},
        voxel::{FULL_COVERAGE, Light, MaterialId, Voxel},
    };
    let voxel = Voxel::new(
        MaterialId::new(material).expect("block material"),
        Light::DARK,
        FULL_COVERAGE,
        false,
    );
    let mut dense = DenseSection::filled(Voxel::AIR, UNKNOWN_BIOME);
    for z in 0..SECTION_SIDE {
        for x in from_x..from_x + SECTION_SIDE / 2 {
            dense.biomes_mut().set(x, z, biome);
            for y in 0..SECTION_SIDE {
                dense.set(x, y, z, voxel);
            }
        }
    }
    let section = Section::from_dense(
        key.pos(),
        &dense,
        ChildMask::UNKNOWN,
        &mut PackScratch::default(),
    )
    .expect("level 0");
    lod::codec::SectionCodec::new().encode(&section)
}

#[test]
fn a_candidate_writes_only_its_own_terrain_and_merges_it_over_the_stored_records() {
    let dir = tempfile::tempdir().expect("temp dir");
    seeded_store(dir.path(), 12);
    // The seeded record knows every column of its section.
    let shared = key(0, 1, 2, 3);
    let decode = |bytes: &[u8]| {
        let section = lod::codec::SectionCodec::new()
            .decode_expecting(bytes, shared)
            .expect("valid record");
        let mut dense = lod::mip::unknown_section();
        section.expand_into(&mut dense);
        dense
    };
    let stored = decode(&record(shared, 1, 4));
    let mut store = open(dir.path(), 10);
    assert!(
        store
            .begin_session(OVERWORLD, Minute::new(10))
            .expect("begin")
            .is_candidate()
    );
    let mut out = Vec::new();
    assert!(
        !store
            .get_own_into(OVERWORLD, shared, &mut out, Minute::new(10))
            .expect("get"),
        "a writer merging into the candidate never sees the stored epoch's record"
    );
    assert!(
        store
            .get_into(OVERWORLD, shared, &mut out, Minute::new(10))
            .expect("get")
    );
    let sampled = half_record(shared, 16, 5, 6);
    store
        .put(OVERWORLD, &sampled, Minute::new(10))
        .expect("put");
    assert!(
        store
            .get_own_into(OVERWORLD, shared, &mut out, Minute::new(10))
            .expect("get")
    );
    assert_eq!(out, sampled, "the candidate holds only its own columns");
    assert!(matches!(
        record_world(&mut store, OVERWORLD, 1, 12),
        EpochStatus::Merged { .. }
    ));
    assert!(
        store
            .get_into(OVERWORLD, shared, &mut out, Minute::new(10))
            .expect("get")
    );
    let merged = decode(&out);
    for x in [0, 15] {
        assert_eq!(merged.get(x, 9, 4), stored.get(x, 9, 4), "stored half kept");
        assert_eq!(merged.biomes().get(x, 4), stored.biomes().get(x, 4));
    }
    assert_eq!(
        merged.get(31, 9, 4).material().raw(),
        5,
        "sampled half laid over"
    );
    assert_eq!(merged.biomes().get(31, 4), 6);
}

#[test]
fn an_undecided_session_reads_stored_records_through_but_never_hashes() {
    let dir = tempfile::tempdir().expect("temp dir");
    seeded_store(dir.path(), 4);
    let stored_key = key(0, 1, 2, 3);
    let stored = record(stored_key, 1, 4);
    let mut store = open(dir.path(), 2);
    store
        .begin_session(OVERWORLD, Minute::new(2))
        .expect("begin");
    store
        .set_chunk_hash(OVERWORLD, 2, 6, Some(8))
        .expect("hash");
    store.close();

    let mut store = open(dir.path(), 10);
    assert_eq!(store.leading_epoch(OVERWORLD), None, "no session yet");
    let status = store
        .begin_session(OVERWORLD, Minute::new(10))
        .expect("begin");
    assert!(status.is_candidate());
    assert_eq!(store.leading_epoch(OVERWORLD), Some(epoch(OVERWORLD, 0)));
    let mut out = Vec::new();
    assert!(
        store
            .get_into(OVERWORLD, stored_key, &mut out, Minute::new(10))
            .expect("get")
    );
    assert_eq!(out, stored, "a revisit shows stored terrain at once");
    assert_eq!(
        store.chunk_hash(OVERWORLD, 2, 6).expect("hash"),
        None,
        "every chunk is sampled again while its samples are votes"
    );
    let newer = record(stored_key, 2, 4);
    store.put(OVERWORLD, &newer, Minute::new(10)).expect("put");
    assert!(
        store
            .get_into(OVERWORLD, stored_key, &mut out, Minute::new(10))
            .expect("get")
    );
    assert_eq!(out, newer, "the candidate's own record wins");
    assert!(
        !store
            .get_into(OVERWORLD, key(0, 9, 9, 9), &mut out, Minute::new(10))
            .expect("get")
    );
}

#[test]
fn a_different_world_splits_into_a_new_epoch() {
    let dir = tempfile::tempdir().expect("temp dir");
    seeded_store(dir.path(), 110);
    let mut store = open(dir.path(), 10);
    store
        .begin_session(OVERWORLD, Minute::new(10))
        .expect("begin");
    assert_eq!(
        record_world(&mut store, OVERWORLD, 2, 110),
        EpochStatus::Established(epoch(OVERWORLD, 1))
    );
    let states: Vec<_> = store.catalog().entries().map(|entry| entry.state).collect();
    assert_eq!(states, [EpochState::Established; 2]);
    assert_eq!(store.leading_epoch(OVERWORLD), None, "decided");
    let mut out = Vec::new();
    assert!(
        !store
            .get_into(OVERWORLD, key(0, 1, 2, 3), &mut out, Minute::new(10))
            .expect("get"),
        "the other world's records stay apart"
    );
    store.close();
    let mut store = open(dir.path(), 11);
    assert_eq!(store.catalog().established(OVERWORLD).count(), 2);
    assert!(
        store
            .begin_session(OVERWORLD, Minute::new(11))
            .expect("begin")
            .is_candidate()
    );
}

#[test]
fn undecided_candidates_are_dropped_with_their_data() {
    let dir = tempfile::tempdir().expect("temp dir");
    seeded_store(dir.path(), 4);
    let mut store = open(dir.path(), 10);
    let first = store
        .begin_session(OVERWORLD, Minute::new(10))
        .expect("begin")
        .epoch();
    store
        .put(OVERWORLD, &record(key(0, 0, 0, 0), 1, 2), Minute::new(10))
        .expect("put");
    // Flat terrain abstains, so the candidate never decides.
    for x in 0..64 {
        let status = store
            .record_surface(OVERWORLD, x, 0, flat_surface(4), Minute::new(10))
            .expect("record");
        assert!(status.is_candidate());
    }
    assert!(store.flush());
    assert!(first.dir(dir.path()).exists());
    // Beginning again, as after a dimension change, drops the old candidate.
    let second = store
        .begin_session(OVERWORLD, Minute::new(10))
        .expect("begin")
        .epoch();
    assert_ne!(first, second);
    assert!(!first.dir(dir.path()).exists());
    store
        .put(OVERWORLD, &record(key(0, 0, 0, 0), 1, 2), Minute::new(10))
        .expect("put");
    assert!(store.flush());
    let events = store.close();
    assert!(
        !second.dir(dir.path()).exists(),
        "closing drops the undecided candidate"
    );
    let dropped = events
        .iter()
        .filter(|event| matches!(event, StoreEvent::CandidateDropped(_)))
        .count();
    assert_eq!(dropped, 2);
    assert_eq!(open(dir.path(), 11).catalog().entries().len(), 1);
}

#[test]
fn candidates_left_by_a_crash_are_dropped_on_open() {
    let dir = tempfile::tempdir().expect("temp dir");
    seeded_store(dir.path(), 4);
    let mut store = open(dir.path(), 10);
    let candidate = store
        .begin_session(OVERWORLD, Minute::new(10))
        .expect("begin")
        .epoch();
    store
        .put(OVERWORLD, &record(key(0, 0, 0, 0), 1, 2), Minute::new(10))
        .expect("put");
    assert!(store.flush());
    // A stray epoch directory the catalog never listed.
    let stray = epoch(NETHER, 40).dir(dir.path());
    fs::create_dir_all(&stray).expect("stray dir");
    drop(store);

    let mut store = open(dir.path(), 11);
    assert!(!candidate.dir(dir.path()).exists());
    assert!(!stray.exists());
    assert_eq!(store.catalog().entries().len(), 1);
    assert!(store.drain_events().iter().any(
        |event| matches!(event, StoreEvent::CandidateDropped(dropped) if *dropped == candidate)
    ));
}

#[test]
fn an_unreadable_registry_resets_the_store() {
    let dir = tempfile::tempdir().expect("temp dir");
    seeded_store(dir.path(), 1);
    let mut store = open(dir.path(), 2);
    store
        .registry()
        .write()
        .expect("registry")
        .get_or_insert(&MaterialKey::Network(1))
        .expect("material");
    assert!(store.flush());
    drop(store);
    flip_byte(&dir.path().join(MATERIALS_FILE), 8);
    let stray = dir.path().join("future-table");
    fs::write(&stray, b"a table this version does not know").expect("stray table");

    // A reader cannot use the store at all.
    let writer = StoreLock::try_acquire(dir.path())
        .expect("lock")
        .expect("free");
    assert!(matches!(
        Store::open(dir.path(), Minute::new(3), StoreOptions::default()),
        Err(StoreError::Registry(_))
    ));
    drop(writer);

    let mut store = open(dir.path(), 3);
    assert!(store.registry().read().expect("registry").is_empty());
    assert_eq!(store.catalog().entries().len(), 0);
    assert!(!dir.path().join(OVERWORLD.dir_name()).exists());
    assert!(dir.path().join(LOCK_FILE).exists());
    assert!(!stray.exists(), "a reset removes every table, known or not");
    assert!(matches!(
        store.drain_events().as_slice(),
        [StoreEvent::Reset(ResetReason::Registry(_))]
    ));
}

#[test]
fn unreadable_catalogs_and_access_tables_are_rebuilt() {
    let dir = tempfile::tempdir().expect("temp dir");
    seeded_store(dir.path(), 1);
    let catalog = dir.path().join(crate::epochs::CATALOG_FILE);
    let access = dir.path().join(crate::access::ACCESS_FILE);
    flip_byte(&catalog, 7);
    fs::write(&access, b"junk").expect("damage access table");
    let mut store = open(dir.path(), 20);
    assert_eq!(
        store.catalog().established(OVERWORLD).collect::<Vec<_>>(),
        [epoch(OVERWORLD, 0)]
    );
    assert_eq!(store.opened(), Minute::new(20));
    let events = store.drain_events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StoreEvent::CatalogRebuilt(_)))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StoreEvent::AccessReset(_)))
    );
    let next = store.begin_session(NETHER, Minute::new(20)).expect("begin");
    assert_eq!(
        next.epoch().id,
        EpochId(1),
        "rebuilt catalogs never reuse ids"
    );
}

#[test]
fn established_epochs_are_bounded_per_store() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = open(dir.path(), 0);
    // Each new dimension's first visit establishes an epoch at once.
    for dimension in 0..=MAX_EPOCHS_PER_STORE as i32 {
        let status = store
            .begin_session(Dimension(dimension), Minute::new(dimension as u32))
            .expect("begin");
        assert!(matches!(status, EpochStatus::Established(_)));
        store
            .put(
                Dimension(dimension),
                &record(key(0, 0, 0, 0), 1, 2),
                Minute::new(0),
            )
            .expect("put");
        store.end_session(Dimension(dimension));
    }
    assert_eq!(store.catalog().entries().len(), MAX_EPOCHS_PER_STORE);
    assert!(
        store.catalog().get(epoch(Dimension(0), 0)).is_none(),
        "oldest evicted"
    );
    assert!(!epoch(Dimension(0), 0).dir(dir.path()).exists());
    assert!(store
        .drain_events()
        .iter()
        .any(|event| matches!(event, StoreEvent::EpochEvicted(evicted) if *evicted == epoch(Dimension(0), 0))));
}

#[test]
fn eviction_takes_unseen_then_far_regions_with_their_signatures() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = open(dir.path(), 0);
    store
        .begin_session(OVERWORLD, Minute::new(0))
        .expect("begin");
    store.begin_session(NETHER, Minute::new(0)).expect("begin");
    let at = |region_x: i32| key(0, region_x * 32, 0, 0);
    // Region 0 is nearest and was used first; region 9 is farthest and used last.
    for (region_x, minute) in [(0, 10), (5, 20), (9, 30)] {
        for y in 0..8 {
            let key = key(0, region_x * 32, y, 0);
            store
                .put(OVERWORLD, &record(key, y as u32, 200), Minute::new(minute))
                .expect("put");
        }
        let chunk_x = region_x * 64;
        store
            .record_surface(
                OVERWORLD,
                chunk_x,
                0,
                surface(1, chunk_x, 0),
                Minute::new(minute),
            )
            .expect("surface");
    }
    store
        .put(NETHER, &record(at(0), 1, 200), Minute::new(40))
        .expect("put");
    assert!(store.flush());
    let region_bytes = |dimension: Dimension, id: u32, region_x: i32| {
        let dir = epoch(dimension, id).dir(dir.path());
        let region = RegionId::of_key(at(region_x)).0;
        fs::metadata(dir.join(region.file_name())).map_or(0, |metadata| metadata.len())
    };
    let one_region = region_bytes(OVERWORLD, 0, 0);
    assert!(one_region > 0);
    let focus = Focus {
        dimension: OVERWORLD,
        block_x: 0,
        block_z: 0,
    };
    let used = store.disk_bytes().expect("usage");
    assert_eq!(
        store.evict_regions(used, Some(focus)).expect("evict"),
        0,
        "under the cap"
    );
    let mut freed = store.evict_regions(used - 1, Some(focus)).expect("evict");
    assert_eq!(
        region_bytes(NETHER, 1, 0),
        0,
        "another dimension goes first, however recent"
    );
    assert!(region_bytes(OVERWORLD, 0, 9) > 0);
    freed += store
        .evict_regions(used - freed - 1, Some(focus))
        .expect("evict");
    assert_eq!(
        region_bytes(OVERWORLD, 0, 9),
        0,
        "then the farthest region, however recent"
    );
    let signature =
        epoch(OVERWORLD, 0)
            .dir(dir.path())
            .join(crate::signatures::signature_file_name(
                RegionId::of_key(at(9)).0,
            ));
    assert!(!signature.exists(), "a level-0 region takes its signatures");
    store
        .evict_regions(used - freed - 1, Some(focus))
        .expect("evict");
    assert_eq!(region_bytes(OVERWORLD, 0, 5), 0);
    assert!(
        region_bytes(OVERWORLD, 0, 0) > 0,
        "the nearest region stays, though it was read longest ago"
    );
}

#[test]
fn flushing_closes_the_files_of_epochs_no_session_uses() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = open(dir.path(), 0);
    store
        .begin_session(OVERWORLD, Minute::new(0))
        .expect("begin");
    store.begin_session(NETHER, Minute::new(0)).expect("begin");
    for dimension in [OVERWORLD, NETHER] {
        store
            .put(dimension, &record(key(0, 0, 0, 0), 1, 2), Minute::new(0))
            .expect("put");
    }
    store.end_session(NETHER);
    assert_eq!(store.epochs.len(), 2);
    assert!(store.flush());
    assert_eq!(
        store.epochs.len(),
        1,
        "only the overworld session's epoch stays open"
    );
    let nether = epoch(NETHER, 1);
    let mut set = RegionSet::open(
        nether.dir(dir.path()),
        Access::ReadOnly,
        DEFAULT_OPEN_REGIONS,
    );
    assert!(
        set.contains(key(0, 0, 0, 0)).expect("contains"),
        "written before closing"
    );
}

#[test]
fn records_committed_by_handle_eviction_never_outrun_the_registry() {
    let dir = tempfile::tempdir().expect("temp dir");
    let options = StoreOptions {
        open_regions: NonZeroUsize::MIN,
        ..StoreOptions::default()
    };
    let mut store = Store::open(dir.path(), Minute::new(1), options).expect("open");
    store
        .begin_session(OVERWORLD, Minute::new(1))
        .expect("begin");
    let material = store
        .registry()
        .write()
        .expect("registry")
        .get_or_insert(&MaterialKey::Network(42))
        .expect("material");
    let (first, second) = (key(0, 0, 0, 0), key(0, 32 * 5, 0, 0));
    store
        .put(OVERWORLD, &record(first, 1, 2), Minute::new(1))
        .expect("put");
    // A second region evicts the first handle, which commits.
    store
        .put(OVERWORLD, &record(second, 2, 2), Minute::new(1))
        .expect("put");
    // The process dies before any flush.
    drop(store);

    let mut store = open(dir.path(), 2);
    let files = store.files(epoch(OVERWORLD, 0)).expect("files");
    assert_eq!(
        files.regions.get(first).expect("get"),
        Some(record(first, 1, 2)),
        "the evicted handle committed"
    );
    assert_eq!(
        store
            .registry()
            .read()
            .expect("registry")
            .get(&MaterialKey::Network(42)),
        Some(material),
        "the registry was saved before that commit"
    );
}

#[test]
fn read_only_session_epochs_never_touch_the_writers_directories() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut writer = open(dir.path(), 2);
    let mut reader = open(dir.path(), 2);
    assert_eq!(reader.access(), Access::ReadOnly);
    let ours = writer
        .begin_session(OVERWORLD, Minute::new(2))
        .expect("begin")
        .epoch();
    let theirs = reader
        .begin_session(OVERWORLD, Minute::new(2))
        .expect("begin")
        .epoch();
    assert_eq!(ours, theirs, "both allocate the next id of one catalog");
    let target = key(0, 3, 0, 3);
    writer
        .put(OVERWORLD, &record(target, 1, 2), Minute::new(2))
        .expect("put");
    writer
        .record_surface(OVERWORLD, 0, 0, surface(1, 0, 0), Minute::new(2))
        .expect("surface");
    writer.close();
    assert!(ours.dir(dir.path()).exists());

    let mut out = Vec::new();
    assert!(
        !reader
            .get_into(OVERWORLD, target, &mut out, Minute::new(2))
            .expect("get"),
        "another session's records are not ours"
    );
    reader
        .record_surface(OVERWORLD, 1, 0, surface(2, 1, 0), Minute::new(2))
        .expect("surface");
    let signature = reader
        .files(theirs)
        .expect("files")
        .signatures
        .get(lod::epoch::RegionPos::of_chunk(1, 0))
        .expect("recorded")
        .clone();
    let chunks: Vec<_> = signature
        .recorded_chunks()
        .map(|(x, z, _)| (x, z))
        .collect();
    assert_eq!(chunks, [(1, 0)], "only the reader's own chunk");
}

#[test]
fn lookup_misses_leave_nothing_on_disk() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = open(dir.path(), 1);
    let epoch = store
        .begin_session(OVERWORLD, Minute::new(1))
        .expect("begin")
        .epoch();
    let mut out = Vec::new();
    for x in 0..20 {
        assert!(
            !store
                .get_into(OVERWORLD, key(0, x * 32, 0, 0), &mut out, Minute::new(1))
                .expect("get")
        );
        assert_eq!(store.chunk_hash(OVERWORLD, x * 64, 0).expect("hash"), None);
    }
    assert!(store.flush());
    assert!(!epoch.dir(dir.path()).exists());
    assert_eq!(
        store.access_table(),
        &crate::access::AccessTable::new(),
        "misses are not uses"
    );
}

#[test]
fn epochs_a_candidate_votes_against_are_never_evicted() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = open(dir.path(), 0);
    for dimension in 0..MAX_EPOCHS_PER_STORE as i32 {
        let dimension = Dimension(dimension);
        store
            .begin_session(dimension, Minute::new(dimension.0 as u32))
            .expect("begin");
        store.end_session(dimension);
    }
    let voted = epoch(OVERWORLD, 0);
    let candidate = store
        .begin_session(OVERWORLD, Minute::new(10))
        .expect("begin");
    assert!(candidate.is_candidate());
    // One more dimension's first visit pushes the store over its bound.
    store
        .begin_session(Dimension(MAX_EPOCHS_PER_STORE as i32), Minute::new(10))
        .expect("begin");
    assert!(
        store.catalog().get(voted).is_some(),
        "the voted epoch stays"
    );
    assert!(
        store.catalog().get(epoch(Dimension(1), 1)).is_none(),
        "the least recently used other epoch goes"
    );
}

#[test]
fn signatures_in_use_stay_in_memory_until_their_epoch_goes_idle() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = open(dir.path(), 0);
    let epoch = store
        .begin_session(OVERWORLD, Minute::new(0))
        .expect("begin")
        .epoch();
    store
        .record_surface(OVERWORLD, 0, 0, surface(1, 0, 0), Minute::new(0))
        .expect("surface");
    let file = epoch
        .dir(dir.path())
        .join(crate::signatures::signature_file_name(
            RegionId::of_chunk(0, 0).0,
        ));
    assert!(store.flush());
    assert!(!file.exists(), "a flush does not rewrite signatures in use");
    store.close();
    assert!(file.exists(), "closing writes them");
}

#[test]
fn open_files_stay_within_one_bound_across_every_epoch() {
    let dir = tempfile::tempdir().expect("temp dir");
    let options = StoreOptions {
        open_regions: NonZeroUsize::new(3).expect("non-zero"),
        ..StoreOptions::default()
    };
    let mut store = Store::open(dir.path(), Minute::new(1), options).expect("open");
    let open_files = |store: &Store| {
        store
            .epochs
            .values()
            .map(|files| files.regions.open_files())
            .sum::<usize>()
    };
    for dimension in [OVERWORLD, NETHER, Dimension(2)] {
        store
            .begin_session(dimension, Minute::new(1))
            .expect("begin");
    }
    for region_x in 0..4 {
        for dimension in [OVERWORLD, NETHER, Dimension(2)] {
            let key = key(0, region_x * 32, 0, 0);
            store
                .put(dimension, &record(key, 1, 2), Minute::new(1))
                .expect("put");
            assert!(open_files(&store) <= 3, "{} files open", open_files(&store));
        }
    }
    assert_eq!(store.epochs.len(), 3);
    assert!(store.flush());
    let mut out = Vec::new();
    for dimension in [OVERWORLD, NETHER, Dimension(2)] {
        for region_x in 0..4 {
            assert!(
                store
                    .get_into(
                        dimension,
                        key(0, region_x * 32, 0, 0),
                        &mut out,
                        Minute::new(1)
                    )
                    .expect("get"),
                "closed files were committed"
            );
        }
    }
}

mod read_through;

//! Which stored records an undecided session reads through to.

use super::*;

/// Records `seed`'s surface over every chunk of signature region `(region, 0)`, returning
/// the session's status.
fn record_region(store: &mut Store, seed: u32, region: i32) -> EpochStatus {
    let mut status = store.epoch_status(OVERWORLD).expect("session");
    for chunk in 0..16 {
        let (x, z) = (region * 4 + chunk % 4, chunk / 4);
        status = store
            .record_surface(OVERWORLD, x, z, surface(seed, x, z), Minute::new(10))
            .expect("record surface");
    }
    status
}

/// Records of world 1 over regions `(0..3, 0)`, keyed by level, section x.
fn stored_keys() -> [lod::geometry::SectionKey; 5] {
    [
        key(0, 0, 2, 0), // region 0
        key(0, 3, 2, 1), // region 1
        key(0, 4, 2, 0), // region 2
        key(1, 1, 1, 0), // exactly region 1
        key(2, 0, 0, 0), // regions 0 and 1
    ]
}

fn reads_through(store: &mut Store, key: lod::geometry::SectionKey) -> bool {
    store
        .get_into(OVERWORLD, key, &mut Vec::new(), Minute::new(10))
        .expect("get")
}

#[test]
fn a_region_that_voted_against_the_stored_epoch_stops_reading_through_at_once() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut store = open(dir.path(), 1);
    store
        .begin_session(OVERWORLD, Minute::new(1))
        .expect("begin");
    record_world(&mut store, OVERWORLD, 1, 3);
    for (seed, key) in stored_keys().into_iter().enumerate() {
        store
            .put(OVERWORLD, &record(key, seed as u32, 4), Minute::new(1))
            .expect("put");
    }
    assert!(store.flush());
    store.close();

    let mut store = open(dir.path(), 10);
    assert!(
        store
            .begin_session(OVERWORLD, Minute::new(10))
            .expect("begin")
            .is_candidate()
    );
    let stored = epoch(OVERWORLD, 0);
    assert!(
        stored_keys()
            .into_iter()
            .all(|key| reads_through(&mut store, key))
    );

    // Region 0 matches, region 1 shows another world and region 2 is flat, so abstains;
    // one match against one mismatch leaves the session undecided.
    assert!(record_region(&mut store, 1, 0).is_candidate());
    assert!(record_region(&mut store, 2, 1).is_candidate());
    for chunk in 0..16 {
        let (x, z) = (8 + chunk % 4, chunk / 4);
        let status = store
            .record_surface(OVERWORLD, x, z, flat_surface(5), Minute::new(10))
            .expect("record flat surface");
        assert!(status.is_candidate());
    }
    assert_eq!(store.leading_epoch(OVERWORLD), Some(stored));
    assert_eq!(store.hidden_regions(), 1);

    let [matching, mismatching, abstaining, region, straddling] = stored_keys();
    assert!(reads_through(&mut store, matching), "a matching region");
    assert!(
        reads_through(&mut store, abstaining),
        "an abstaining region"
    );
    assert!(
        !reads_through(&mut store, mismatching),
        "the mismatching region"
    );
    assert!(
        !reads_through(&mut store, region),
        "a coarser section of it"
    );
    assert!(
        !reads_through(&mut store, straddling),
        "a coarser section overlapping it"
    );
    assert_eq!(store.read_through_epoch(OVERWORLD, matching), Some(stored));
    assert_eq!(store.read_through_epoch(OVERWORLD, mismatching), None);
    assert!(store.drain_events().iter().any(|event| matches!(
        event,
        StoreEvent::ReadThroughHidden { region, epoch, .. }
            if *region == lod::epoch::RegionPos { x: 1, z: 0 } && *epoch == stored
    )));

    // The session's own record still shows where read-through is hidden.
    let own = record(mismatching, 9, 4);
    store.put(OVERWORLD, &own, Minute::new(10)).expect("put");
    let mut out = Vec::new();
    assert!(
        store
            .get_into(OVERWORLD, mismatching, &mut out, Minute::new(10))
            .expect("get")
    );
    assert_eq!(out, own);
}

#[test]
fn a_split_leading_epoch_moves_read_through_to_the_next_stored_epoch() {
    let dir = tempfile::tempdir().expect("temp dir");
    // Epoch 0: world 1 over regions 0..110, holding `older`.
    seeded_store(dir.path(), 110);
    let older = key(0, 1, 2, 3);
    let newer = key(0, 2, 2, 1);
    // Epoch 1: world 2 over regions 0..220, holding `newer`; it splits from epoch 0.
    let mut store = open(dir.path(), 10);
    store
        .begin_session(OVERWORLD, Minute::new(10))
        .expect("begin");
    for region in 0..220 {
        record_region(&mut store, 2, region);
    }
    assert_eq!(
        store.epoch_status(OVERWORLD),
        Some(EpochStatus::Established(epoch(OVERWORLD, 1)))
    );
    store
        .put(OVERWORLD, &record(newer, 2, 4), Minute::new(10))
        .expect("put");
    assert!(store.flush());
    store.close();

    let mut store = open(dir.path(), 20);
    assert!(
        store
            .begin_session(OVERWORLD, Minute::new(20))
            .expect("begin")
            .is_candidate()
    );
    assert_eq!(
        store.leading_epoch(OVERWORLD),
        Some(epoch(OVERWORLD, 1)),
        "the most recently used"
    );
    assert!(reads_through(&mut store, newer));
    assert!(!reads_through(&mut store, older));
    let changes = store.leading_epoch_changes();

    // A third world where only epoch 1 has signatures: it splits from epoch 1 and leaves
    // epoch 0 undecided.
    for region in 110..220 {
        assert!(record_region(&mut store, 3, region).is_candidate());
    }
    assert_eq!(store.leading_epoch(OVERWORLD), Some(epoch(OVERWORLD, 0)));
    assert_eq!(store.leading_epoch_changes(), changes + 1);
    assert!(
        reads_through(&mut store, older),
        "the next epoch reads through"
    );
    assert!(
        !reads_through(&mut store, newer),
        "the split epoch no longer does"
    );
}

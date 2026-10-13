//! Crashes at every write of a sequence of commits: each replayed prefix, whole or torn,
//! must open as the last committed state, and only a file with no valid commit yet may
//! need a scan, which must keep that state.

use std::collections::BTreeMap;

use super::*;
use crate::positional::recorder::{self, Write};

/// What a file holds: every record by key bits and every chunk hash by slot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct State {
    records: BTreeMap<u64, Vec<u8>>,
    hashes: BTreeMap<u16, u64>,
}

fn state(region: &RegionFile) -> State {
    let mut records = BTreeMap::new();
    for key in region.keys() {
        let record = region.get(key).expect("readable").expect("listed");
        records.insert(key.bits(), record);
    }
    let hashes = region
        .index
        .hashes()
        .map(|(slot, hash)| (slot.raw(), hash))
        .collect();
    State { records, hashes }
}

/// One step: changes to a file, then a commit.
type Step = fn(&mut RegionFile, u32);

fn put_three(region: &mut RegionFile, seed: u32) {
    for x in 0..3 {
        region
            .put(&record(key(0, x, 0, 0), seed + x as u32, 2))
            .expect("put");
    }
    region
        .set_chunk_hash(0, 0, Some(seed.into()))
        .expect("hash");
}

fn churn(region: &mut RegionFile, seed: u32) {
    region
        .put(&record(key(0, 0, 0, 0), seed, 2))
        .expect("overwrite");
    region.remove(key(0, 1, 0, 0)).expect("remove");
    region.set_chunk_hash(0, 0, None).expect("clear hash");
    region
        .set_chunk_hash(5, 7, Some(seed.into()))
        .expect("hash");
}

fn grow(region: &mut RegionFile, seed: u32) {
    let x = 4 + (seed % 20) as i32;
    region.put(&record(key(0, x, 1, 2), seed, 2)).expect("put");
}

const STEPS: [Step; 8] = [put_three, churn, grow, grow, put_three, churn, grow, grow];

/// Runs [`STEPS`] on `region`, recording every write and the state after each commit;
/// the first state is the one before any step.
fn run(mut region: RegionFile, seed: u32) -> (Vec<Write>, Vec<State>) {
    let mut states = vec![state(&region)];
    recorder::start();
    for (index, step) in STEPS.iter().enumerate() {
        step(&mut region, seed + index as u32 * 10);
        assert!(region.commit().expect("commit"));
        states.push(state(&region));
    }
    (recorder::finish(), states)
}

/// Opens every crash image of `writes` over `base` and checks it against `states`.
/// `base_committed` tells whether `base` already holds a valid commit.
fn replay(
    dir: &Path,
    base: &[u8],
    base_committed: bool,
    writes: &[Write],
    states: &[State],
    phase: &str,
) {
    let path = dir.join(format!("replay-{phase}"));
    let mut image = base.to_vec();
    let mut commits = 0;
    for (index, (offset, bytes)) in writes.iter().enumerate() {
        for torn in [0, 1, bytes.len() / 2, bytes.len() - 1] {
            let mut crashed = image.clone();
            apply(&mut crashed, *offset, &bytes[..torn]);
            let scan_allowed = !base_committed && commits == 0;
            let (old, new) = (&states[commits], &states[commits + 1]);
            check(&path, &crashed, old, new, scan_allowed, || {
                format!("{phase}: write {index} torn after {torn} bytes")
            });
        }
        apply(&mut image, *offset, bytes);
        let slot_write =
            bytes.len() == COMMIT_SLOT_LEN && COMMIT_SLOT_OFFSETS.contains(&(*offset as usize));
        commits += usize::from(slot_write);
    }
    assert_eq!(
        commits,
        states.len() - 1,
        "{phase}: one slot write per commit"
    );
    let last = &states[commits];
    check(&path, &image, last, last, false, || {
        format!("{phase}: every write")
    });
}

fn apply(image: &mut Vec<u8>, offset: u64, bytes: &[u8]) {
    let end = offset as usize + bytes.len();
    if image.len() < end {
        image.resize(end, 0);
    }
    image[offset as usize..end].copy_from_slice(bytes);
}

/// Opens `image` read-only: a committed index must give exactly `old`. A scan, when
/// allowed, must keep every record of `old` and hold only records and hashes of `old` or
/// `new`.
fn check(
    path: &Path,
    image: &[u8],
    old: &State,
    new: &State,
    scan_allowed: bool,
    what: impl Fn() -> String,
) {
    fs::write(path, image).expect("write image");
    let region = RegionFile::open_read_only(path, region0())
        .unwrap_or_else(|error| panic!("{}: {error}", what()))
        .expect("exists");
    let found = state(&region);
    if let Some(recovery) = region.recovery() {
        assert!(scan_allowed, "{}: needed a scan: {recovery:?}", what());
    } else {
        assert_eq!(&found, old, "{}", what());
        return;
    }
    for (key, record) in &found.records {
        assert!(
            old.records.get(key) == Some(record) || new.records.get(key) == Some(record),
            "{}: scan found a record of neither state",
            what()
        );
    }
    for key in old.records.keys() {
        assert!(
            found.records.contains_key(key),
            "{}: scan lost a record",
            what()
        );
    }
    for (slot, hash) in &found.hashes {
        assert!(
            old.hashes.get(slot) == Some(hash) || new.hashes.get(slot) == Some(hash),
            "{}: scan found a hash of neither state",
            what()
        );
    }
}

#[test]
fn every_crash_point_opens_as_a_committed_state() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);

    // A new file: its first commit has no older slot to fall back to.
    let region = open(&path);
    let base = fs::read(&path).expect("base");
    let (writes, states) = run(region, 1);
    replay(dir.path(), &base, false, &writes, &states, "new");

    // After a scan rebuilt the index, whose sequence comes from the frames.
    wipe_commit_slots(&path);
    let base = fs::read(&path).expect("base");
    let region = open(&path);
    assert!(region.recovery().is_some());
    let (writes, states) = run(region, 100);
    replay(dir.path(), &base, false, &writes, &states, "rebuilt");

    // The same handle after compaction restarted its sequence.
    let mut region = open(&path);
    for x in 0..40 {
        for round in 0..3 {
            region
                .put(&record(key(0, x % 32, 3, x / 32), round, 30))
                .expect("put");
        }
    }
    region.commit().expect("commit");
    region.compact().expect("compact");
    let base = fs::read(&path).expect("base");
    let (writes, states) = run(region, 200);
    replay(dir.path(), &base, true, &writes, &states, "compacted");
}

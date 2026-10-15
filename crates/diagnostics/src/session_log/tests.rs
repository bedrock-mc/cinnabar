use super::*;
use std::time::Duration;

/// A clock that advances one second per reading from a fixed start.
fn ticking(start_secs: u64) -> Clock {
    let mut next = start_secs;
    Box::new(move || {
        let at = UNIX_EPOCH + Duration::from_secs(next);
        next += 1;
        at
    })
}

fn names(family: &Family) -> Vec<String> {
    family
        .archives()
        .unwrap()
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn stamps_are_utc_calendar_times() {
    assert_eq!(utc_stamp(UNIX_EPOCH), "19700101-000000");
    assert_eq!(
        utc_stamp(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
        "20231114-221320"
    );
    assert_eq!(
        utc_stamp(UNIX_EPOCH + Duration::from_secs(951_782_400)),
        "20000229-000000"
    );
}

#[test]
fn opening_a_session_archives_the_previous_one_under_its_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let family = Family::new(dir.path(), "client", "log");
    fs::write(family.current(), b"previous session\n").unwrap();
    let mut log = SessionLog::open_with_clock(
        family.clone(),
        Retention::DEFAULT,
        b"HEADER\n".to_vec(),
        ticking(1_700_000_000),
    )
    .unwrap();
    log.write_all(b"current\n").unwrap();
    log.flush().unwrap();
    assert_eq!(names(&family), ["client-20231114-221320.log"]);
    assert_eq!(
        fs::read_to_string(dir.path().join("client-20231114-221320.log")).unwrap(),
        "previous session\n"
    );
    assert_eq!(
        fs::read_to_string(family.current()).unwrap(),
        "HEADER\ncurrent\n"
    );
}

#[test]
fn the_legacy_backup_is_archived_as_the_older_session() {
    let dir = tempfile::tempdir().unwrap();
    let family = Family::new(dir.path(), "client", "log");
    fs::write(dir.path().join("client.log.1"), b"oldest").unwrap();
    fs::write(family.current(), b"newer").unwrap();
    let _log =
        SessionLog::open_with_clock(family.clone(), Retention::DEFAULT, Vec::new(), ticking(0))
            .unwrap();
    let archives = family.archives().unwrap();
    let texts: Vec<String> = archives
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect();
    assert_eq!(texts, ["oldest", "newer"]);
    assert!(!dir.path().join("client.log.1").exists());
}

#[test]
fn retention_keeps_at_most_the_newest_count_of_archives() {
    let dir = tempfile::tempdir().unwrap();
    let family = Family::new(dir.path(), "client", "log");
    let retention = Retention {
        session_bytes: 1024,
        archives: 3,
        archive_bytes: 1 << 20,
    };
    for session in 0..6u64 {
        let mut log = SessionLog::open_with_clock(
            family.clone(),
            retention,
            Vec::new(),
            ticking(session * 60),
        )
        .unwrap();
        write!(log, "session {session}").unwrap();
    }
    let kept: Vec<String> = family
        .archives()
        .unwrap()
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect();
    assert_eq!(kept, ["session 2", "session 3", "session 4"]);
    assert_eq!(fs::read_to_string(family.current()).unwrap(), "session 5");
}

#[test]
fn retention_deletes_the_oldest_archives_past_the_byte_budget() {
    let dir = tempfile::tempdir().unwrap();
    let family = Family::new(dir.path(), "client", "log");
    for (index, size) in [40usize, 30, 20, 10].into_iter().enumerate() {
        let at = UNIX_EPOCH + Duration::from_secs(index as u64 * 3600);
        fs::write(family.archive_path(at).unwrap(), vec![b'x'; size]).unwrap();
    }
    family
        .prune(Retention {
            session_bytes: 1024,
            archives: 10,
            archive_bytes: 60,
        })
        .unwrap();
    let sizes: Vec<u64> = family
        .archives()
        .unwrap()
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .collect();
    assert_eq!(sizes, [30, 20, 10]);
}

/// Once an archive breaks the byte budget, every older archive goes too, even one that would
/// still fit, so retention never keeps an older session over a newer one.
#[test]
fn retention_never_keeps_an_archive_older_than_one_it_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let family = Family::new(dir.path(), "client", "log");
    for (index, size) in [1usize, 4, 7, 8].into_iter().enumerate() {
        let at = UNIX_EPOCH + Duration::from_secs(index as u64 * 3600);
        fs::write(family.archive_path(at).unwrap(), vec![b'x'; size]).unwrap();
    }
    family
        .prune(Retention {
            session_bytes: 1024,
            archives: 10,
            archive_bytes: 16,
        })
        .unwrap();
    let sizes: Vec<u64> = family
        .archives()
        .unwrap()
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .collect();
    assert_eq!(sizes, [7, 8]);
}

#[test]
fn archives_ignore_other_families_and_unrelated_names() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "client-20260101-000000.log",
        "client-20260101-000000-2.log",
        "client-20260101-000000.jsonl",
        "core.log",
        "client-notes.log",
        "first-run-20260101-000000.log",
    ] {
        fs::write(dir.path().join(name), b"").unwrap();
    }
    assert_eq!(
        names(&Family::new(dir.path(), "client", "log")),
        ["client-20260101-000000.log", "client-20260101-000000-2.log"]
    );
    assert_eq!(
        names(&Family::new(dir.path(), "first-run", "log")),
        ["first-run-20260101-000000.log"]
    );
}

#[test]
fn an_oversized_session_continues_in_a_fresh_file_with_its_header() {
    let dir = tempfile::tempdir().unwrap();
    let family = Family::new(dir.path(), "client", "log");
    let retention = Retention {
        session_bytes: 16,
        archives: 10,
        archive_bytes: 1 << 20,
    };
    let mut log =
        SessionLog::open_with_clock(family.clone(), retention, b"H\n".to_vec(), ticking(0))
            .unwrap();
    log.write_all(b"first part\n").unwrap();
    log.write_all(b"second part\n").unwrap();
    log.write_all(b"0123456789abcdefghij").unwrap();
    log.flush().unwrap();
    let archives = family.archives().unwrap();
    let texts: Vec<String> = archives
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect();
    assert_eq!(texts, ["H\nfirst part\n", "H\nsecond part\n"]);
    assert_eq!(
        fs::read_to_string(family.current()).unwrap(),
        "H\n6789abcdefghij"
    );
}

#[test]
fn shared_writers_append_whole_writes_to_one_session() {
    let dir = tempfile::tempdir().unwrap();
    let family = Family::new(dir.path(), "first-run", "log");
    let log = SharedLog::new(
        SessionLog::open_with_clock(family.clone(), Retention::DEFAULT, Vec::new(), ticking(0))
            .unwrap(),
    );
    let mut first = log.clone();
    let mut second = log;
    first.write_all(b"stdout line\n").unwrap();
    second.write_all(b"stderr line\n").unwrap();
    assert_eq!(
        fs::read_to_string(family.current()).unwrap(),
        "stdout line\nstderr line\n"
    );
}

/// Pruning a timestamp's lowest suffix must not make a later archive look older.
#[test]
fn same_second_rotation_keeps_the_newest_archives_after_pruning() {
    let dir = tempfile::tempdir().unwrap();
    let family = Family::new(dir.path(), "client", "log");
    let retention = Retention {
        session_bytes: 16,
        archives: 3,
        archive_bytes: 1024,
    };
    let mut log = SessionLog::open_with_clock(
        family.clone(),
        retention,
        b"H\n".to_vec(),
        Box::new(|| UNIX_EPOCH),
    )
    .unwrap();
    for index in 0..24 {
        writeln!(log, "part {index:02} abc").unwrap();
    }
    log.flush().unwrap();
    let kept: Vec<_> = family
        .archives()
        .unwrap()
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect();
    assert_eq!(
        kept,
        ["H\npart 20 abc\n", "H\npart 21 abc\n", "H\npart 22 abc\n"]
    );
    assert_eq!(
        fs::read_to_string(family.current()).unwrap(),
        "H\npart 23 abc\n"
    );
}

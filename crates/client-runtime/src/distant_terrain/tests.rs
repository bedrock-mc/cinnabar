use std::time::Duration;

use lod_store::store::{Store, StoreOptions};
use protocol::{ChangeDimensionEvent, WorldBootstrap, WorldEvent};
use test_time::eventually;

use super::*;

const NOW: Minute = Minute::new(1);
const DESTINATION: &str = "remote:distant-terrain.example:19132";

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    })
}

fn target(session_generation: u64) -> DistantTerrainTarget {
    DistantTerrainTarget::new(DESTINATION, "level", 7, session_generation)
}

/// Runs one frame for `stream` wanting `target`.
fn update(
    sessions: &mut DistantTerrainSessions,
    stream: &mut WorldStream,
    target: Option<DistantTerrainTarget>,
) {
    sessions.update(DistantTerrainFrame {
        target,
        stream: Some(stream),
        now: NOW,
        uptime: Duration::ZERO,
        player_block: Some([0, 0]),
    });
}

/// Runs frames until `done` holds.
fn update_until(
    sessions: &mut DistantTerrainSessions,
    stream: &mut WorldStream,
    target: Option<DistantTerrainTarget>,
    what: &str,
    done: impl Fn(&DistantTerrainSessions, &WorldStream) -> bool,
) {
    eventually(what, || {
        update(sessions, stream, target.clone());
        done(sessions, stream)
    });
}

fn open_writer(sessions: &DistantTerrainSessions, _: &WorldStream) -> bool {
    sessions.report().state == StoreState::Open { read_only: false }
}

#[test]
fn a_session_opens_its_store_off_the_frame_and_attaches_the_stream() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sessions = DistantTerrainSessions::new(dir.path());
    let mut stream = stream();
    update(&mut sessions, &mut stream, Some(target(1)));
    assert_eq!(sessions.report().state, StoreState::Opening);
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the store to open",
        open_writer,
    );
    assert!(stream.distant_terrain_enabled());
    assert_eq!(sessions.session_dimensions(), [0]);
    assert!(
        StoreRoot::new(dir.path())
            .store_dir(&target(1).key)
            .is_dir()
    );
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the session's epoch",
        |sessions, _| sessions.report().epoch_changes == 1,
    );
    assert_eq!(sessions.report().store_errors, 0);
    assert!(sessions.report().ingest.is_some());
}

#[test]
fn a_dimension_change_begins_its_session_and_ends_the_settled_one() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sessions = DistantTerrainSessions::new(dir.path());
    let mut stream = stream();
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the store to open",
        open_writer,
    );
    stream
        .submit(
            1,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0; 3],
                ..Default::default()
            }),
        )
        .expect("dimension change");
    assert_eq!(stream.current_dimension(), 1);
    update(&mut sessions, &mut stream, Some(target(1)));
    // Nothing waits to be written, so the overworld session ends at once.
    assert_eq!(sessions.session_dimensions(), [1]);
    assert!(stream.distant_terrain_enabled());
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "both sessions' epochs",
        |sessions, _| sessions.report().epoch_changes == 2,
    );
}

#[test]
fn a_change_into_the_same_dimension_begins_its_session_again() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sessions = DistantTerrainSessions::new(dir.path());
    let mut stream = stream();
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the first session's epoch",
        |sessions, _| sessions.report().epoch_changes == 1,
    );
    stream
        .submit(
            1,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 0,
                position: [0.0; 3],
                ..Default::default()
            }),
        )
        .expect("dimension change");
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the session to begin again",
        |sessions, _| sessions.report().epoch_changes == 2,
    );
    assert_eq!(sessions.session_dimensions(), [0]);
}

#[test]
fn turning_off_closes_without_blocking_and_turning_on_opens_as_the_writer_again() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sessions = DistantTerrainSessions::new(dir.path());
    let mut stream = stream();
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the store to open",
        open_writer,
    );
    update(&mut sessions, &mut stream, None);
    assert_eq!(sessions.report().state, StoreState::Off);
    assert!(!stream.distant_terrain_enabled());
    assert!(sessions.session_dimensions().is_empty());
    assert!(!sessions.is_idle(), "closing");

    // The store reopens only once the closed one released its lock, so it writes.
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the store to reopen",
        open_writer,
    );
    assert!(!sessions.is_closing());
    assert!(stream.distant_terrain_enabled());
}

#[test]
fn a_closed_store_waits_for_its_released_stream_before_closing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sessions = DistantTerrainSessions::new(dir.path());
    let mut stream = stream();
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the store to open",
        open_writer,
    );
    // Leaving takes the stream off the frame before the adapter closes the store.
    let frame = |sessions: &mut DistantTerrainSessions| {
        sessions.update(DistantTerrainFrame {
            target: None,
            stream: None,
            now: NOW,
            uptime: Duration::ZERO,
            player_block: None,
        });
    };
    frame(&mut sessions);
    frame(&mut sessions);
    assert_eq!(sessions.report().state, StoreState::Off);
    assert!(sessions.is_closing(), "the stream may still capture");
    assert!(!sessions.is_idle());
    drop(stream);
    eventually("the store to close", || {
        frame(&mut sessions);
        sessions.is_idle()
    });
}

#[test]
fn a_closed_store_stops_waiting_for_final_captures_after_the_limit() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sessions = DistantTerrainSessions::new(dir.path());
    let mut stream = stream();
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the store to open",
        open_writer,
    );
    let at = |sessions: &mut DistantTerrainSessions, now: Minute, uptime| {
        sessions.update(DistantTerrainFrame {
            target: None,
            stream: None,
            now,
            uptime,
            player_block: None,
        });
    };
    let start = Duration::from_secs(100);
    at(&mut sessions, NOW, start);
    // The wall clock turning a minute a second after closing starts is no reason to stop.
    let next_minute = Minute::new(NOW.get() + 1);
    at(&mut sessions, next_minute, start + Duration::from_secs(1));
    at(
        &mut sessions,
        next_minute,
        start + FINISH_LIMIT - Duration::from_millis(1),
    );
    assert!(sessions.is_closing());
    eventually("the store to close", || {
        at(&mut sessions, next_minute, start + FINISH_LIMIT);
        sessions.is_idle()
    });
    assert!(
        !stream.distant_terrain_enabled(),
        "closing early stopped the stream's ingestion"
    );
    // Turning it on again attaches the stream to the reopened store.
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the store to reopen",
        |sessions, stream| open_writer(sessions, stream) && stream.distant_terrain_enabled(),
    );
}

#[test]
fn a_new_session_on_the_same_world_reopens_its_store() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sessions = DistantTerrainSessions::new(dir.path());
    let mut first = stream();
    update_until(
        &mut sessions,
        &mut first,
        Some(target(1)),
        "the store to open",
        open_writer,
    );
    // A new session retires the old one's stream.
    drop(first);
    let mut second = stream();
    update(&mut sessions, &mut second, Some(target(2)));
    assert_ne!(
        sessions.report().state,
        StoreState::Open { read_only: false }
    );
    assert!(!second.distant_terrain_enabled());
    update_until(
        &mut sessions,
        &mut second,
        Some(target(2)),
        "the new session's store",
        |sessions, stream| open_writer(sessions, stream) && stream.distant_terrain_enabled(),
    );
    assert_eq!(sessions.session_dimensions(), [0]);
}

#[test]
fn a_store_another_client_writes_is_opened_read_only_and_ingests_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let writer = Store::open(
        &StoreRoot::new(dir.path()).store_dir(&target(1).key),
        NOW,
        StoreOptions::default(),
    )
    .expect("open the writer");
    let mut sessions = DistantTerrainSessions::new(dir.path());
    let mut stream = stream();
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the store to open",
        |sessions, _| sessions.report().state == StoreState::Open { read_only: true },
    );
    update(&mut sessions, &mut stream, Some(target(1)));
    assert!(!stream.distant_terrain_enabled());
    drop(writer);
}

#[test]
fn a_store_that_cannot_open_fails_once_for_its_target() {
    let dir = tempfile::tempdir().expect("temp dir");
    // A file where the root directory belongs.
    let blocked = dir.path().join("lod");
    std::fs::write(&blocked, b"not a directory").expect("write file");
    let mut sessions = DistantTerrainSessions::new(&blocked);
    let mut stream = stream();
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the open to fail",
        |sessions, _| sessions.report().state == StoreState::Failed,
    );
    update(&mut sessions, &mut stream, Some(target(1)));
    assert_eq!(sessions.report().state, StoreState::Failed);
    assert_eq!(sessions.report().store_errors, 1);
    assert!(!stream.distant_terrain_enabled());
}

#[test]
fn quotas_run_on_their_minute_cadence() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = StoreRoot::new(dir.path());
    let other = StoreKey::new("remote:other.example:19132", "", 0);
    Store::open(&root.store_dir(&other), NOW, StoreOptions::default())
        .expect("open another store")
        .close();
    // The other store expires a minute before the cadence comes round.
    let quotas = Quotas {
        expiry_minutes: QUOTA_INTERVAL_MINUTES - 2,
        ..Quotas::default()
    };
    let mut sessions = DistantTerrainSessions::with_quotas(dir.path(), quotas);
    let mut stream = stream();
    update_until(
        &mut sessions,
        &mut stream,
        Some(target(1)),
        "the store to open",
        open_writer,
    );
    let mut at = |now| {
        sessions.update(DistantTerrainFrame {
            target: Some(target(1)),
            stream: Some(&mut stream),
            now,
            uptime: Duration::ZERO,
            player_block: Some([0, 0]),
        });
    };
    at(Minute::new(NOW.get() + QUOTA_INTERVAL_MINUTES - 1));
    assert!(root.store_dir(&other).exists(), "not yet the quota cadence");
    at(Minute::new(NOW.get() + QUOTA_INTERVAL_MINUTES));
    eventually("the expired store to be removed", || {
        !root.store_dir(&other).exists()
    });
    assert!(root.store_dir(&target(1).key).is_dir());
}

#[test]
fn a_writer_thread_that_cannot_start_is_its_own_state_not_read_only() {
    let mut report = DistantTerrainReport::default();
    let error = chunk_pipeline::DistantTerrainUnavailable::WriterThread(std::io::Error::other(
        "no threads",
    ));
    assert_eq!(
        lifecycle::unavailable(&error, &mut report),
        Ingest::WriterUnavailable
    );
    assert_eq!(report.store_errors, 1);
    let read_only = chunk_pipeline::DistantTerrainUnavailable::ReadOnly;
    assert_eq!(
        lifecycle::unavailable(&read_only, &mut report),
        Ingest::ReadOnly
    );
    assert_eq!(report.store_errors, 1, "another client's lock is no error");
    report.state = StoreState::WriterUnavailable;
    assert!(
        report
            .marker_line()
            .starts_with("CINNABAR_LOD_INGEST state=writer_unavailable ")
    );
}

#[test]
fn the_marker_line_names_the_state_and_every_counter() {
    let report = DistantTerrainReport {
        state: StoreState::Open { read_only: true },
        ingest: Some(DistantTerrainStats {
            sampled: 3,
            skipped_unchanged: 2,
            ..DistantTerrainStats::default()
        }),
        store_errors: 1,
        ..DistantTerrainReport::default()
    };
    let line = report.marker_line();
    assert!(line.starts_with("CINNABAR_LOD_INGEST state=read_only "));
    for field in [
        "sampled=3",
        "skipped_unchanged=2",
        " store_errors=1",
        "dropped=0",
        "dropped_final_captures=0",
        "jobs_queued=0",
        "superseded=0",
        "stale_writes=0",
        "unknown_ids=0",
        "unkeyed_ids=0",
        "registry_full=0",
        "worker_ms=0",
    ] {
        assert!(line.contains(field), "{field} in {line}");
    }
}

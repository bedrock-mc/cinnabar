use std::path::Path;

use test_time::eventually;

use super::*;
use crate::{
    epochs::{EpochId, EpochRef},
    region_id::RegionId,
    store::{EpochStatus, StoreOptions},
    test_support::{flip_byte, key, record},
};

const OVERWORLD: Dimension = Dimension(0);

fn spawn(dir: &Path, config: ActorConfig) -> StoreActor {
    let root = StoreRoot::new(dir);
    let store = root
        .open(&store_key(), Minute::new(1), StoreOptions::default())
        .expect("open");
    StoreActor::spawn(store, root, Quotas::default(), config).expect("spawn")
}

fn store_key() -> crate::identity::StoreKey {
    crate::identity::StoreKey::new("remote:actor", "", 0)
}

fn submit(actor: &mut StoreActor, request: Request) {
    actor.try_submit(request).expect("queued");
}

fn put(dimension: Dimension, record: Vec<u8>) -> Request {
    Request::Put {
        dimension,
        record: record.into(),
        now: Minute::new(1),
    }
}

/// Polls until `ticket` is answered.
fn result_of(actor: &mut StoreActor, ticket: ReadTicket) -> ReadOutcome {
    let mut found = None;
    eventually("the read result", || {
        let mut results = Vec::new();
        actor.poll_results(&mut results);
        if let Some(result) = results.into_iter().find(|result| result.ticket == ticket) {
            found = Some(result.outcome);
        }
        found.is_some()
    });
    found.expect("found above")
}

/// Polls events until one matches.
fn wait_for_event(actor: &mut StoreActor, what: &str, matches: impl Fn(&ActorEvent) -> bool) {
    eventually(what, || {
        let mut events = Vec::new();
        actor.poll_events(&mut events);
        events.iter().any(&matches)
    });
}

#[test]
fn reads_see_earlier_writes_and_the_last_write_per_key_wins() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut actor = spawn(dir.path(), ActorConfig::default());
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(1),
        },
    );
    let target = key(0, 3, 1, -4);
    submit(&mut actor, put(OVERWORLD, record(target, 1, 3)));
    submit(&mut actor, put(OVERWORLD, record(target, 2, 3)));
    let ticket = actor
        .try_read(OVERWORLD, target, Minute::new(1))
        .expect("read");
    assert_eq!(
        result_of(&mut actor, ticket),
        ReadOutcome::Found(record(target, 2, 3))
    );

    submit(
        &mut actor,
        Request::Remove {
            dimension: OVERWORLD,
            key: target,
        },
    );
    let ticket = actor
        .try_read(OVERWORLD, target, Minute::new(1))
        .expect("read");
    assert_eq!(result_of(&mut actor, ticket), ReadOutcome::Missing);
    let other = actor
        .try_read(Dimension(7), target, Minute::new(1))
        .expect("read");
    assert!(
        matches!(result_of(&mut actor, other), ReadOutcome::Failed(_)),
        "a read without a session fails instead of waiting"
    );
    assert_eq!(actor.reads_outstanding(), 0);
    actor.shutdown(Minute::new(2));
}

#[test]
fn shutdown_flushes_every_queued_write() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut actor = spawn(dir.path(), ActorConfig::default());
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(1),
        },
    );
    let keys: Vec<_> = (0..40).map(|x| key(0, x, 0, 0)).collect();
    for (seed, &key) in keys.iter().enumerate() {
        submit(&mut actor, put(OVERWORLD, record(key, seed as u32, 2)));
    }
    submit(
        &mut actor,
        Request::ChunkHash {
            dimension: OVERWORLD,
            x: 1,
            z: 1,
            hash: Some(5),
        },
    );
    let events = actor.shutdown(Minute::new(2));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ActorEvent::RequestFailed { .. }))
    );

    let root = StoreRoot::new(dir.path());
    let store = root
        .open(&store_key(), Minute::new(3), StoreOptions::default())
        .expect("reopen");
    let epoch = EpochRef {
        dimension: OVERWORLD,
        id: EpochId(0),
    };
    let mut set = crate::region_set::RegionSet::open(
        epoch.dir(store.dir()),
        Access::ReadOnly,
        crate::region_set::DEFAULT_OPEN_REGIONS,
    );
    for (seed, &key) in keys.iter().enumerate() {
        assert_eq!(
            set.get(key).expect("get"),
            Some(record(key, seed as u32, 2))
        );
    }
    assert_eq!(set.chunk_hash(1, 1).expect("hash"), Some(5));
}

#[test]
fn corrupt_records_read_as_corrupt_and_flushes_report_quotas() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut actor = spawn(dir.path(), ActorConfig::default());
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(1),
        },
    );
    let target = key(0, 0, 0, 0);
    submit(&mut actor, put(OVERWORLD, record(target, 1, 3)));
    submit(
        &mut actor,
        Request::EnforceQuotas {
            now: Minute::new(1),
            focus: None,
        },
    );
    wait_for_event(&mut actor, "the flush's quota report", |event| {
        matches!(event, ActorEvent::Quota(_))
    });
    let path = EpochRef {
        dimension: OVERWORLD,
        id: EpochId(0),
    }
    .dir(&StoreRoot::new(dir.path()).store_dir(&store_key()))
    .join(RegionId::of_key(target).0.file_name());
    // Inside the record's payload, past the frame header after the fixed file header.
    flip_byte(&path, crate::region_file::DATA_START + 40);
    let ticket = actor
        .try_read(OVERWORLD, target, Minute::new(1))
        .expect("read");
    assert_eq!(result_of(&mut actor, ticket), ReadOutcome::Corrupt);
    actor.shutdown(Minute::new(2));
}

#[test]
fn sessions_report_their_epochs_and_read_only_writes_fail() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut writer = spawn(dir.path(), ActorConfig::default());
    submit(
        &mut writer,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(1),
        },
    );
    wait_for_event(&mut writer, "the established epoch", |event| {
        matches!(
            event,
            ActorEvent::Store(StoreEvent::Epoch {
                status: EpochStatus::Established(_),
                ..
            })
        )
    });

    let mut reader = spawn(dir.path(), ActorConfig::default());
    assert_eq!(reader.access(), Access::ReadOnly);
    submit(
        &mut reader,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(1),
        },
    );
    submit(&mut reader, put(OVERWORLD, record(key(0, 0, 0, 0), 1, 2)));
    wait_for_event(&mut reader, "the rejected write", |event| {
        matches!(
            event,
            ActorEvent::RequestFailed {
                error: StoreError::ReadOnly,
                ..
            }
        )
    });
    reader.shutdown(Minute::new(2));
    writer.shutdown(Minute::new(2));
}

#[test]
fn read_slots_are_bounded_and_freed_by_polling() {
    let dir = tempfile::tempdir().expect("temp dir");
    let config = ActorConfig {
        reads_in_flight: NonZeroUsize::new(2).expect("non-zero"),
        ..ActorConfig::default()
    };
    let mut actor = spawn(dir.path(), config);
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(1),
        },
    );
    let target = key(0, 0, 0, 0);
    let first = actor
        .try_read(OVERWORLD, target, Minute::new(1))
        .expect("slot");
    actor
        .try_read(OVERWORLD, target, Minute::new(1))
        .expect("slot");
    assert!(matches!(
        actor.try_read(OVERWORLD, target, Minute::new(1)),
        Err(SubmitError::Full(()))
    ));
    assert_eq!(result_of(&mut actor, first), ReadOutcome::Missing);
    eventually("both slots free", || {
        let mut results = Vec::new();
        actor.poll_results(&mut results);
        actor.reads_outstanding() == 0
    });
    actor
        .try_read(OVERWORLD, target, Minute::new(1))
        .expect("slot again");
    actor.shutdown(Minute::new(2));
}

#[test]
fn a_stopped_thread_fails_outstanding_reads_and_hands_requests_back() {
    let (commands, command_rx) = mpsc::sync_channel(1);
    let (result_tx, results) = mpsc::sync_channel::<ReadResult>(4);
    let (_event_tx, events) = mpsc::sync_channel(4);
    let dir = tempfile::tempdir().expect("temp dir");
    let registry = Store::open(dir.path(), Minute::new(1), StoreOptions::default())
        .expect("open")
        .registry();
    let mut actor = StoreActor {
        commands: Some(commands),
        shutdown_time: Arc::default(),
        results,
        events,
        dropped_events: Arc::default(),
        outstanding: HashMap::default(),
        reads_in_flight: 4,
        next_ticket: 0,
        registry,
        access: Access::ReadWrite,
        thread: None,
    };
    let target = key(0, 0, 0, 0);
    let ticket = actor
        .try_read(OVERWORLD, target, Minute::new(1))
        .expect("queued");
    let request = Request::End {
        dimension: OVERWORLD,
    };
    assert!(matches!(
        actor.try_submit(request),
        Err(SubmitError::Full(Request::End { .. }))
    ));
    drop((command_rx, result_tx));
    let mut out = Vec::new();
    actor.poll_results(&mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].ticket, ticket);
    assert!(matches!(out[0].outcome, ReadOutcome::Failed(_)));
    assert_eq!(actor.reads_outstanding(), 0);
    assert!(matches!(
        actor.try_submit(Request::End {
            dimension: OVERWORLD
        }),
        Err(SubmitError::Stopped(_))
    ));
}

#[test]
fn closing_signals_when_the_store_can_be_opened_as_writer_again() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut actor = spawn(dir.path(), ActorConfig::default());
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(1),
        },
    );
    submit(&mut actor, put(OVERWORLD, record(key(0, 0, 0, 0), 1, 2)));
    let closing = actor.close(Minute::new(2));
    eventually("the closed store", || closing.is_finished());
    let root = StoreRoot::new(dir.path());
    let store = root
        .open(&store_key(), Minute::new(3), StoreOptions::default())
        .expect("reopen");
    assert_eq!(store.access(), Access::ReadWrite);
    assert!(
        !closing
            .wait()
            .iter()
            .any(|event| matches!(event, ActorEvent::RequestFailed { .. }))
    );
}

#[test]
fn chunk_hashes_are_read_through_the_actor() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut actor = spawn(dir.path(), ActorConfig::default());
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(1),
        },
    );
    let ticket = actor
        .try_read_chunk_hash(OVERWORLD, 3, -4, Minute::new(1))
        .expect("read");
    assert_eq!(result_of(&mut actor, ticket), ReadOutcome::ChunkHash(None));
    submit(
        &mut actor,
        Request::ChunkHash {
            dimension: OVERWORLD,
            x: 3,
            z: -4,
            hash: Some(9),
        },
    );
    let ticket = actor
        .try_read_chunk_hash(OVERWORLD, 3, -4, Minute::new(1))
        .expect("read");
    assert_eq!(
        result_of(&mut actor, ticket),
        ReadOutcome::ChunkHash(Some(9))
    );
    actor.shutdown(Minute::new(2));
}

#[test]
fn shutting_down_never_blocks_on_a_full_queue() {
    let (commands, queue) = mpsc::sync_channel(1);
    commands
        .try_send(Command::Request(Request::End {
            dimension: OVERWORLD,
        }))
        .expect("fills the queue");
    let time = ShutdownTime::default();
    request_shutdown(commands, Some(Minute::new(7)), &time);
    assert!(matches!(queue.recv(), Ok(Command::Request(_))));
    assert!(queue.recv().is_err(), "the channel closed instead");
    assert_eq!(time.get(), Some(Minute::new(7)));
    time.set(None);
    assert_eq!(time.get(), None);
}

#[test]
fn a_thread_whose_channel_closed_closes_the_store_at_the_handles_time() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = StoreRoot::new(dir.path());
    let expired = root.store_dir(&crate::identity::StoreKey::new("remote:old", "", 0));
    drop(Store::open(&expired, Minute::new(0), StoreOptions::default()).expect("old store"));
    let store = root
        .open(&store_key(), Minute::new(1), StoreOptions::default())
        .expect("open");
    let (commands, command_rx) = mpsc::sync_channel(4);
    let (results, _results) = mpsc::sync_channel(1);
    let (events, _events) = mpsc::sync_channel(16);
    let shutdown_time = Arc::new(ShutdownTime::default());
    let worker = worker::Worker {
        store: Some(store),
        root: root.clone(),
        quotas: Quotas::default(),
        commands: command_rx,
        results,
        events,
        dropped_events: Arc::default(),
        shutdown_time: Arc::clone(&shutdown_time),
        batch: 4,
    };
    let now = Minute::new(1 + crate::root::STORE_EXPIRY_MINUTES + 1);
    shutdown_time.set(Some(now));
    drop(commands);
    worker.run();
    assert!(
        !expired.exists(),
        "closing enforced the quotas at the handle's time"
    );
    assert!(root.store_dir(&store_key()).exists());
}

#[test]
fn a_sync_request_commits_queued_writes_while_the_thread_runs() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut actor = spawn(dir.path(), ActorConfig::default());
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(1),
        },
    );
    let target = key(0, 2, 0, 2);
    submit(&mut actor, put(OVERWORLD, record(target, 4, 2)));
    submit(
        &mut actor,
        Request::Sync {
            now: Minute::new(2),
        },
    );
    let epoch = EpochRef {
        dimension: OVERWORLD,
        id: EpochId(0),
    };
    let dir = StoreRoot::new(dir.path()).store_dir(&store_key());
    eventually("the synced record on disk", || {
        let mut set = crate::region_set::RegionSet::open(
            epoch.dir(&dir),
            Access::ReadOnly,
            crate::region_set::DEFAULT_OPEN_REGIONS,
        );
        set.get(target).ok().flatten() == Some(record(target, 4, 2))
    });
    actor.shutdown(Minute::new(3));
}

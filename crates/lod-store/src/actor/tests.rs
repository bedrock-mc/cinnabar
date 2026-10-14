use std::{
    collections::HashMap,
    num::NonZeroUsize,
    path::Path,
    sync::{Arc, mpsc},
};

use test_time::eventually;

use super::{handle::request_shutdown, *};
use crate::{
    epochs::{EpochId, EpochRef},
    error::StoreError,
    identity::Dimension,
    minute::Minute,
    region_file::Access,
    region_id::RegionId,
    root::{Quotas, StoreRoot},
    store::{EpochStatus, Store, StoreEvent, StoreOptions},
    test_support::{flip_byte, key, record, surface},
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

/// Polls events until a quota report arrives; its evicted region bytes.
fn quota_eviction(actor: &mut StoreActor, what: &str) -> u64 {
    let mut evicted = None;
    eventually(what, || {
        let mut events = Vec::new();
        actor.poll_events(&mut events);
        for event in events {
            if let ActorEvent::Quota(report) = event {
                evicted = Some(report.evicted_region_bytes);
            }
        }
        evicted.is_some()
    });
    evicted.expect("found above")
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
fn changed_quotas_apply_to_the_next_quota_pass() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut actor = spawn(dir.path(), ActorConfig::default());
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(1),
        },
    );
    submit(&mut actor, put(OVERWORLD, record(key(0, 0, 0, 0), 1, 3)));
    let enforce = Request::EnforceQuotas {
        now: Minute::new(1),
        focus: None,
    };
    submit(&mut actor, enforce.clone());
    assert_eq!(
        quota_eviction(&mut actor, "the default quota report"),
        0,
        "the default cap keeps a small store"
    );
    submit(
        &mut actor,
        Request::SetQuotas(Quotas {
            store_bytes: 0,
            ..Quotas::default()
        }),
    );
    submit(&mut actor, enforce);
    let evicted = quota_eviction(&mut actor, "the reduced quota report");
    assert!(evicted > 0, "the reduced cap evicts the store's regions");
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
        clients: Arc::new(client::ClientSlot::new(commands.clone())),
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
fn a_failed_section_write_keeps_the_chunk_hashes_under_it_cleared() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut actor = spawn(dir.path(), ActorConfig::default());
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(1),
        },
    );
    // A directory where the level-1 region file belongs makes every write there fail.
    let lost = key(1, 0, 0, 0);
    let epoch = EpochRef {
        dimension: OVERWORLD,
        id: EpochId(0),
    };
    let store_dir = StoreRoot::new(dir.path()).store_dir(&store_key());
    std::fs::create_dir_all(
        epoch
            .dir(&store_dir)
            .join(RegionId::of_key(lost).0.file_name()),
    )
    .expect("blocking directory");
    submit(&mut actor, put(OVERWORLD, record(lost, 1, 2)));
    let hash = |x, z| Request::ChunkHash {
        dimension: OVERWORLD,
        x,
        z,
        hash: Some(7),
    };
    // Chunks 0..4 lie under the level-1 section at the origin; chunk 4 does not.
    for (x, z) in [(0, 0), (3, 3), (4, 0)] {
        submit(&mut actor, hash(x, z));
    }
    wait_for_event(&mut actor, "the failed write", |event| {
        matches!(event, ActorEvent::RequestFailed { .. })
    });
    for (x, z, expected) in [(0, 0, None), (3, 3, None), (4, 0, Some(7))] {
        let ticket = actor
            .try_read_chunk_hash(OVERWORLD, x, z, Minute::new(1))
            .expect("read");
        assert_eq!(
            result_of(&mut actor, ticket),
            ReadOutcome::ChunkHash(expected),
            "chunk ({x}, {z})"
        );
    }
    // A later hash under the lost section stays cleared until the session begins again.
    submit(&mut actor, hash(1, 2));
    let ticket = actor
        .try_read_chunk_hash(OVERWORLD, 1, 2, Minute::new(1))
        .expect("read");
    assert_eq!(result_of(&mut actor, ticket), ReadOutcome::ChunkHash(None));
    actor.shutdown(Minute::new(2));
}

#[test]
fn clients_write_and_read_from_other_threads_until_the_handle_closes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let actor = spawn(dir.path(), ActorConfig::default());
    let client = actor.client();
    let target = key(0, 2, 0, -1);
    let worker = std::thread::spawn({
        let client = client.clone();
        move || {
            client
                .try_submit(Request::Begin {
                    dimension: OVERWORLD,
                    now: Minute::new(1),
                })
                .expect("queued");
            client
                .try_submit(put(OVERWORLD, record(target, 4, 3)))
                .expect("queued");
            client
                .read(OVERWORLD, ReadTarget::Section(target), Minute::new(1))
                .expect("answered")
        }
    });
    assert_eq!(
        worker.join().expect("client thread"),
        ReadOutcome::Found(record(target, 4, 3)),
        "a client read sees the client's earlier write"
    );
    let closing = actor.close(Minute::new(2));
    assert!(matches!(
        client.read(OVERWORLD, ReadTarget::Section(target), Minute::new(2)),
        Err(SubmitError::Stopped(()))
    ));
    assert!(matches!(
        client.try_submit(Request::End {
            dimension: OVERWORLD
        }),
        Err(SubmitError::Stopped(_))
    ));
    eventually("the closed store, though a client remains", || {
        closing.is_finished()
    });
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
        unwritten: Default::default(),
        changes: Arc::default(),
        hidden: Arc::default(),
        seen: SessionEpoch(0),
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

/// Reads `target` through `client` after every earlier write.
fn own_section(client: &StoreClient, target: lod::geometry::SectionKey) -> ReadOutcome {
    client
        .read(OVERWORLD, ReadTarget::OwnSection(target), Minute::new(1))
        .expect("read queued")
}

#[test]
fn epoch_writes_prepared_before_a_session_change_are_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut actor = spawn(dir.path(), ActorConfig::default());
    let client = actor.client();
    let begin = Request::Begin {
        dimension: OVERWORLD,
        now: Minute::new(1),
    };
    let before = client.session_epoch();
    submit(&mut actor, begin.clone());
    let begun = client.session_epoch();
    assert!(begun > before, "a queued begin is a change at once");
    let (first, second) = (key(0, 1, 0, 1), key(0, 2, 0, 2));
    assert!(matches!(
        client.write_in_epoch(begun, vec![put(OVERWORLD, record(first, 1, 2))]),
        Ok(EpochWrite::Taken)
    ));
    submit(
        &mut actor,
        Request::End {
            dimension: OVERWORLD,
        },
    );
    submit(&mut actor, begin);
    assert!(matches!(
        client.write_in_epoch(begun, vec![put(OVERWORLD, record(second, 2, 2))]),
        Ok(EpochWrite::Stale)
    ));
    assert_eq!(own_section(&client, second), ReadOutcome::Missing);
    assert!(matches!(
        client.write_in_epoch(
            client.session_epoch(),
            vec![put(OVERWORLD, record(second, 2, 2))]
        ),
        Ok(EpochWrite::Taken)
    ));
    assert!(matches!(
        own_section(&client, second),
        ReadOutcome::Found(_)
    ));
    actor.shutdown(Minute::new(2));
}

#[test]
fn a_merge_the_thread_decides_refuses_writes_prepared_in_the_candidate() {
    let dir = tempfile::tempdir().expect("temp dir");
    {
        let mut actor = spawn(dir.path(), ActorConfig::default());
        submit(
            &mut actor,
            Request::Begin {
                dimension: OVERWORLD,
                now: Minute::new(1),
            },
        );
        submit_world(&mut actor);
        actor.shutdown(Minute::new(1));
    }
    let mut actor = spawn(dir.path(), ActorConfig::default());
    let client = actor.client();
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(2),
        },
    );
    let candidate = client.session_epoch();
    submit_world(&mut actor);
    wait_for_event(&mut actor, "the candidate to merge", |event| {
        matches!(
            event,
            ActorEvent::Store(StoreEvent::Epoch {
                status: EpochStatus::Merged { .. },
                ..
            })
        )
    });
    assert!(client.session_epoch() > candidate);
    let target = key(0, 3, 0, 3);
    assert!(matches!(
        client.write_in_epoch(candidate, vec![put(OVERWORLD, record(target, 3, 2))]),
        Ok(EpochWrite::Stale)
    ));
    assert_eq!(own_section(&client, target), ReadOutcome::Missing);
    actor.shutdown(Minute::new(2));
}

#[test]
fn a_region_hidden_from_read_through_moves_the_view_epoch_before_reads_see_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let target = key(0, 1, 2, 1);
    {
        let mut actor = spawn(dir.path(), ActorConfig::default());
        submit(
            &mut actor,
            Request::Begin {
                dimension: OVERWORLD,
                now: Minute::new(1),
            },
        );
        submit_world(&mut actor);
        submit(&mut actor, put(OVERWORLD, record(target, 1, 2)));
        actor.shutdown(Minute::new(1));
    }
    let mut actor = spawn(dir.path(), ActorConfig::default());
    let client = actor.client();
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(2),
        },
    );
    let begun = client.view_epoch();
    let read = || {
        client
            .read(OVERWORLD, ReadTarget::Section(target), Minute::new(2))
            .expect("read queued")
    };
    assert!(matches!(read(), ReadOutcome::Found(_)), "read through");
    assert_eq!(client.view_epoch(), begun);
    // Another world in the target's signature region: one mismatch decides nothing.
    submit_region(&mut actor, 0, 2);
    assert_eq!(read(), ReadOutcome::Missing, "hidden at once");
    let hidden = client.view_epoch();
    assert_eq!(
        hidden,
        ViewEpoch {
            hidden: begun.hidden + 1,
            ..begun
        }
    );
    assert_eq!(
        client.view_changes(begun),
        (
            hidden,
            ViewChange::Hidden(vec![(OVERWORLD, lod::epoch::RegionPos { x: 0, z: 0 })])
        ),
        "the mismatching region is named"
    );
    actor.shutdown(Minute::new(2));
}

#[test]
fn a_split_leading_epoch_changes_the_whole_view_but_not_the_session() {
    let dir = tempfile::tempdir().expect("temp dir");
    let config = ActorConfig {
        requests: NonZeroUsize::new(8192).expect("non-zero"),
        ..ActorConfig::default()
    };
    let (older, newer) = (key(0, 1, 2, 3), key(0, 2, 2, 1));
    // Epoch 0 holds world 1 over regions 0..110; epoch 1, split from it, world 2 over
    // regions 0..220.
    for (now, seed, regions, stored) in [(1, 1, 110, older), (2, 2, 220, newer)] {
        let mut actor = spawn(dir.path(), config);
        submit(
            &mut actor,
            Request::Begin {
                dimension: OVERWORLD,
                now: Minute::new(now),
            },
        );
        for region in 0..regions {
            submit_region(&mut actor, region, seed);
        }
        submit(&mut actor, put(OVERWORLD, record(stored, seed, 2)));
        actor.shutdown(Minute::new(now));
    }
    let mut actor = spawn(dir.path(), config);
    let client = actor.client();
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(3),
        },
    );
    let read = |target| {
        client
            .read(OVERWORLD, ReadTarget::Section(target), Minute::new(3))
            .expect("read queued")
    };
    assert!(
        matches!(read(newer), ReadOutcome::Found(_)),
        "epoch 1 leads"
    );
    assert_eq!(read(older), ReadOutcome::Missing);
    let (view, session) = (client.view_epoch(), client.session_epoch());
    // A third world where only epoch 1 has signatures splits from it and leaves epoch 0
    // undecided.
    for region in 110..220 {
        submit_region(&mut actor, region, 3);
    }
    assert!(
        matches!(read(older), ReadOutcome::Found(_)),
        "epoch 0 leads"
    );
    assert_eq!(read(newer), ReadOutcome::Missing);
    let (now, change) = client.view_changes(view);
    assert_eq!(change, ViewChange::All);
    assert_eq!(now.leading, view.leading + 1);
    assert_eq!(
        client.session_epoch(),
        session,
        "the session's own writes stay current"
    );
    actor.shutdown(Minute::new(3));
}

#[test]
fn a_read_batch_answers_every_target_in_order_after_earlier_writes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut actor = spawn(dir.path(), ActorConfig::default());
    let client = actor.client();
    submit(
        &mut actor,
        Request::Begin {
            dimension: OVERWORLD,
            now: Minute::new(1),
        },
    );
    let (first, second, absent) = (key(0, 1, 0, 1), key(1, 2, 0, 2), key(0, 5, 0, 5));
    let (first_record, second_record) = (record(first, 1, 2), record(second, 2, 2));
    submit(&mut actor, put(OVERWORLD, first_record.clone()));
    submit(&mut actor, put(OVERWORLD, second_record.clone()));
    let targets = vec![
        ReadTarget::Section(second),
        ReadTarget::Section(absent),
        ReadTarget::OwnSection(first),
        ReadTarget::ChunkHash { x: 9, z: 9 },
    ];
    assert_eq!(
        client
            .read_batch(OVERWORLD, targets, Minute::new(1))
            .expect("queued"),
        [
            ReadOutcome::Found(second_record),
            ReadOutcome::Missing,
            ReadOutcome::Found(first_record),
            ReadOutcome::ChunkHash(None),
        ]
    );
    assert_eq!(
        client
            .read_batch(OVERWORLD, Vec::new(), Minute::new(1))
            .expect("queued"),
        []
    );
    actor.shutdown(Minute::new(2));
}

/// Records one world's surface over enough signature regions for a merge vote.
fn submit_world(actor: &mut StoreActor) {
    for region in 0..12 {
        submit_region(actor, region, 1);
    }
}

/// Records world `seed`'s surface over every chunk of signature region `(region, 0)`.
fn submit_region(actor: &mut StoreActor, region: i32, seed: u32) {
    for chunk in 0..16 {
        let (x, z) = (region * 4 + chunk % 4, chunk / 4);
        submit(
            actor,
            Request::Surface {
                dimension: OVERWORLD,
                chunk_x: x,
                chunk_z: z,
                surface: Box::new(surface(seed, x, z)),
                now: Minute::new(1),
            },
        );
    }
}

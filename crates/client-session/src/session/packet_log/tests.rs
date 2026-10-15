use std::ffi::OsStr;
use std::path::Path;
use std::time::{Duration, Instant};

use bytes::Bytes;
use protocol::capture::{
    CAPTURE_HEADER_BYTES, CaptureFile, CaptureHeader, CaptureRecord, CaptureWriter,
    RECORD_HEADER_BYTES, read_capture,
};
use protocol::session_wire::{CapturedPacketKind, captured_packet_kind};

use super::capture::channel;
use super::samples::FailureSamples;
use super::*;

/// Generous: the writer finishes as soon as it has written a handful of records.
const FINISH_WITHIN: Duration = Duration::from_secs(60);

/// A log of this test's own, apart from the process-wide one.
fn fresh_log() -> Box<PacketLog> {
    Box::new(PacketLog::new())
}

/// The pinned protocol's StartGame ID.
fn start_game() -> u32 {
    (0..0x400)
        .find(|id| captured_packet_kind(*id) == CapturedPacketKind::StartGame)
        .expect("the pinned protocol has StartGame")
}

/// The lowest packet ID whose body may carry credentials or content keys.
fn secret_packet_id() -> u32 {
    (0..0x400)
        .find(|id| carries_secrets(*id))
        .expect("the pinned protocol has credential-bearing packets")
}

fn packet(packet_id: u32, elapsed_us: u64, body: &'static [u8]) -> CaptureRecord {
    CaptureRecord::Packet {
        direction: PacketDirection::Clientbound,
        packet_id,
        elapsed_us,
        body: Bytes::from_static(body),
    }
}

fn header(started_unix_ms: u64) -> CaptureHeader {
    CaptureHeader {
        protocol_version: protocol::PROTOCOL_VERSION,
        started_unix_ms,
    }
}

fn read_file(path: &Path) -> CaptureFile {
    read_capture(Bytes::from(std::fs::read(path).unwrap())).unwrap()
}

#[test]
fn the_ring_keeps_the_newest_records_in_order_across_wraparound() {
    let log = fresh_log();
    let total = RING_CAPACITY as u64 + 10;
    for index in 0..total {
        let direction = PacketDirection::ALL[index as usize % 2];
        let sequence = log.record(
            direction,
            (index % 0x400) as u32,
            index as usize,
            index * 3,
            PacketOutcome::Accepted,
        );
        assert_eq!(sequence, index + 1);
    }
    let headers = log.headers();
    assert_eq!(headers.len(), RING_CAPACITY);
    for (offset, header) in headers.iter().enumerate() {
        let index = 10 + offset as u64;
        assert_eq!(
            *header,
            HeaderRecord {
                sequence: index + 1,
                direction: PacketDirection::ALL[index as usize % 2],
                packet_id: (index % 0x400) as u32,
                len: index as u32,
                elapsed_ms: index * 3,
                outcome: PacketOutcome::Accepted,
            }
        );
    }
}

#[test]
fn header_fields_survive_packing_at_their_limits() {
    let log = fresh_log();
    log.record(
        PacketDirection::Serverbound,
        0x3ff,
        usize::MAX,
        u64::MAX,
        PacketOutcome::Skipped,
    );
    assert_eq!(
        log.headers(),
        [HeaderRecord {
            sequence: 1,
            direction: PacketDirection::Serverbound,
            packet_id: 0x3ff,
            len: u32::MAX,
            elapsed_ms: u64::MAX,
            outcome: PacketOutcome::Skipped,
        }]
    );
}

/// Two writers racing for one slot, a full ring apart: a slot only moves forward, and a reader
/// never sees one record's stamp over another's fields.
#[test]
fn a_stalled_writer_never_overwrites_or_mixes_with_a_newer_record() {
    let log = fresh_log();
    let ring = &log.ring;
    let publish = |sequence: u64| {
        ring.publish(
            sequence,
            PacketDirection::Clientbound,
            (sequence % 0x400) as u32,
            sequence as u32,
            sequence,
            PacketOutcome::Accepted,
        );
    };
    let record = || {
        let sequence = ring.reserve();
        if ring.begin(sequence) {
            publish(sequence);
        }
        sequence
    };
    let consistent = |headers: &[HeaderRecord]| {
        headers.iter().all(|header| {
            header.packet_id == (header.sequence % 0x400) as u32
                && u64::from(header.len) == header.sequence
                && header.elapsed_ms == header.sequence
        })
    };

    // A writer that took its sequence number, then stalled for a whole ring.
    let stalled = ring.reserve();
    for _ in 1..RING_CAPACITY {
        record();
    }
    let newer = record();
    assert_eq!(newer, stalled + RING_CAPACITY as u64);
    assert!(!ring.begin(stalled), "a stale writer must give up its slot");
    let headers = log.headers();
    assert_eq!(headers.last().map(|header| header.sequence), Some(newer));
    assert!(consistent(&headers));

    // A writer stalled mid-write keeps its slot: the newer record is given up, not mixed in.
    let slow = ring.reserve();
    assert!(ring.begin(slow));
    for _ in 1..RING_CAPACITY {
        record();
    }
    let blocked = ring.reserve();
    assert_eq!(blocked, slow + RING_CAPACITY as u64);
    assert!(!ring.begin(blocked));
    let headers = log.headers();
    assert!(headers.iter().all(|header| header.sequence != blocked));
    assert_eq!(headers.len(), RING_CAPACITY - 1);
    publish(slow);
    let headers = log.headers();
    assert!(headers.iter().all(|header| header.sequence != slow));
    assert!(consistent(&headers));
    // Once published, the slot takes the next record a ring later.
    assert!(ring.begin(blocked + RING_CAPACITY as u64));
}

#[test]
fn outcomes_settle_only_the_record_they_name() {
    let log = fresh_log();
    let records: Vec<_> = PacketOutcome::ALL
        .iter()
        .map(|_| {
            log.record(
                PacketDirection::Clientbound,
                9,
                1,
                0,
                PacketOutcome::Accepted,
            )
        })
        .collect();
    for (sequence, outcome) in records.iter().zip(PacketOutcome::ALL) {
        log.reject(1, *sequence, 9, outcome, &[]);
    }
    let outcomes: Vec<_> = log.headers().iter().map(|header| header.outcome).collect();
    assert_eq!(outcomes, PacketOutcome::ALL);

    // Once the ring overwrites a record, a late outcome for it changes nothing.
    for _ in 0..RING_CAPACITY {
        log.record(
            PacketDirection::Clientbound,
            7,
            1,
            0,
            PacketOutcome::Accepted,
        );
    }
    log.reject(1, records[0], 9, PacketOutcome::Failed, &[]);
    assert!(
        log.headers()
            .iter()
            .all(|header| header.packet_id == 7 && header.outcome == PacketOutcome::Accepted)
    );
}

#[test]
fn samples_keep_four_bodies_per_packet_and_stay_within_the_byte_bound() {
    let mut samples = FailureSamples::new();
    for sequence in 0..6 {
        samples.offer(1, sequence, 9, PacketOutcome::Failed, &[sequence as u8; 3]);
    }
    assert_eq!(samples.samples().len(), MAX_SAMPLES_PER_PACKET);
    assert_eq!(samples.dropped(), 2);
    assert_eq!(
        samples
            .samples()
            .iter()
            .map(|sample| sample.sequence)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );

    samples.offer(1, 10, secret_packet_id(), PacketOutcome::Failed, &[1, 2]);
    assert_eq!(samples.withheld(), 1);
    assert_eq!(samples.samples().len(), MAX_SAMPLES_PER_PACKET);

    let room = MAX_SAMPLE_BYTES - samples.bytes();
    samples.offer(1, 11, 20, PacketOutcome::Skipped, &vec![5; room - 10]);
    samples.offer(1, 12, 21, PacketOutcome::Failed, &[6; 100]);
    let last = samples.samples().last().unwrap();
    assert_eq!((last.sequence, last.len, last.body.len()), (12, 100, 10));
    assert_eq!(samples.bytes(), MAX_SAMPLE_BYTES);
    samples.offer(1, 13, 22, PacketOutcome::Failed, &[7]);
    assert_eq!(samples.dropped(), 3);
    assert_eq!(samples.samples().len(), MAX_SAMPLES_PER_PACKET + 2);
}

/// Each session gets its own budget; a newer session's first sample replaces the older one's.
#[test]
fn samples_and_their_bounds_belong_to_one_session() {
    let mut samples = FailureSamples::new();
    for sequence in 0..=MAX_SAMPLES_PER_PACKET as u64 {
        samples.offer(1, sequence, 9, PacketOutcome::Failed, &[1]);
    }
    samples.offer(1, 9, secret_packet_id(), PacketOutcome::Failed, &[1]);
    assert_eq!((samples.dropped(), samples.withheld()), (1, 1));

    // The next session keeps none of the first's samples or counts once it rejects a packet.
    samples.offer(2, 20, 9, PacketOutcome::Skipped, &[2, 2]);
    assert_eq!(samples.session(), 2);
    assert_eq!(
        samples
            .samples()
            .iter()
            .map(|sample| (sample.session, sample.sequence, sample.body.clone()))
            .collect::<Vec<_>>(),
        [(2, 20, vec![2, 2])]
    );
    assert_eq!(
        (samples.bytes(), samples.dropped(), samples.withheld()),
        (2, 0, 0)
    );

    // A late rejection from the older session changes nothing.
    samples.offer(1, 21, 9, PacketOutcome::Failed, &[3]);
    assert_eq!(samples.samples().len(), 1);
}

#[test]
fn dump_lists_headers_by_name_and_summarises_samples() {
    let log = fresh_log();
    let start_game = start_game();
    let session = log.begin_session();
    let start = log.record(
        PacketDirection::Clientbound,
        start_game,
        58,
        1234,
        PacketOutcome::Accepted,
    );
    log.record(
        PacketDirection::Serverbound,
        0x3ff,
        4,
        1240,
        PacketOutcome::Accepted,
    );
    log.reject(
        session,
        start,
        start_game,
        PacketOutcome::Failed,
        &[1, 2, 3],
    );
    let mut out = Vec::new();
    log.dump(&mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines[0], "packet headers: 2 recorded, last 2 kept");
    let name = format!("{} ({start_game})", PacketName(start_game));
    assert!(lines[2].ends_with(&name), "{text}");
    for field in ["1", "1234", "clientbound", "failed", "58"] {
        assert!(
            lines[2].split_whitespace().any(|word| word == field),
            "{text}"
        );
    }
    assert!(
        lines[3].contains("serverbound") && lines[3].contains("accepted"),
        "{text}"
    );
    assert_eq!(
        lines[4],
        format!(
            "rejected packet samples from session {session}: 1 kept (3 bytes), 0 withheld as credential-bearing, 0 over the bounds"
        )
    );
}

/// A crash on the thread updating the samples must not hang the dump.
#[test]
fn dumps_never_wait_for_samples_another_caller_holds() {
    let log = fresh_log();
    let start = log.record(
        PacketDirection::Clientbound,
        9,
        1,
        0,
        PacketOutcome::Accepted,
    );
    log.reject(1, start, 9, PacketOutcome::Failed, &[1]);
    let held = log.lock_samples();
    let mut out = Vec::new();
    log.dump(&mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains(" failed "), "{text}");
    assert!(
        text.ends_with("rejected packet samples: unavailable while being updated\n"),
        "{text}"
    );
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(log.dump_samples(&dir.path().join("packets")).unwrap(), 0);
    drop(held);
    assert_eq!(log.dump_samples(&dir.path().join("packets")).unwrap(), 1);
}

#[test]
fn dump_samples_writes_each_body_and_an_index() {
    let log = fresh_log();
    let start_game = start_game();
    let start = log.record(
        PacketDirection::Clientbound,
        start_game,
        3,
        0,
        PacketOutcome::Accepted,
    );
    log.reject(3, start, start_game, PacketOutcome::Skipped, &[4, 5, 6]);
    let dir = tempfile::tempdir().unwrap();
    let samples = dir.path().join("packets");
    assert_eq!(log.dump_samples(&samples).unwrap(), 1);
    let name = format!("{start}-{}.bin", PacketName(start_game));
    assert_eq!(std::fs::read(samples.join(&name)).unwrap(), [4, 5, 6]);
    let index = std::fs::read_to_string(samples.join("index.txt")).unwrap();
    assert!(
        index.starts_with(&name) && index.contains("session 3") && index.contains("skipped"),
        "{index}"
    );
}

#[test]
fn capture_setting_is_off_by_default_and_resolves_both_path_forms() {
    let logs = Path::new("logs");
    for off in [None, Some(""), Some("0")] {
        assert_eq!(
            PacketCaptureRequest::from_setting(off.map(OsStr::new), logs, 5),
            None
        );
    }
    assert_eq!(
        PacketCaptureRequest::from_setting(Some(OsStr::new("1")), logs, 1_760_000_000_123),
        Some(PacketCaptureRequest {
            path: logs.join(format!("packets-1760000000123.{CAPTURE_EXTENSION}")),
            started_unix_ms: 1_760_000_000_123,
        })
    );
    assert_eq!(
        PacketCaptureRequest::from_setting(Some(OsStr::new("custom/run.bin")), logs, 7),
        Some(PacketCaptureRequest {
            path: "custom/run.bin".into(),
            started_unix_ms: 7,
        })
    );
}

#[test]
fn a_full_queue_drops_packets_and_reports_them_in_a_gap_before_the_next() {
    let started = Instant::now();
    let (feed, queue, _stop) = channel(2, started);
    let body = Bytes::from_static(&[1, 2]);
    let at = |micros| started + Duration::from_micros(micros);
    let send = |packet_id, micros| {
        feed.packet(
            PacketDirection::Clientbound,
            packet_id,
            at(micros),
            PacketBody::whole(&body),
        );
    };
    for micros in 0..5 {
        send(9, micros);
    }
    assert_eq!(
        queue.take_queued(),
        [packet(9, 0, &[1, 2]), packet(9, 1, &[1, 2])]
    );
    send(10, 40);
    assert_eq!(
        queue.take_queued(),
        [
            CaptureRecord::Gap {
                elapsed_us: 40,
                dropped: 3
            },
            packet(10, 40, &[1, 2]),
        ]
    );

    // Packets dropped at the end of a session reach the capture in the writer's final gap,
    // without the feed ever waiting for room.
    for micros in 50..53 {
        send(9, micros);
    }
    drop(feed);
    let written = queue
        .drain(CaptureWriter::new(Vec::new(), header(1)).unwrap())
        .unwrap();
    let file = read_capture(Bytes::from(written)).unwrap();
    assert!(!file.cut_short);
    assert!(matches!(
        file.records[..],
        [
            CaptureRecord::Packet { .. },
            CaptureRecord::Packet { .. },
            CaptureRecord::Gap { dropped: 1, .. }
        ]
    ));
}

/// Teardown can stop the writer while tasks still hold the feed: what is queued is written,
/// then the drop count.
#[test]
fn a_stop_writes_the_queued_records_and_the_last_gap_while_feeds_live() {
    let started = Instant::now();
    let (feed, queue, stop) = channel(1, started);
    let body = Bytes::from_static(&[3]);
    for micros in 0..3 {
        feed.packet(
            PacketDirection::Serverbound,
            9,
            started + Duration::from_micros(micros),
            PacketBody::whole(&body),
        );
    }
    stop.send(()).unwrap();
    let written = queue
        .drain(CaptureWriter::new(Vec::new(), header(2)).unwrap())
        .unwrap();
    let file = read_capture(Bytes::from(written)).unwrap();
    assert!(matches!(
        file.records[..],
        [
            CaptureRecord::Packet {
                direction: PacketDirection::Serverbound,
                elapsed_us: 0,
                ..
            },
            CaptureRecord::Gap { dropped: 2, .. }
        ]
    ));
    // The finished writer has gone; the feed neither blocks nor fails.
    feed.packet(
        PacketDirection::Serverbound,
        9,
        started,
        PacketBody::whole(&body),
    );
}

#[test]
fn the_writer_stops_at_the_size_limit_with_a_final_marker() {
    let started = Instant::now();
    let (feed, queue, _stop) = channel(16, started);
    let body = Bytes::from_static(&[7; 10]);
    for micros in 0..5 {
        feed.packet(
            PacketDirection::Serverbound,
            9,
            started + Duration::from_micros(micros),
            PacketBody::whole(&body),
        );
    }
    let one = RECORD_HEADER_BYTES + 10;
    let limit = CAPTURE_HEADER_BYTES + 2 * one + RECORD_HEADER_BYTES;
    let writer = CaptureWriter::with_limit(Vec::new(), header(3), limit).unwrap();
    let written = queue.drain(writer).unwrap();
    assert_eq!(written.len() as u64, limit);
    let file = read_capture(Bytes::from(written)).unwrap();
    assert_eq!(file.header, header(3));
    assert_eq!(file.records.len(), 3);
    assert_eq!(file.records[2], CaptureRecord::Truncated { elapsed_us: 2 });
}

#[test]
fn recording_a_header_without_capture_does_not_allocate() {
    let log = fresh_log();
    let tap = SessionTap::new(&log, 1, Instant::now(), None);
    let body = Bytes::from_static(&[1, 2, 3, 4]);
    let start_game = start_game();
    let (record, allocations) = test_alloc::count(|| {
        let record = tap.record(
            PacketDirection::Clientbound,
            start_game,
            PacketBody::whole(&body),
        );
        tap.record(PacketDirection::Serverbound, 9, PacketBody::whole(&body));
        record
    });
    assert_eq!(allocations, 0);
    assert_eq!(record, PacketRecordId(1));
    assert_eq!(log.headers().len(), 2);
}

#[test]
fn a_session_capture_keeps_packets_but_never_credential_bearing_bodies() {
    let log = fresh_log();
    let start_game = start_game();
    let dir = tempfile::tempdir().unwrap();
    let request = PacketCaptureRequest {
        path: dir
            .path()
            .join("logs")
            .join(format!("session.{CAPTURE_EXTENSION}")),
        started_unix_ms: 1_760_000_000_000,
    };
    let started = Instant::now();
    let (feed, finisher, path) = CaptureFeed::start(&request, started).unwrap();
    assert_eq!(path, request.path);
    let session = log.begin_session();
    let tap = SessionTap::new(&log, session, started, Some(feed));
    let start_body = Bytes::from_static(&[1, 2, 3]);
    let reply = Bytes::from_static(&[4]);
    let secret = Bytes::from_static(b"content-key");
    let first = tap.record(
        PacketDirection::Clientbound,
        start_game,
        PacketBody::whole(&start_body),
    );
    tap.record(PacketDirection::Serverbound, 9, PacketBody::whole(&reply));
    let withheld = tap.record(
        PacketDirection::Clientbound,
        secret_packet_id(),
        PacketBody::whole(&secret),
    );
    tap.reject(
        first,
        start_game,
        PacketOutcome::Failed,
        PacketBody::whole(&start_body),
    );
    tap.reject(
        withheld,
        secret_packet_id(),
        PacketOutcome::Failed,
        PacketBody::whole(&secret),
    );
    tap.reject_batch(17);
    // Teardown finishes the file while the tap, as a lingering task would, still holds the feed.
    assert!(finisher.finish(FINISH_WITHIN));

    let file = read_file(&path);
    assert_eq!(file.header, header(request.started_unix_ms));
    assert!(!file.cut_short);
    let packets: Vec<_> = file
        .records
        .iter()
        .map(|record| match record {
            CaptureRecord::Packet {
                direction,
                packet_id,
                body,
                ..
            } => (*direction, *packet_id, body.to_vec()),
            other => panic!("unexpected capture record {other:?}"),
        })
        .collect();
    assert_eq!(
        packets,
        [
            (PacketDirection::Clientbound, start_game, vec![1, 2, 3]),
            (PacketDirection::Serverbound, 9, vec![4]),
        ]
    );
    let headers: Vec<_> = log
        .headers()
        .iter()
        .map(|header| (header.packet_id, header.len, header.outcome))
        .collect();
    assert_eq!(
        headers,
        [
            (start_game, 3, PacketOutcome::Failed),
            (9, 1, PacketOutcome::Accepted),
            (secret_packet_id(), 11, PacketOutcome::Failed),
            (UNFRAMED_BATCH_ID, 17, PacketOutcome::Failed),
        ]
    );
    assert_eq!(
        log.samples()
            .iter()
            .map(|sample| (sample.session, sample.packet_id, sample.body.clone()))
            .collect::<Vec<_>>(),
        [(session, start_game, vec![1, 2, 3])]
    );
    drop(tap);

    // A second session never replaces the first capture.
    let (feed, finisher, second) = CaptureFeed::start(&request, started).unwrap();
    drop(feed);
    assert!(finisher.finish(FINISH_WITHIN));
    assert_eq!(
        second,
        request.path.with_file_name(format!(
            "session-{}.{CAPTURE_EXTENSION}",
            request.started_unix_ms
        ))
    );
    assert_eq!(read_file(&path).records.len(), 2);
}

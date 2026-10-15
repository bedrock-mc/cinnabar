use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use bytes::Bytes;
use protocol::capture::{
    CAPTURE_HEADER_BYTES, CaptureFile, CaptureHeader, CaptureRecord, CaptureWriter,
    RECORD_HEADER_BYTES, read_capture,
};
use protocol::session_wire::{CapturedPacketKind, captured_packet_kind};

use super::capture::channel;
use protocol::packet_tap::PacketName;

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
    let total = packets::CAPACITY as u64 + 10;
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
    assert_eq!(headers.len(), packets::CAPACITY);
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
            PacketDirection::Clientbound as u8,
            (sequence % 0x400) as u32,
            sequence as u32,
            sequence,
            PacketOutcome::Accepted as u8,
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
    for _ in 1..packets::CAPACITY {
        record();
    }
    let newer = record();
    assert_eq!(newer, stalled + packets::CAPACITY as u64);
    assert!(!ring.begin(stalled), "a stale writer must give up its slot");
    let headers = log.headers();
    assert_eq!(headers.last().map(|header| header.sequence), Some(newer));
    assert!(consistent(&headers));

    // A writer stalled mid-write keeps its slot: the newer record is given up, not mixed in.
    let slow = ring.reserve();
    assert!(ring.begin(slow));
    for _ in 1..packets::CAPACITY {
        record();
    }
    let blocked = ring.reserve();
    assert_eq!(blocked, slow + packets::CAPACITY as u64);
    assert!(!ring.begin(blocked));
    let headers = log.headers();
    assert!(headers.iter().all(|header| header.sequence != blocked));
    assert_eq!(headers.len(), packets::CAPACITY - 1);
    publish(slow);
    let headers = log.headers();
    assert!(headers.iter().all(|header| header.sequence != slow));
    assert!(consistent(&headers));
    // Once published, the slot takes the next record a ring later.
    assert!(ring.begin(blocked + packets::CAPACITY as u64));
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
    for _ in 0..packets::CAPACITY {
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

/// The ring moves into the staging file with the headers recorded so far, and every later
/// header and outcome lands in the file a crash report reads.
#[test]
fn an_attached_log_keeps_its_headers_in_the_staging_ring_file() {
    let log = fresh_log();
    let staging = tempfile::tempdir().unwrap();
    let early = log.record(
        PacketDirection::Clientbound,
        start_game(),
        58,
        1234,
        PacketOutcome::Accepted,
    );
    log.attach(staging.path()).unwrap();
    let late = log.record(
        PacketDirection::Serverbound,
        0x3ff,
        4,
        1240,
        PacketOutcome::Accepted,
    );
    log.reject(1, late, 0x3ff, PacketOutcome::Skipped, &[9]);
    let file = std::fs::read(staging.path().join(staging::PACKET_RING_FILE)).unwrap();
    let image = packets::read(&file).unwrap();
    let headers: Vec<_> = image
        .headers
        .into_iter()
        .filter_map(HeaderRecord::from_ring)
        .collect();
    assert_eq!(image.recorded, 2);
    assert_eq!(headers, log.headers());
    assert_eq!(
        headers
            .iter()
            .map(|header| (header.sequence, header.outcome))
            .collect::<Vec<_>>(),
        [
            (early, PacketOutcome::Accepted),
            (late, PacketOutcome::Skipped)
        ]
    );
    assert_eq!(
        log.attach(staging.path()).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
}

/// Samples kept before the attach and after it are on disk with their index; the next session
/// to reject a packet replaces them there as in memory.
#[test]
fn attached_samples_follow_the_latest_session_on_disk() {
    let log = fresh_log();
    let start_game = start_game();
    let staging = tempfile::tempdir().unwrap();
    let dir = staging.path().join(staging::DECODE_SAMPLES_DIR);
    let first = log.record(
        PacketDirection::Clientbound,
        start_game,
        3,
        0,
        PacketOutcome::Accepted,
    );
    log.reject(3, first, start_game, PacketOutcome::Skipped, &[4, 5, 6]);
    log.attach(staging.path()).unwrap();
    let second = log.record(
        PacketDirection::Clientbound,
        9,
        2,
        0,
        PacketOutcome::Accepted,
    );
    log.reject(3, second, 9, PacketOutcome::Failed, &[7, 8]);
    log.reject(3, 0, secret_packet_id(), PacketOutcome::Failed, &[1]);
    log.samples_writer.get().unwrap().flush();
    let first_name = log.samples()[0].file_name();
    assert_eq!(
        first_name,
        format!("{first}-{}.bin", PacketName(start_game))
    );
    assert_eq!(std::fs::read(dir.join(&first_name)).unwrap(), [4, 5, 6]);
    let second_name = log.samples()[1].file_name();
    assert_eq!(std::fs::read(dir.join(&second_name)).unwrap(), [7, 8]);
    let index = std::fs::read_to_string(dir.join(samples::INDEX_FILE)).unwrap();
    let lines: Vec<_> = index.lines().collect();
    assert_eq!(
        lines[0],
        "rejected packet samples from session 3: 2 kept (5 bytes), 1 withheld as credential-bearing, 0 over the bounds"
    );
    assert!(
        lines[1].starts_with(&first_name) && lines[1].contains("skipped"),
        "{index}"
    );
    assert!(
        lines[2].contains("failed") && lines[2].contains("2 of 2 bytes"),
        "{index}"
    );

    let third = log.record(
        PacketDirection::Clientbound,
        9,
        1,
        0,
        PacketOutcome::Accepted,
    );
    log.reject(4, third, 9, PacketOutcome::Failed, &[1]);
    log.samples_writer.get().unwrap().flush();
    assert!(!dir.join(&first_name).exists());
    let index = std::fs::read_to_string(dir.join(samples::INDEX_FILE)).unwrap();
    assert!(
        index.starts_with("rejected packet samples from session 4: 1 kept"),
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
            in_log_dir: true,
        })
    );
    assert_eq!(
        PacketCaptureRequest::from_setting(Some(OsStr::new("custom/run.bin")), logs, 7),
        Some(PacketCaptureRequest {
            path: "custom/run.bin".into(),
            started_unix_ms: 7,
            in_log_dir: false,
        })
    );
}

/// Starts and finishes one session's capture, as a session that receives nothing would.
fn capture_session(request: &PacketCaptureRequest) {
    let (feed, finisher, _) = CaptureFeed::start(request, Instant::now()).unwrap();
    drop(feed);
    assert!(finisher.finish(FINISH_WITHIN));
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// With capture on across many sessions the log directory keeps only the newest captures;
/// its other files, and the directory of a capture path the user named, are left alone.
#[test]
fn log_dir_captures_keep_only_the_newest_across_sessions() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    let custom = root.path().join("custom");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::create_dir_all(&custom).unwrap();
    let others = [
        "client.log",
        "packets-notes.txt",
        "packets-12.bin",
        "packets-x.cnbrcap",
        "packets-+3.cnbrcap",
    ];
    for name in others {
        std::fs::write(logs.join(name), b"kept").unwrap();
    }
    let stale = 2 * LOG_DIR_CAPTURES_KEPT as u64;
    for started in 1..=stale {
        std::fs::write(
            custom.join(format!("packets-{started}.{CAPTURE_EXTENSION}")),
            b"",
        )
        .unwrap();
    }
    let named_path = custom.join(format!("packets-{}.{CAPTURE_EXTENSION}", stale + 1));
    let named = PacketCaptureRequest::from_setting(Some(named_path.as_os_str()), &logs, 1).unwrap();
    assert!(!named.in_log_dir);
    capture_session(&named);
    assert_eq!(file_names(&custom).len(), stale as usize + 1);

    let sessions = 3 * LOG_DIR_CAPTURES_KEPT as u64;
    for started in 100..100 + sessions {
        let request =
            PacketCaptureRequest::from_setting(Some(OsStr::new("1")), &logs, started).unwrap();
        capture_session(&request);
    }
    let mut expected: Vec<_> = (100 + sessions - LOG_DIR_CAPTURES_KEPT as u64..100 + sessions)
        .map(|started| format!("packets-{started}.{CAPTURE_EXTENSION}"))
        .chain(others.map(str::to_owned))
        .collect();
    expected.sort();
    assert_eq!(file_names(&logs), expected);
}

/// Captures go oldest first, a start-time collision's suffixed name after the plain one, once
/// either the count or the byte budget would break.
#[test]
fn capture_pruning_keeps_the_newest_within_both_budgets() {
    let dir = tempfile::tempdir().unwrap();
    let names = ["packets-1", "packets-2", "packets-3", "packets-3-9"];
    for name in names {
        std::fs::write(
            dir.path().join(format!("{name}.{CAPTURE_EXTENSION}")),
            [0; 10],
        )
        .unwrap();
    }
    super::capture::prune_captures(dir.path(), 10, 25).unwrap();
    assert_eq!(
        file_names(dir.path()),
        [
            format!("packets-3-9.{CAPTURE_EXTENSION}"),
            format!("packets-3.{CAPTURE_EXTENSION}")
        ]
    );
    super::capture::prune_captures(dir.path(), 1, u64::MAX).unwrap();
    assert_eq!(
        file_names(dir.path()),
        [format!("packets-3-9.{CAPTURE_EXTENSION}")]
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

/// Neither the in-memory ring nor one mapped into a staging file allocates per header.
#[test]
fn recording_a_header_without_capture_does_not_allocate() {
    let staging = tempfile::tempdir().unwrap();
    for attached in [false, true] {
        let log = fresh_log();
        if attached {
            log.attach(staging.path()).unwrap();
        }
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
        assert_eq!(allocations, 0, "attached: {attached}");
        assert_eq!(record, PacketRecordId(1));
        assert_eq!(log.headers().len(), 2);
    }
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
        in_log_dir: false,
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

#[test]
fn capture_byte_budget_drops_before_record_slots_fill_and_recovers_after_drain() {
    let started = Instant::now();
    let (feed, queue, _stop) = channel(8, started);
    let body = Bytes::from(vec![7; super::capture::CAPTURE_QUEUE_BYTES / 2 + 1]);
    for _ in 0..2 {
        feed.packet(
            PacketDirection::Clientbound,
            9,
            started,
            PacketBody::whole(&body),
        );
    }
    assert_eq!(
        queue.take_queued().len(),
        1,
        "the byte budget must drop the second body"
    );
    feed.packet(
        PacketDirection::Clientbound,
        9,
        started,
        PacketBody::whole(&body),
    );
    let records = queue.take_queued();
    assert!(matches!(
        records.as_slice(),
        [
            CaptureRecord::Gap { dropped: 1, .. },
            CaptureRecord::Packet { .. }
        ]
    ));
}

#[test]
fn capture_refuses_a_body_larger_than_its_byte_budget() {
    let started = Instant::now();
    let (feed, queue, _stop) = channel(8, started);
    let body = Bytes::from(vec![7; super::capture::CAPTURE_QUEUE_BYTES + 1]);
    feed.packet(
        PacketDirection::Clientbound,
        9,
        started,
        PacketBody::whole(&body),
    );
    assert!(
        queue.take_queued().is_empty(),
        "an oversized body must never be retained"
    );
    drop(feed);
    let written = queue
        .drain(CaptureWriter::new(Vec::new(), header(1)).unwrap())
        .unwrap();
    let file = read_capture(Bytes::from(written)).unwrap();
    assert!(matches!(
        file.records.as_slice(),
        [CaptureRecord::Gap { dropped: 1, .. }]
    ));
}

#[test]
fn queued_capture_bodies_release_unrelated_batch_storage() {
    let started = Instant::now();
    let (feed, queue, _stop) = channel(8, started);
    let batch = Bytes::from(vec![7; super::capture::CAPTURE_QUEUE_BYTES]);
    let body = batch.slice(..1);
    feed.packet(
        PacketDirection::Clientbound,
        9,
        started,
        PacketBody::whole(&body),
    );
    drop(body);
    assert!(
        batch.is_unique(),
        "queued packets must not retain the batch allocation"
    );
    assert_eq!(queue.take_queued(), [packet(9, 0, &[7])]);
}

#[test]
fn capture_charges_the_body_while_disk_output_is_blocked() {
    use std::sync::atomic::AtomicBool;

    /// Stops the first armed write until the test has tried another capture admission.
    struct GatedWriter {
        armed: Arc<AtomicBool>,
        entered: crossbeam_channel::Sender<()>,
        release: crossbeam_channel::Receiver<()>,
        bytes: Vec<u8>,
    }
    impl Write for GatedWriter {
        /// Stores output, with one deterministic pause after the file header is written.
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.armed.swap(false, Ordering::SeqCst) {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        /// Has no buffered output beyond the test's owned byte vector.
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let started = Instant::now();
    let (feed, queue, stop) = channel(8, started);
    let body = Bytes::from(vec![7; super::capture::CAPTURE_QUEUE_BYTES / 2 + 1]);
    let armed = Arc::new(AtomicBool::new(false));
    let (entered, writing) = crossbeam_channel::bounded(1);
    let (release, blocked) = crossbeam_channel::bounded(1);
    let writer = CaptureWriter::new(
        GatedWriter {
            armed: Arc::clone(&armed),
            entered,
            release: blocked,
            bytes: Vec::new(),
        },
        header(1),
    )
    .unwrap();
    feed.packet(
        PacketDirection::Clientbound,
        9,
        started,
        PacketBody::whole(&body),
    );
    armed.store(true, Ordering::SeqCst);
    let thread = std::thread::spawn(move || queue.drain(writer).unwrap().bytes);
    writing.recv().unwrap();
    feed.packet(
        PacketDirection::Clientbound,
        9,
        started,
        PacketBody::whole(&body),
    );
    stop.send(()).unwrap();
    release.send(()).unwrap();
    let file = read_capture(Bytes::from(thread.join().unwrap())).unwrap();
    assert!(matches!(
        file.records.as_slice(),
        [
            CaptureRecord::Packet { .. },
            CaptureRecord::Gap { dropped: 1, .. }
        ]
    ));
}

/// A closed capture must leave no packet-copying overhead on the network producer.
#[test]
fn an_exited_capture_writer_stops_allocating_packet_bodies() {
    let started = Instant::now();
    let body = Bytes::from(vec![7; 4096]);
    for limited in [false, true] {
        let (feed, queue, _stop) = channel(8, started);
        feed.packet(
            PacketDirection::Clientbound,
            9,
            started,
            PacketBody::whole(&body),
        );
        if limited {
            let writer = CaptureWriter::with_limit(
                Vec::new(),
                header(3),
                CAPTURE_HEADER_BYTES + RECORD_HEADER_BYTES,
            )
            .unwrap();
            queue.drain(writer).unwrap();
        } else {
            drop(queue);
        }
        let (_, allocations) = test_alloc::count(|| {
            feed.packet(
                PacketDirection::Clientbound,
                9,
                started,
                PacketBody::whole(&body),
            );
        });
        assert_eq!(allocations, 0, "the stopped writer must skip body copies");
    }
}

/// A disk error disables copying just like a size-limit or normal writer exit.
#[test]
fn a_failed_capture_write_stops_allocating_packet_bodies() {
    struct HeaderOnly(usize);
    impl std::io::Write for HeaderOnly {
        /// Accepts the file header, then simulates a full disk on the first record.
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.0 {
                return Err(std::io::Error::other("disk full"));
            }
            self.0 -= bytes.len();
            Ok(bytes.len())
        }

        /// The unbuffered fixture has nothing left to flush.
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let started = Instant::now();
    let body = Bytes::from(vec![7; 4096]);
    let (feed, queue, _stop) = channel(8, started);
    feed.packet(
        PacketDirection::Clientbound,
        9,
        started,
        PacketBody::whole(&body),
    );
    let writer = CaptureWriter::new(HeaderOnly(CAPTURE_HEADER_BYTES as usize), header(3)).unwrap();
    assert!(queue.drain(writer).is_err());
    let (_, allocations) = test_alloc::count(|| {
        feed.packet(
            PacketDirection::Clientbound,
            9,
            started,
            PacketBody::whole(&body),
        );
    });
    assert_eq!(allocations, 0, "a failed writer must skip body copies");
}

/// Ingress must return even while opening a sample file would wait for a reader.
#[cfg(unix)]
#[test]
fn rejected_samples_do_not_wait_for_filesystem_consumers() {
    let dir = tempfile::tempdir().unwrap();
    let log = Arc::new(PacketLog::new());
    log.reject(1, 1, 9, PacketOutcome::Failed, b"first");
    log.attach(dir.path()).unwrap();
    let samples = dir.path().join(staging::DECODE_SAMPLES_DIR);
    let fifo = samples.join(
        FailureSample {
            session: 1,
            sequence: 2,
            packet_id: 9,
            outcome: PacketOutcome::Failed,
            len: 6,
            body: Vec::new(),
        }
        .file_name(),
    );
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let (done, finished) = std::sync::mpsc::channel();
    let producer = Arc::clone(&log);
    let worker = std::thread::spawn(move || {
        producer.reject(1, 2, 9, PacketOutcome::Failed, b"second");
        done.send(()).unwrap();
    });
    let completed = finished
        .recv_timeout(std::time::Duration::from_secs(2))
        .is_ok();
    // Opening the read end releases a blocked old writer before asserting the regression.
    let mut reader = std::process::Command::new("cat")
        .arg(&fifo)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    if !completed {
        finished
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
    }
    worker.join().unwrap();
    if reader.try_wait().unwrap().is_none() {
        reader.kill().unwrap();
        reader.wait().unwrap();
    }
    assert!(
        completed,
        "network ingress waited for a sample file consumer"
    );
}

#[test]
fn empty_rejections_still_obey_a_total_sample_bound() {
    let mut samples = FailureSamples::new();
    for id in 10000..10000 + packets::CAPACITY as u32 * 2 {
        samples.offer(1, u64::from(id), id, PacketOutcome::Skipped, b"");
    }
    assert_eq!(samples.samples().len(), packets::CAPACITY);
    assert_eq!(samples.dropped(), packets::CAPACITY as u64);
}

#[test]
fn a_busy_sample_snapshot_never_blocks_ingress_and_counts_the_offer() {
    let log = fresh_log();
    let locked = log.lock_samples();
    log.reject(1, 1, 9, PacketOutcome::Failed, b"sample");
    drop(locked);
    assert_eq!(log.missed_samples.load(Ordering::Relaxed), 1);
}

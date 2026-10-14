use std::ffi::OsStr;
use std::path::Path;
use std::time::{Duration, Instant};

use bytes::Bytes;
use protocol::capture::{
    CAPTURE_HEADER_BYTES, CaptureHeader, CaptureRecord, CaptureWriter, RECORD_HEADER_BYTES,
    read_capture,
};

use super::capture::drain;
use super::samples::FailureSamples;
use super::*;

const START_GAME: u32 = 11;

/// A log of this test's own, apart from the process-wide one.
fn fresh_log() -> Box<PacketLog> {
    Box::new(PacketLog::new())
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
        log.reject(*sequence, 9, outcome, &[]);
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
    log.reject(records[0], 9, PacketOutcome::Failed, &[]);
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
        samples.offer(sequence, 9, PacketOutcome::Failed, &[sequence as u8; 3]);
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

    samples.offer(10, secret_packet_id(), PacketOutcome::Failed, &[1, 2]);
    assert_eq!(samples.withheld(), 1);
    assert_eq!(samples.samples().len(), MAX_SAMPLES_PER_PACKET);

    let room = MAX_SAMPLE_BYTES - samples.bytes();
    samples.offer(11, 20, PacketOutcome::Skipped, &vec![5; room - 10]);
    samples.offer(12, 21, PacketOutcome::Failed, &[6; 100]);
    let last = samples.samples().last().unwrap();
    assert_eq!((last.sequence, last.len, last.body.len()), (12, 100, 10));
    assert_eq!(samples.bytes(), MAX_SAMPLE_BYTES);
    samples.offer(13, 22, PacketOutcome::Failed, &[7]);
    assert_eq!(samples.dropped(), 3);
    assert_eq!(samples.samples().len(), MAX_SAMPLES_PER_PACKET + 2);
}

#[test]
fn dump_lists_headers_by_name_and_summarises_samples() {
    let log = fresh_log();
    let start = log.record(
        PacketDirection::Clientbound,
        START_GAME,
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
    log.reject(start, START_GAME, PacketOutcome::Failed, &[1, 2, 3]);
    let mut out = Vec::new();
    log.dump(&mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines[0], "packet headers: 2 recorded, last 2 kept");
    let start_game = format!("{} ({START_GAME})", PacketName(START_GAME));
    assert!(lines[2].ends_with(&start_game), "{text}");
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
        "rejected packet samples: 1 kept (3 bytes), 0 withheld as credential-bearing, 0 over the bounds"
    );
}

#[test]
fn dump_samples_writes_each_body_and_an_index() {
    let log = fresh_log();
    let start = log.record(
        PacketDirection::Clientbound,
        START_GAME,
        3,
        0,
        PacketOutcome::Accepted,
    );
    log.reject(start, START_GAME, PacketOutcome::Skipped, &[4, 5, 6]);
    let dir = tempfile::tempdir().unwrap();
    let samples = dir.path().join("packets");
    assert_eq!(log.dump_samples(&samples).unwrap(), 1);
    let name = format!("{start}-{}.bin", PacketName(START_GAME));
    assert_eq!(std::fs::read(samples.join(&name)).unwrap(), [4, 5, 6]);
    let index = std::fs::read_to_string(samples.join("index.txt")).unwrap();
    assert!(
        index.starts_with(&name) && index.contains("skipped"),
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
    let (sender, receiver) = crossbeam_channel::bounded(2);
    let started = Instant::now();
    let feed = CaptureFeed::new(sender, started);
    let body = Bytes::from_static(&[1, 2]);
    let at = |micros| started + Duration::from_micros(micros);
    for micros in 0..5 {
        feed.packet(
            PacketDirection::Clientbound,
            9,
            at(micros),
            PacketBody::whole(&body),
        );
    }
    assert_eq!(
        receiver.try_iter().collect::<Vec<_>>(),
        [packet(9, 0, &[1, 2]), packet(9, 1, &[1, 2])]
    );
    feed.packet(
        PacketDirection::Clientbound,
        10,
        at(40),
        PacketBody::whole(&body),
    );
    assert_eq!(
        receiver.try_iter().collect::<Vec<_>>(),
        [
            CaptureRecord::Gap {
                elapsed_us: 40,
                dropped: 3
            },
            packet(10, 40, &[1, 2]),
        ]
    );

    // Packets dropped at the end of a session still reach the capture as a gap.
    for micros in 50..53 {
        feed.packet(
            PacketDirection::Clientbound,
            9,
            at(micros),
            PacketBody::whole(&body),
        );
    }
    assert_eq!(receiver.try_iter().count(), 2);
    drop(feed);
    assert!(matches!(
        receiver.try_iter().collect::<Vec<_>>()[..],
        [CaptureRecord::Gap { dropped: 1, .. }]
    ));
}

#[test]
fn the_writer_stops_at_the_size_limit_with_a_final_marker() {
    let (sender, receiver) = crossbeam_channel::bounded(16);
    let started = Instant::now();
    let feed = CaptureFeed::new(sender, started);
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
    let header = CaptureHeader {
        protocol_version: protocol::PROTOCOL_VERSION,
        started_unix_ms: 3,
    };
    let writer = CaptureWriter::with_limit(Vec::new(), header, limit).unwrap();
    let written = drain(&receiver, writer).unwrap();
    assert_eq!(written.len() as u64, limit);
    let (read_header, records) = read_capture(Bytes::from(written)).unwrap();
    assert_eq!(read_header, header);
    assert_eq!(records.len(), 3);
    assert_eq!(records[2], CaptureRecord::Truncated { elapsed_us: 2 });

    // A feed whose writer has finished neither blocks nor counts drops.
    drop(receiver);
    feed.packet(
        PacketDirection::Serverbound,
        9,
        started,
        PacketBody::whole(&body),
    );
    drop(feed);
}

#[test]
fn recording_a_header_without_capture_does_not_allocate() {
    let log = fresh_log();
    let tap = SessionTap::new(&log, Instant::now(), None);
    let body = Bytes::from_static(&[1, 2, 3, 4]);
    let (record, allocations) = test_alloc::count(|| {
        let record = tap.record(
            PacketDirection::Clientbound,
            START_GAME,
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
    let dir = tempfile::tempdir().unwrap();
    let request = PacketCaptureRequest {
        path: dir.path().join("logs").join("session.cnbrcap"),
        started_unix_ms: 1_760_000_000_000,
    };
    let started = Instant::now();
    let (feed, path, writer) = CaptureFeed::start(&request, started).unwrap();
    assert_eq!(path, request.path);
    let tap = SessionTap::new(&log, started, Some(feed));
    let start_game = Bytes::from_static(&[1, 2, 3]);
    let reply = Bytes::from_static(&[4]);
    let secret = Bytes::from_static(b"content-key");
    let first = tap.record(
        PacketDirection::Clientbound,
        START_GAME,
        PacketBody::whole(&start_game),
    );
    tap.record(PacketDirection::Serverbound, 9, PacketBody::whole(&reply));
    let withheld = tap.record(
        PacketDirection::Clientbound,
        secret_packet_id(),
        PacketBody::whole(&secret),
    );
    tap.reject(
        first,
        START_GAME,
        PacketOutcome::Failed,
        PacketBody::whole(&start_game),
    );
    tap.reject(
        withheld,
        secret_packet_id(),
        PacketOutcome::Failed,
        PacketBody::whole(&secret),
    );
    tap.reject_batch(17);
    drop(tap);
    writer.join().unwrap();

    let (header, records) = read_capture(Bytes::from(std::fs::read(&path).unwrap())).unwrap();
    assert_eq!(
        header,
        CaptureHeader {
            protocol_version: protocol::PROTOCOL_VERSION,
            started_unix_ms: request.started_unix_ms,
        }
    );
    let packets: Vec<_> = records
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
            (PacketDirection::Clientbound, START_GAME, vec![1, 2, 3]),
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
            (START_GAME, 3, PacketOutcome::Failed),
            (9, 1, PacketOutcome::Accepted),
            (secret_packet_id(), 11, PacketOutcome::Failed),
            (UNFRAMED_BATCH_ID, 17, PacketOutcome::Failed),
        ]
    );
    assert_eq!(
        log.samples()
            .iter()
            .map(|sample| (sample.packet_id, sample.body.clone()))
            .collect::<Vec<_>>(),
        [(START_GAME, vec![1, 2, 3])]
    );

    // A second session never replaces the first capture.
    let (feed, second, writer) = CaptureFeed::start(&request, started).unwrap();
    drop(feed);
    writer.join().unwrap();
    assert_eq!(
        second,
        request
            .path
            .with_file_name(format!("session-{}.cnbrcap", request.started_unix_ms))
    );
    assert!(std::fs::read(&path).unwrap().len() as u64 > CAPTURE_HEADER_BYTES);
}

use super::{arguments, fixtures, handoff};
use crate::{
    capture::read_capture,
    options::Options,
    replay::{ReplayResult, Schedule, plan_bursts, replay_bursts},
};
use bytes::Bytes;
use futures::SinkExt;
use protocol::capture::{CaptureHeader, CaptureRecord, CaptureWriter, MAX_CAPTURE_BYTES};
use protocol::packet_tap::PacketDirection;
use std::{io::Cursor, time::Duration};

/// Arrival of each fixture packet, in microseconds from the capture's start.
const ARRIVALS: [u64; 6] = [1_000, 1_000, 1_300, 5_000, 5_100, 30_000];
const STARTED_UNIX_MS: u64 = 1_760_000_000_000;

/// The v1 fixture's packet IDs and bodies, in order.
fn fixture_packets() -> Vec<(u32, Bytes)> {
    let data = fixtures::capture();
    let mut rest = data.as_slice();
    let mut packets = Vec::new();
    while !rest.is_empty() {
        let id = u32::from_le_bytes(rest[..4].try_into().unwrap());
        let length = u32::from_le_bytes(rest[4..8].try_into().unwrap()) as usize;
        packets.push((id, Bytes::copy_from_slice(&rest[8..8 + length])));
        rest = &rest[8 + length..];
    }
    packets
}

/// The fixture as a client records it in v2: each packet at its [`ARRIVALS`] time, with a
/// serverbound reply and a gap of three dropped packets after the first, within `limit` bytes.
fn v2_fixture(protocol_version: i32, limit: u64) -> Vec<u8> {
    let header = CaptureHeader {
        protocol_version,
        started_unix_ms: STARTED_UNIX_MS,
    };
    let mut writer = CaptureWriter::with_limit(Vec::new(), header, limit).unwrap();
    for (index, ((packet_id, body), elapsed_us)) in
        fixture_packets().into_iter().zip(ARRIVALS).enumerate()
    {
        writer
            .write(&CaptureRecord::Packet {
                direction: PacketDirection::Clientbound,
                packet_id,
                elapsed_us,
                body,
            })
            .unwrap();
        if index == 0 {
            writer
                .write(&CaptureRecord::Packet {
                    direction: PacketDirection::Serverbound,
                    packet_id: 0x45,
                    elapsed_us: elapsed_us + 1,
                    body: Bytes::from_static(&[1]),
                })
                .unwrap();
            writer
                .write(&CaptureRecord::Gap {
                    elapsed_us: elapsed_us + 2,
                    dropped: 3,
                })
                .unwrap();
        }
    }
    writer.into_inner()
}

fn read(data: Vec<u8>) -> anyhow::Result<crate::capture::Capture> {
    read_capture(Cursor::new(data))
}

#[test]
fn v2_captures_replay_the_clientbound_packets_a_v1_capture_holds() {
    let v1 = read(fixtures::capture()).unwrap();
    let v2 = read(v2_fixture(protocol::PROTOCOL_VERSION, MAX_CAPTURE_BYTES)).unwrap();
    assert_eq!(v2.summary.replay_sha256, v1.summary.replay_sha256);
    assert_eq!(v2.summary.packet_counts, v1.summary.packet_counts);
    assert_eq!(
        (
            v1.summary.format,
            v2.summary.format,
            v2.summary.records,
            v2.summary.replay_packets,
            v2.summary.serverbound_records_ignored,
            v2.summary.dropped_packets,
            v2.summary.truncated,
            v2.summary.capture_started_unix_ms,
        ),
        (1, 2, 8, 6, 1, 3, false, Some(STARTED_UNIX_MS))
    );
    assert_eq!(
        v2.packets
            .iter()
            .map(|packet| (packet.record, packet.elapsed))
            .collect::<Vec<_>>(),
        [0, 3, 4, 5, 6, 7]
            .into_iter()
            .zip(ARRIVALS.map(|us| Some(Duration::from_micros(us))))
            .collect::<Vec<_>>()
    );
    assert!(v1.packets.iter().all(|packet| packet.elapsed.is_none()));
}

#[test]
fn a_capture_cut_by_its_size_limit_replays_what_it_kept() {
    let full = v2_fixture(protocol::PROTOCOL_VERSION, MAX_CAPTURE_BYTES);
    let cut = read(v2_fixture(
        protocol::PROTOCOL_VERSION,
        full.len() as u64 - 1,
    ))
    .unwrap();
    assert!(cut.summary.truncated);
    assert_eq!(cut.summary.replay_packets, 5);
}

/// A client that crashed mid-write leaves a capture ending inside a record.
#[test]
fn a_capture_ending_inside_a_record_replays_its_complete_records() {
    let full = read(v2_fixture(protocol::PROTOCOL_VERSION, MAX_CAPTURE_BYTES)).unwrap();
    assert!(!full.summary.truncated);
    let mut torn = v2_fixture(protocol::PROTOCOL_VERSION, MAX_CAPTURE_BYTES);
    torn.pop();
    let torn = read(torn).unwrap();
    assert!(torn.summary.truncated);
    assert_eq!(torn.summary.replay_packets, full.summary.replay_packets - 1);
    assert_eq!(
        torn.packets
            .iter()
            .map(|packet| packet.wire.clone())
            .collect::<Vec<_>>(),
        full.packets[..full.packets.len() - 1]
            .iter()
            .map(|packet| packet.wire.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn v2_captures_from_another_protocol_are_rejected() {
    assert!(
        read(v2_fixture(
            protocol::PROTOCOL_VERSION + 1,
            MAX_CAPTURE_BYTES
        ))
        .is_err()
    );
}

/// Terrain requiring blobs outside the capture must fail before a replay endpoint is opened.
#[test]
fn cached_terrain_captures_are_rejected_explicitly() {
    let mut accepted_cached = Vec::new();
    for cached in [true, false] {
        for packet in protocol::test_support::packets::terrain_cache_packets(cached) {
            let batch = protocol::session_wire::batch_frame_from_bedrock(
                &protocol::encode(&packet).unwrap(),
            )
            .unwrap();
            let protocol::session_wire::CoreMessage::Batch(mut packets) =
                protocol::session_wire::decode_core_message(batch).unwrap()
            else {
                panic!("terrain batch");
            };
            let wire = packets.pop().unwrap();
            // The ID's canonical header length comes from the protocol encoder.
            let id = packet.kind().id();
            let header_len = protocol::session_wire::packet_from_body(id, &[])
                .unwrap()
                .len();
            let body = wire.slice(header_len..);
            for v2 in [false, true] {
                let mut data = fixtures::capture();
                if v2 {
                    let mut writer = CaptureWriter::new(
                        Vec::new(),
                        CaptureHeader {
                            protocol_version: protocol::PROTOCOL_VERSION,
                            started_unix_ms: STARTED_UNIX_MS,
                        },
                    )
                    .unwrap();
                    for (packet_id, body) in
                        fixture_packets().into_iter().chain([(id, body.clone())])
                    {
                        writer
                            .write(&CaptureRecord::Packet {
                                direction: PacketDirection::Clientbound,
                                packet_id,
                                elapsed_us: 0,
                                body,
                            })
                            .unwrap();
                    }
                    data = writer.into_inner();
                } else {
                    data.extend(fixtures::record(id, &body));
                }
                let result = read(data);
                if cached {
                    match result {
                        Err(error) => {
                            assert!(format!("{error:#}").contains("blob-cache"), "{error:#}")
                        }
                        Ok(_) => accepted_cached.push((id, if v2 { 2 } else { 1 })),
                    }
                } else {
                    assert!(result.is_ok(), "uncached terrain: {:?}", result.err());
                }
            }
        }
    }
    assert!(
        accepted_cached.is_empty(),
        "cached terrain must not silently lose its blobs: {accepted_cached:?}"
    );
}

#[test]
fn original_timing_needs_recorded_arrival_times() {
    let v1 = read(fixtures::capture()).unwrap();
    assert!(plan_bursts(&v1.packets, 32, 1 << 20, Schedule::Original).is_err());
}

#[tokio::test(start_paused = true)]
async fn original_timing_sends_each_burst_at_its_recorded_arrival() {
    let captured = read(v2_fixture(protocol::PROTOCOL_VERSION, MAX_CAPTURE_BYTES)).unwrap();
    let bursts = plan_bursts(&captured.packets, 32, 1 << 20, Schedule::Original).unwrap();
    // StartGame arrived at 1 ms; packets within 1 ms of a burst's first packet share it.
    assert_eq!(
        bursts
            .iter()
            .map(|burst| (burst.packets.clone(), burst.due))
            .collect::<Vec<_>>(),
        [
            (0..1, Duration::ZERO),
            (1..3, Duration::ZERO),
            (3..5, Duration::from_millis(4)),
            (5..6, Duration::from_millis(29)),
        ]
    );
    let mut sink = futures::sink::drain::<Bytes>().sink_map_err(std::io::Error::other);
    let mut result = ReplayResult::default();
    replay_bursts(
        &mut sink,
        &captured.packets,
        &bursts,
        handoff(),
        &[],
        &mut result,
    )
    .await
    .unwrap();
    assert_eq!(result.sha256, captured.summary.replay_sha256);
    let samples = result.bursts.unwrap();
    assert!(
        samples
            .iter()
            .zip(&bursts)
            .all(|(sample, burst)| sample.flushed_ms == sample.due_ms
                && sample.due_ms == burst.due.as_secs_f64() * 1000.)
    );

    // The burst limits still split packets that arrived together.
    let single = plan_bursts(&captured.packets, 1, 1 << 20, Schedule::Original).unwrap();
    assert_eq!(
        single.iter().map(|burst| burst.due).collect::<Vec<_>>(),
        [0, 0, 300, 4_000, 4_100, 29_000].map(Duration::from_micros)
    );
}

#[test]
fn original_timing_is_a_boolean_flag() {
    let parse = |extra: &[&str]| {
        Options::parse(
            arguments()
                .into_iter()
                .chain(extra.iter().map(|flag| (*flag).to_owned())),
        )
    };
    assert!(!parse(&[]).unwrap().unwrap().original_timing);
    let opts = parse(&["-original-timing", "-hold", "1s"])
        .unwrap()
        .unwrap();
    assert!(opts.original_timing);
    assert_eq!(opts.hold, Duration::from_secs(1));
    assert!(
        !parse(&["--original-timing=false"])
            .unwrap()
            .unwrap()
            .original_timing
    );
    assert!(parse(&["-original-timing=maybe"]).is_err());
}

mod fixtures;
mod session;
use super::{
    capture::read_capture,
    options::Options,
    replay::{ReplayResult, plan_bursts, replay_bursts},
};
use bytes::Bytes;
use fixtures::record;
use protocol::session_wire::{SessionHandoff, SessionIdentity};
use std::{io::Cursor, time::Duration};

/// Supplies only the required paths so every optional flag retains its compatible default.
fn arguments() -> Vec<String> {
    [
        "-capture",
        "fixture.bin",
        "-socket-dir",
        "local",
        "-report",
        "result.json",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Builds an empty metadata handoff; replay fills its startup from the capture.
fn handoff() -> SessionHandoff {
    SessionHandoff {
        identity: SessionIdentity {
            display_name: "Fixture".into(),
            xuid: String::new(),
            uuid: String::new(),
        },
        client_cache: false,
        packs_required: false,
        packs: Vec::new(),
        startup: Vec::new(),
    }
}

#[test]
fn options_reject_invalid_runs_and_keep_go_flag_forms() {
    for extra in [
        ["-upstream", "example.test:19132"],
        ["-burst-packets", "0"],
        ["-burst-bytes", "0"],
        ["-burst-interval", "-1s"],
        ["-timeout", "0s"],
        ["-hold", "-1s"],
    ] {
        assert!(Options::parse(arguments().into_iter().chain(extra.map(str::to_owned))).is_err());
    }
    assert!(Options::parse(arguments().into_iter().chain(["unexpected".into()])).is_err());
    let opts = Options::parse(arguments()).unwrap().unwrap();
    assert_eq!(
        (
            opts.burst_packets,
            opts.burst_bytes,
            opts.interval,
            opts.timeout,
            opts.hold
        ),
        (
            32,
            super::options::MAX_BURST_BYTES,
            Duration::from_millis(50),
            Duration::from_secs(120),
            Duration::ZERO
        )
    );
    let opts = Options::parse(
        arguments()
            .into_iter()
            .chain(["--hold=1m2.5s".into(), "-burst-interval=1us".into()]),
    )
    .unwrap()
    .unwrap();
    assert_eq!(opts.hold, Duration::from_millis(62500));
    assert_eq!(opts.interval, Duration::from_micros(1));
    assert!(Options::parse(["-h".into()]).unwrap().is_none());
}

#[test]
fn capture_preserves_opaque_bodies_and_post_login_metadata() {
    let data: Vec<_> = [
        (143, &[1][..]),
        (7, &[2]),
        (10, &[3]),
        (11, &[4]),
        (6, &[5]),
        (0x3fe, &[0, 128, 255, 9]),
    ]
    .into_iter()
    .flat_map(|(id, body)| record(id, body))
    .collect();
    let captured = read_capture(Cursor::new(data)).unwrap();
    assert_eq!(
        (
            captured.summary.records,
            captured.summary.handshake_records_regenerated,
            captured.summary.replay_packets
        ),
        (6, 2, 4)
    );
    let expected = [
        vec![10, 3],
        vec![11, 4],
        vec![6, 5],
        vec![0xfe, 7, 0, 128, 255, 9],
    ];
    assert_eq!(
        captured
            .packets
            .iter()
            .map(|packet| packet.wire.as_ref())
            .collect::<Vec<_>>(),
        expected.iter().map(Vec::as_slice).collect::<Vec<_>>()
    );
    let bursts = plan_bursts(&captured.packets, 2, 8).unwrap();
    assert_eq!(
        bursts
            .iter()
            .map(|burst| burst.packets.clone())
            .collect::<Vec<_>>(),
        [0..2, 2..4]
    );
}

#[test]
fn capture_rejects_truncation_transfers_and_duplicate_start() {
    let valid = record(11, &[10, 20, 30]);
    for cut in 0..valid.len() {
        assert!(read_capture(Cursor::new(&valid[..cut])).is_err());
    }
    for suffix in [
        vec![1],
        record(85, &[]),
        record(11, &[]),
        record(0x400, &[]),
    ] {
        assert!(read_capture(Cursor::new([valid.clone(), suffix].concat())).is_err());
    }
}

#[tokio::test(start_paused = true)]
async fn pacing_preserves_witness_and_source_boundaries() {
    let captured = read_capture(Cursor::new(fixtures::capture())).unwrap();
    let bursts = plan_bursts(&captured.packets, 2, 1 << 20).unwrap();
    let mut sink = futures::sink::drain::<Bytes>().sink_map_err(std::io::Error::other);
    let mut result = ReplayResult::default();
    replay_bursts(
        &mut sink,
        &captured.packets,
        &bursts,
        Duration::from_millis(50),
        handoff(),
        &[],
        &mut result,
    )
    .await
    .unwrap();
    assert_eq!(result.sha256, captured.summary.replay_sha256);
    assert_eq!(result.packets, captured.packets.len());
    let samples = result.bursts.unwrap();
    assert_eq!(
        samples
            .iter()
            .map(|sample| sample.due_ms)
            .collect::<Vec<_>>(),
        [0., 50., 100., 150.]
    );
    assert!(
        samples
            .iter()
            .all(|sample| sample.flushed_ms == sample.due_ms)
    );
    assert_eq!(
        (
            samples.first().unwrap().first_record,
            samples.last().unwrap().last_record
        ),
        (0, 5)
    );
}

#[tokio::test]
async fn invalid_bounds_and_failed_writes_never_report_delivery() {
    use futures::SinkExt;
    let captured = read_capture(Cursor::new(fixtures::capture())).unwrap();
    for bounds in [(0, 10), (1, 0), (1, 1)] {
        assert!(plan_bursts(&captured.packets, bounds.0, bounds.1).is_err());
    }
    let bursts = plan_bursts(&captured.packets, 1, 1 << 20).unwrap();
    let mut sink = Box::pin(
        futures::sink::drain::<Bytes>()
            .sink_map_err(std::io::Error::other)
            .with(|_: Bytes| async { Err::<Bytes, _>(std::io::Error::other("closed")) }),
    );
    for interval in [Duration::ZERO, Duration::from_nanos(i64::MAX as u64)] {
        let mut result = ReplayResult::default();
        assert!(
            replay_bursts(
                &mut sink,
                &captured.packets,
                &bursts,
                interval,
                handoff(),
                &[],
                &mut result
            )
            .await
            .is_err()
        );
        assert_eq!(result.packets, 0);
        assert!(result.sha256.is_empty());
    }
}

use futures::SinkExt;

#[tokio::test(start_paused = true)]
async fn backpressure_catches_up_to_the_original_schedule() {
    let captured = read_capture(Cursor::new(fixtures::capture())).unwrap();
    let bursts = plan_bursts(&captured.packets, 2, 1 << 20).unwrap();
    let (waiting, blocked) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let mut first = Some((waiting, released));
    let mut sink = Box::pin(
        futures::sink::drain::<Bytes>()
            .sink_map_err(std::io::Error::other)
            .with(move |frame| {
                let first = first.take();
                async move {
                    if let Some((waiting, released)) = first {
                        waiting.send(()).unwrap();
                        released.await.unwrap();
                    }
                    Ok::<_, std::io::Error>(frame)
                }
            }),
    );
    let mut result = ReplayResult::default();
    let control = async {
        blocked.await.unwrap();
        tokio::time::advance(Duration::from_millis(125)).await;
        release.send(()).unwrap();
    };
    let (replayed, ()) = tokio::join!(
        replay_bursts(
            &mut sink,
            &captured.packets,
            &bursts,
            Duration::from_millis(50),
            handoff(),
            &[],
            &mut result
        ),
        control
    );
    replayed.unwrap();
    let samples = result.bursts.unwrap();
    assert_eq!(
        samples
            .iter()
            .map(|sample| sample.flushed_ms)
            .collect::<Vec<_>>(),
        [125., 125., 125., 150.]
    );
}

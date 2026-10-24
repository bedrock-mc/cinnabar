//! Terminating a startup must not wait for obsolete presentation preparation.

use super::*;
use bridge::FrameRead;
use futures::{FutureExt, Stream};
use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
};

/// Delivers ready core frames, then keeps the connection open without producing more input.
struct ReadyFrames(VecDeque<Bytes>);

impl Stream for ReadyFrames {
    type Item = Result<Bytes, BridgeError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0
            .pop_front()
            .map_or(Poll::Pending, |frame| Poll::Ready(Some(Ok(frame))))
    }
}

impl FrameRead for ReadyFrames {
    fn trace_conn(&self) -> u64 {
        0
    }

    fn set_trace_conn(&self, _: u64) {}
}

/// Builds ordered input with an actually closed writer and no socket task.
fn transport(frames: Vec<Bytes>) -> SessionTransport<ReadyFrames> {
    let (client, _peer) = tokio::io::duplex(64);
    let (read, write) = tokio::io::split(client);
    let (_, queue, _writer) =
        bridge::tasks::prepare_bounded_connection(read, write, bridge::MAX_FRAME_LEN, 64);
    SessionTransport::with_timers(
        ReadyFrames(frames.into()),
        queue,
        &[],
        bridge::tasks::Timers::tokio(),
    )
}

/// Encodes the core's typed terminal message, following ordinary play frames.
fn terminal(kind: u8, body: serde_json::Value) -> Bytes {
    let mut frame = vec![kind];
    frame.extend(serde_json::to_vec(&body).unwrap());
    frame.into()
}

/// Returns an ordinary packet whose order is observable after preparation completes.
fn time(value: i32) -> Bytes {
    batch_frame(&[Packet::new(SetTime {
        time: wire::ZigZag32(value),
    })])
}

#[tokio::test]
async fn transfer_during_held_preparation_ends_startup_before_release() {
    let mut transport = transport(vec![
        time(1),
        terminal(
            bridge::contract::KIND_TRANSFER,
            serde_json::json!({"address":"next.example.test","port":19133,"reload_world":true}),
        ),
        time(2),
    ]);
    let (release, held) = tokio::sync::oneshot::channel::<()>();
    let mut waiting = Box::pin(prepare_with_read_ahead(&mut transport, async {
        held.await.unwrap();
    }));
    let early = waiting.as_mut().now_or_never();
    let preparation_abandoned = release.is_closed();
    drop(waiting);
    assert!(preparation_abandoned, "obsolete preparation is abandoned");
    let error = early
        .expect("the transfer must finish startup while preparation is held")
        .expect_err("a transferred startup must not succeed");
    let target = error.server_transfer().expect("the validated transfer");
    assert_eq!(target.host, "next.example.test");
    assert_eq!(target.port, 19133);
    assert!(target.reload_world);
}

#[tokio::test]
async fn disconnect_during_held_preparation_retains_its_display_policy() {
    for hide_screen in [false, true] {
        let mut transport = transport(vec![terminal(
            bridge::contract::KIND_DISCONNECT,
            serde_json::json!({"reason":25,"message":"full","filtered_message":"","hide_screen":hide_screen}),
        )]);
        let (_release, held) = tokio::sync::oneshot::channel::<()>();
        let error = prepare_with_read_ahead(&mut transport, async { held.await.unwrap() })
            .now_or_never()
            .expect("disconnect must not wait for preparation")
            .expect_err("a disconnected startup must not succeed");
        let disconnect = error.server_disconnect().expect("the server disconnect");
        assert_eq!(disconnect.reason, "Serverfull");
        assert_eq!(
            disconnect.message.as_deref(),
            (!hide_screen).then_some("full")
        );
    }
}

#[tokio::test]
async fn ordinary_backlog_still_waits_for_preparation_and_keeps_fifo_order() {
    let mut transport = transport(vec![time(1), time(2)]);
    let (release, held) = tokio::sync::oneshot::channel();
    let mut waiting = Box::pin(prepare_with_read_ahead(&mut transport, async {
        held.await.unwrap()
    }));
    assert!(waiting.as_mut().now_or_never().is_none());
    release.send(7).unwrap();
    assert_eq!(waiting.as_mut().now_or_never().unwrap().unwrap(), 7);
    drop(waiting);
    for expected in [1, 2] {
        let crate::session_transport::Inbound::Packet(raw) = transport.recv().await.unwrap() else {
            panic!("the original packet order is retained")
        };
        let packets::Packet::SetTime(time) = *raw.decode().unwrap().data else {
            panic!("only the written packets arrive")
        };
        assert_eq!(time.time.0, expected);
    }
}

#[tokio::test]
async fn requested_latency_during_held_preparation_replies_before_release_and_survives_cancellation()
 {
    let requested = Packet::new(packets::NetworkStackLatency {
        creation_time: wire::U64LE(777),
        is_from_server: true,
    });
    let unrequested = Packet::new(packets::NetworkStackLatency {
        creation_time: wire::U64LE(888),
        is_from_server: false,
    });
    let (client, peer) = tokio::io::duplex(8192);
    let (read, write) = tokio::io::split(client);
    let (_, queue, writer) =
        bridge::tasks::prepare_bounded_connection(read, write, bridge::MAX_FRAME_LEN, 8192);
    let mut transport = SessionTransport::with_timers(
        ReadyFrames(
            vec![
                time(1),
                batch_frame(&[requested, unrequested.clone()]),
                time(2),
            ]
            .into(),
        ),
        queue,
        &[],
        bridge::tasks::Timers::tokio(),
    );
    let mut peer = FakeCore { stream: peer };
    let mut writer = Box::pin(writer);
    let (release, held) = tokio::sync::oneshot::channel();
    let mut preparation = Box::pin(async { held.await.unwrap() });
    let mut waiting = Box::pin(prepare_with_read_ahead(
        &mut transport,
        preparation.as_mut(),
    ));
    assert!(waiting.as_mut().now_or_never().is_none());
    assert!(writer.as_mut().now_or_never().is_none());
    assert!(waiting.as_mut().now_or_never().is_none());
    let before_release = peer.receive_packets().now_or_never();
    let preparation_still_held = !release.is_closed();
    drop(waiting);

    let mut resumed = Box::pin(prepare_with_read_ahead(
        &mut transport,
        preparation.as_mut(),
    ));
    assert!(resumed.as_mut().now_or_never().is_none());
    assert!(writer.as_mut().now_or_never().is_none());
    assert!(resumed.as_mut().now_or_never().is_none());
    let duplicate_after_resume = peer.receive_packets().now_or_never();
    release.send(7).unwrap();
    let completed = resumed.as_mut().now_or_never();
    drop(resumed);
    let mut deferred = Vec::new();
    for _ in 0..3 {
        let incoming = transport.recv().now_or_never();
        deferred.push(incoming.map(|result| match result.unwrap() {
            crate::session_transport::Inbound::Packet(raw) => raw.decode().unwrap(),
            _ => panic!("only the original ordinary packets are retained"),
        }));
    }
    drop(transport);
    drop(peer);
    assert!(writer.as_mut().now_or_never().is_some());

    assert!(preparation_still_held);
    assert_eq!(
        before_release,
        Some(vec![crate::network_stack_latency_reply(777)]),
        "the requested probe must be answered while presentation remains held"
    );
    assert!(
        duplicate_after_resume.is_none(),
        "a canceled wait must not repeat a reply"
    );
    assert_eq!(completed.unwrap().unwrap(), 7);
    assert_eq!(
        deferred,
        vec![
            Some(Packet::new(SetTime {
                time: wire::ZigZag32(1)
            })),
            Some(unrequested),
            Some(Packet::new(SetTime {
                time: wire::ZigZag32(2)
            })),
        ],
        "the requested probe is consumed once and unrelated siblings retain their order"
    );
}

#[tokio::test]
async fn buffered_transfer_supersedes_a_probe_when_the_writer_is_backpressured() {
    let budget = 64;
    let (client, peer) = tokio::io::duplex(budget);
    let (read, write) = tokio::io::split(client);
    let (_, queue, writer) =
        bridge::tasks::prepare_bounded_connection(read, write, bridge::MAX_FRAME_LEN, budget);
    let mut occupied = Box::pin(
        queue
            .accept(Bytes::from(vec![0; budget]))
            .now_or_never()
            .expect("the original bounded queue admits its first frame")
            .unwrap(),
    );
    assert!(occupied.as_mut().now_or_never().is_none());
    assert!(
        queue
            .accept(Bytes::from_static(&[1]))
            .now_or_never()
            .is_none(),
        "the undrained original frame occupies the whole writer budget"
    );
    let accepted = vec![
        Packet::new(SetTime {
            time: wire::ZigZag32(1),
        }),
        Packet::new(packets::NetworkStackLatency {
            creation_time: wire::U64LE(777),
            is_from_server: true,
        }),
        Packet::new(packets::Transfer {
            server_address: "next.example.test".into(),
            server_port: wire::U16LE(19133),
            reload_world: true,
            ..Default::default()
        }),
        Packet::new(SetTime {
            time: wire::ZigZag32(2),
        }),
    ];
    let mut transport = SessionTransport::with_timers(
        ReadyFrames(vec![batch_frame(&accepted)].into()),
        queue,
        &[],
        bridge::tasks::Timers::tokio(),
    );
    let (release, held) = tokio::sync::oneshot::channel::<()>();
    let mut waiting = Box::pin(prepare_with_read_ahead(&mut transport, async {
        held.await.unwrap();
    }));
    let early = waiting.as_mut().now_or_never();
    let preparation_abandoned = release.is_closed();
    assert!(occupied.as_mut().now_or_never().is_none());
    drop(waiting);
    let mut retained = Vec::new();
    for _ in &accepted {
        retained.push(transport.recv().now_or_never().map(|result| {
            let crate::session_transport::Inbound::Packet(raw) = result.unwrap() else {
                panic!("the original terminal packet must retain its FIFO position")
            };
            raw.decode().unwrap()
        }));
    }
    drop(transport);
    drop(writer);
    drop(peer);
    assert!(occupied.as_mut().now_or_never().unwrap().is_err());

    let error = early
        .expect("the buffered terminal must finish without draining the blocked writer")
        .expect_err("the original transfer must end startup");
    let target = error.server_transfer().expect("the validated transfer");
    assert_eq!(target.host, "next.example.test");
    assert_eq!(target.port, 19133);
    assert!(target.reload_world);
    assert!(preparation_abandoned, "obsolete preparation is abandoned");
    assert_eq!(retained, accepted.into_iter().map(Some).collect::<Vec<_>>());
}

#[tokio::test]
async fn failed_probe_write_preserves_the_next_readable_transfer() {
    let requested = Packet::new(packets::NetworkStackLatency {
        creation_time: wire::U64LE(777),
        is_from_server: true,
    });
    let mut transport = transport(vec![
        batch_frame(std::slice::from_ref(&requested)),
        terminal(
            bridge::contract::KIND_TRANSFER,
            serde_json::json!({"address":"next.example.test","port":19133,"reload_world":true}),
        ),
    ]);
    assert!(
        transport
            .send(&[crate::network_stack_latency_reply(0)])
            .now_or_never()
            .expect("the original closed writer fails without input progress")
            .is_err()
    );
    let (release, held) = tokio::sync::oneshot::channel::<()>();
    let mut waiting = Box::pin(prepare_with_read_ahead(&mut transport, async {
        held.await.unwrap();
    }));
    let result = waiting.as_mut().now_or_never();
    let preparation_abandoned = release.is_closed();
    drop(waiting);
    drop(transport);

    assert!(preparation_abandoned, "obsolete preparation is abandoned");
    let error = result
        .expect("the ready terminal must finish startup after the write fails")
        .expect_err("the server transfer must end startup");
    let target = error
        .server_transfer()
        .expect("the separately readable Transfer must preserve its original cause");
    assert_eq!(target.host, "next.example.test");
    assert_eq!(target.port, 19133);
    assert!(target.reload_world);
}

#[tokio::test]
async fn unusable_buffered_transfer_does_not_bypass_preparation() {
    let mut transport = transport(vec![terminal(
        bridge::contract::KIND_TRANSFER,
        serde_json::json!({"address":" ","port":19133,"reload_world":false}),
    )]);
    let (release, held) = tokio::sync::oneshot::channel();
    let mut waiting = Box::pin(prepare_with_read_ahead(&mut transport, async {
        held.await.unwrap()
    }));
    assert!(waiting.as_mut().now_or_never().is_none());
    release.send(7).unwrap();
    assert_eq!(waiting.as_mut().now_or_never().unwrap().unwrap(), 7);
    drop(waiting);
    assert!(matches!(
        transport.recv().await.unwrap(),
        crate::session_transport::Inbound::Terminal(crate::session_transport::Terminal::Transfer(
            _
        ))
    ));
}

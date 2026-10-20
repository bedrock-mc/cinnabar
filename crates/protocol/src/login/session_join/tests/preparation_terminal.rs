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

/// Builds the real ordered transport without a socket task; these waits never send packets.
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

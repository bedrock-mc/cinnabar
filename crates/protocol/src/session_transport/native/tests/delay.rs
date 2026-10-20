//! New-feature controls on the original native pump and read-ahead FIFO, without a cipher model.
use super::*;
use crate::session_transport::{Inbound, MAX_READ_AHEAD_BYTES};
use bedrock_protocol_1_26_51::{packets, wire};
use std::task::Poll;

/// The existing packet-connection boundary with independently observed original input and writes.
struct DuplexConnection {
    incoming: mpsc::Receiver<Bytes>,
    outgoing: mpsc::Sender<Bytes>,
}

impl PacketConnection for DuplexConnection {
    async fn send_body(&mut self, body: Bytes) -> io::Result<()> {
        self.outgoing.send(body).await.map_err(io::Error::other)
    }

    async fn recv_body(&mut self) -> io::Result<Bytes> {
        self.incoming
            .recv()
            .await
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "fixture input ended"))
    }

    async fn close(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Supplies the original bounded send/receive owners, without another payload queue or task.
fn pair() -> (SessionTransport, mpsc::Sender<Bytes>, mpsc::Receiver<Bytes>) {
    let (incoming, receive) = mpsc::channel(QUEUED_BATCHES);
    let (send, outgoing) = mpsc::channel(QUEUED_BATCHES);
    let transport = SessionTransport::from_native(DuplexConnection {
        incoming: receive,
        outgoing: send,
    });
    (transport, incoming, outgoing)
}

/// Uses the original codec's body, shared by this owning boundary's incoming and outgoing fixture.
fn body(index: usize) -> Bytes {
    crate::session_transport::batch_body(&[crate::Packet::new(packets::SetTime {
        time: wire::ZigZag32(index.try_into().unwrap()),
    })])
    .unwrap()
}

/// Reads the original accepted raw entry through its existing generated decoder.
fn time(inbound: Inbound) -> i32 {
    let Inbound::Packet(raw) = inbound else {
        panic!("unexpected fixture terminal")
    };
    let packets::Packet::SetTime(packet) = *raw.decode().unwrap().data else {
        panic!("unexpected fixture packet")
    };
    packet.time.0
}

#[tokio::test(start_paused = true)]
async fn native_delay_preserves_original_bidirectional_batch_pipeline() {
    tokio::time::timeout(Duration::from_secs(2), async {
        let (mut transport, incoming, mut outgoing) = pair();
        let delay = transport.packet_delay().unwrap();
        // This is a pump-boundary control, not a fabricated authenticated SpawnReady.
        delay.bind(42);
        delay.configure(200, false).unwrap();
        let start = tokio::time::Instant::now();
        let sender = transport.sender();
        let mut receipts = Vec::new();
        let expected: Vec<_> = (0..QUEUED_BATCHES).map(body).collect();
        for body in &expected {
            incoming.send(body.clone()).await.unwrap();
            receipts.push(sender.accept(body.clone()).await.unwrap());
        }
        for _ in 0..QUEUED_BATCHES {
            std::future::poll_fn(|cx| transport.poll_read_ahead_one(MAX_READ_AHEAD_BYTES, cx))
                .await;
        }
        tokio::task::yield_now().await;
        let mut received = Vec::new();
        let early_incoming = {
            let mut receive = std::pin::pin!(transport.recv());
            match std::future::poll_fn(|cx| Poll::Ready(receive.as_mut().poll(cx))).await {
                Poll::Ready(result) => {
                    received.push(time(result.unwrap()));
                    true
                }
                Poll::Pending => false,
            }
        };
        let first_outgoing = outgoing.try_recv().ok();
        let early_outgoing = first_outgoing.is_some();
        let mut sent: Vec<_> = first_outgoing.into_iter().collect();
        tokio::time::advance(Duration::from_millis(200)).await;
        while received.len() < QUEUED_BATCHES {
            received.push(time(transport.recv().await.unwrap()));
        }
        while sent.len() < QUEUED_BATCHES {
            sent.push(outgoing.recv().await.unwrap());
        }
        for receipt in receipts {
            receipt.await.unwrap();
        }
        let elapsed = start.elapsed();
        transport.close().await.unwrap();
        assert!(
            !early_outgoing,
            "native post-login batches bypassed the configured outgoing delay"
        );
        assert!(
            !early_incoming,
            "native post-login batches bypassed the configured incoming delay"
        );
        assert_eq!(
            sent, expected,
            "original admitted bodies changed or reordered"
        );
        assert_eq!(
            received,
            (0..QUEUED_BATCHES)
                .map(|index| i32::try_from(index).unwrap())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            elapsed,
            Duration::from_millis(200),
            "each original admission keeps its own deadline, without serial delays"
        );
    })
    .await
    .expect("original delayed pump failed to finish under its finite fixture budget");
}

/// Retained admissions keep their first deadline through borrowed cancellation and new policy.
#[tokio::test(start_paused = true)]
async fn native_delay_keeps_original_deadlines_and_releases_old_backlog() {
    tokio::time::timeout(Duration::from_secs(2), async {
        let (mut transport, incoming, mut outgoing) = pair();
        let delay = transport.packet_delay().unwrap();
        delay.bind(42);
        delay.configure(200, false).unwrap();
        let sender = transport.sender();
        let start = tokio::time::Instant::now();
        incoming.send(body(0)).await.unwrap();
        let first = sender.accept(body(0)).await.unwrap();
        std::future::poll_fn(|cx| transport.poll_read_ahead_one(MAX_READ_AHEAD_BYTES, cx)).await;
        let first_pending = {
            let mut receive = std::pin::pin!(transport.recv());
            std::future::poll_fn(|cx| Poll::Ready(receive.as_mut().poll(cx)))
                .await
                .is_pending()
        };
        tokio::time::advance(Duration::from_millis(100)).await;
        delay.configure(400, false).unwrap();
        incoming.send(body(1)).await.unwrap();
        let second = sender.accept(body(1)).await.unwrap();
        std::future::poll_fn(|cx| transport.poll_read_ahead_one(MAX_READ_AHEAD_BYTES, cx)).await;
        tokio::time::advance(Duration::from_millis(100)).await;
        let received_first = time(transport.recv().await.unwrap());
        let written_first = outgoing.recv().await.unwrap();
        first.await.unwrap();
        let second_pending = {
            let mut receive = std::pin::pin!(transport.recv());
            std::future::poll_fn(|cx| Poll::Ready(receive.as_mut().poll(cx)))
                .await
                .is_pending()
        };
        let second_not_written = outgoing.try_recv().is_err();
        delay.configure(0, false).unwrap();
        delay.configure(400, false).unwrap();
        let received_second = time(transport.recv().await.unwrap());
        let written_second = outgoing.recv().await.unwrap();
        second.await.unwrap();
        let elapsed = start.elapsed();
        transport.close().await.unwrap();
        assert!(
            first_pending && second_pending,
            "canceled receive escaped its original due time"
        );
        assert!(
            second_not_written,
            "new nonzero delay failed to apply to its own admission"
        );
        assert_eq!((received_first, received_second), (0, 1));
        assert_eq!((written_first, written_second), (body(0), body(1)));
        assert_eq!(
            elapsed,
            Duration::from_millis(200),
            "changed policy retimed an original admission or re-delayed released backlog"
        );
    })
    .await
    .expect("retained delay admission failed its finite fixture budget");
}

/// Builds a mixed original body; only the last finite own movement may advance its witness.
fn movement_body() -> Bytes {
    use bedrock_protocol_1_26_51::{
        enums::PlayerPositionModeComponentPositionMode, types::ActorRuntimeID,
    };
    crate::session_transport::batch_body(&[
        crate::Packet::new(packets::MovePlayer {
            player_runtime_id: ActorRuntimeID(42),
            position: [1.0, 2.0, 3.0].into(),
            ..Default::default()
        }),
        crate::Packet::new(packets::MovePlayer {
            player_runtime_id: ActorRuntimeID(99),
            position: [90.0; 3].into(),
            ..Default::default()
        }),
        crate::Packet::new(packets::MovePlayer {
            player_runtime_id: ActorRuntimeID(42),
            position: [80.0; 3].into(),
            position_mode: PlayerPositionModeComponentPositionMode::Onlyheadrot,
            ..Default::default()
        }),
        crate::Packet::new(packets::PlayerAuthInput {
            position: [4.0, 5.0, 6.0].into(),
            ..Default::default()
        }),
        crate::Packet::new(packets::MovePlayer {
            player_runtime_id: ActorRuntimeID(42),
            position: [f32::NAN, 0.0, 0.0].into(),
            ..Default::default()
        }),
    ])
    .unwrap()
}

/// A canceled receipt borrow leaves the original write and opt-in witness owned by the same pump.
#[tokio::test(start_paused = true)]
async fn native_delay_position_requires_the_original_completed_write() {
    tokio::time::timeout(Duration::from_secs(2), async {
        let (mut transport, _incoming, mut outgoing) = pair();
        let delay = transport.packet_delay().unwrap();
        delay.bind(42);
        delay.configure(200, true).unwrap();
        let expected = movement_body();
        let sender = transport.sender();
        let mut written = sender.accept(expected.clone()).await.unwrap();
        let receipt_pending = {
            let mut borrow = std::pin::pin!(&mut written);
            std::future::poll_fn(|cx| Poll::Ready(borrow.as_mut().poll(cx)))
                .await
                .is_pending()
        };
        let before = delay.position();
        let early = outgoing.try_recv().ok();
        tokio::time::advance(Duration::from_millis(200)).await;
        let actual = outgoing.recv().await.unwrap();
        written.await.unwrap();
        let completed = delay.position();
        delay.configure(0, false).unwrap();
        delay.configure(200, true).unwrap();
        let reset = delay.position();
        let abandoned = sender.accept(expected.clone()).await.unwrap();
        drop(abandoned);
        let marker = sender.accept(body(9)).await.unwrap();
        tokio::time::advance(Duration::from_millis(200)).await;
        let repeated = outgoing.recv().await.unwrap();
        let last = outgoing.recv().await.unwrap();
        marker.await.unwrap();
        let after_abandoned = delay.position();
        transport.close().await.unwrap();
        let retired = delay.position();
        assert!(
            receipt_pending && early.is_none(),
            "the original write bypassed its due time"
        );
        assert_eq!(
            before, None,
            "queued movement was reported as physically written"
        );
        assert_eq!(actual, expected);
        assert_eq!(completed, Some([4.0, 5.0, 6.0]));
        assert_eq!(
            reset, None,
            "disable and reenable retained the old opt-in witness"
        );
        assert_eq!(
            repeated, expected,
            "abandoning the receipt lost or duplicated the actual write"
        );
        assert_eq!(last, body(9));
        assert_eq!(after_abandoned, Some([4.0, 5.0, 6.0]));
        assert_eq!(retired, None);
    })
    .await
    .expect("physical write observation failed its finite fixture budget");
}

/// Relay output retains the same native body FIFO and immutable delay through cancelled reads.
#[cfg(unix)]
#[tokio::test(start_paused = true)]
async fn native_relay_preserves_the_delayed_front_and_original_body_writes() {
    use crate::packet_relay::tests::FakeRelay;
    let (mut transport, incoming, mut outgoing) = pair();
    let relay = Arc::new(FakeRelay::default());
    relay
        .state()
        .inbound_replies
        .push_back(Some(body(7).to_vec()));
    relay
        .state()
        .outbound_replies
        .push_back(Some(body(8).to_vec()));
    transport.set_relay(relay.clone());
    transport.finish_startup();
    let delay = transport.packet_delay().unwrap();
    delay.bind(42);
    delay.configure(200, false).unwrap();
    incoming.send(body(1)).await.unwrap();
    std::future::poll_fn(|cx| transport.poll_read_ahead_one(MAX_READ_AHEAD_BYTES, cx)).await;
    {
        let mut receive = std::pin::pin!(transport.recv());
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        assert!(receive.as_mut().poll(&mut cx).is_pending());
    }
    assert!(relay.state().inbound.is_empty());
    tokio::time::advance(Duration::from_millis(200)).await;
    assert_eq!(time(transport.recv().await.unwrap()), 7);
    transport
        .send(&[crate::Packet::new(packets::SetTime {
            time: wire::ZigZag32(2),
        })])
        .await
        .unwrap();
    assert_eq!(outgoing.recv().await.unwrap(), body(8));
    assert_eq!(relay.state().inbound, [body(1).to_vec()]);
    assert_eq!(relay.state().outbound, [body(2).to_vec()]);
    transport.close().await.unwrap();
    drop(transport);
    assert!(relay.state().stopped);
}

/// A filtered body keeps its original finite admission and frame position until ordinary delivery.
#[cfg(unix)]
#[tokio::test]
async fn native_relay_drop_keeps_read_ahead_bounded_and_preserves_the_next_trace_position() {
    use crate::packet_relay::tests::FakeRelay;
    let (mut transport, incoming, _outgoing) = pair();
    let relay = Arc::new(FakeRelay::default());
    relay.state().inbound_replies.push_back(Some(Vec::new()));
    transport.set_relay(relay.clone());
    transport.finish_startup();
    let original = body(1);
    incoming.send(original.clone()).await.unwrap();
    let more = std::future::poll_fn(|cx| transport.poll_read_ahead_one(original.len(), cx)).await;
    let retained_bytes = transport.ahead_bytes;
    let retained_entries = transport.ahead.len();
    let callbacks_before_delivery = relay.state().inbound.len();
    incoming.send(body(2)).await.unwrap();
    let delivered = time(transport.recv().await.unwrap());
    let trace = transport.trace_position();
    let drained_bytes = transport.ahead_bytes;
    let callbacks = relay.state().inbound.clone();
    transport.close().await.unwrap();
    drop(transport);
    assert!(!more, "filtering removed the original admission bound");
    assert_eq!(retained_bytes, original.len());
    assert_eq!(retained_entries, 1);
    assert_eq!(callbacks_before_delivery, 1);
    assert_eq!(delivered, 2);
    assert_eq!(trace, (0, 2, 0));
    assert_eq!(drained_bytes, 0);
    assert_eq!(callbacks, [original.to_vec(), body(2).to_vec()]);
    assert!(relay.state().stopped);
}

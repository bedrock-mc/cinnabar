//! Actual prepared-pump reads and cursor ownership under a small caller-owned byte budget.

use super::*;
use crate::login::native_network::PacketConnection;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn byte_backpressure_stays_with_the_actual_body_cursor_and_allows_a_larger_body_alone() {
    let first = body(0..2);
    let larger = body(10..16);
    assert!(larger.len() > first.len());
    let (mut pump, mut incoming, reads, _source) =
        fixture(Some(first.len()), [first, larger, body(20..21)]).await;
    assert!(futures::poll!(&mut pump).is_pending());
    assert_eq!(
        reads.load(Ordering::SeqCst),
        2,
        "the actual pending body bypassed byte backpressure"
    );
    assert_eq!(time(incoming.recv().await.unwrap()), 0);
    assert!(futures::poll!(&mut pump).is_pending());
    assert_eq!(
        reads.load(Ordering::SeqCst),
        2,
        "dequeue released the body budget before its cursor was consumed"
    );
    assert_eq!(time(incoming.recv().await.unwrap()), 1);
    {
        let next = incoming.recv();
        tokio::pin!(next);
        assert!(futures::poll!(&mut next).is_pending());
        assert!(futures::poll!(&mut pump).is_pending());
        assert_eq!(
            reads.load(Ordering::SeqCst),
            2,
            "an oversized admitted cursor must prevent another payload read"
        );
        assert_eq!(
            time(next.await.unwrap()),
            10,
            "a valid larger body must occupy the budget alone"
        );
    }
    assert!(futures::poll!(&mut pump).is_pending());
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    for expected in 11..16 {
        assert_eq!(time(incoming.recv().await.unwrap()), expected);
    }
    {
        let next = incoming.recv();
        tokio::pin!(next);
        assert!(futures::poll!(&mut next).is_pending());
        assert!(futures::poll!(&mut pump).is_pending());
        assert_eq!(reads.load(Ordering::SeqCst), 3);
        assert_eq!(time(next.await.unwrap()), 20);
    }
    drop(incoming);
    pump.await.unwrap();
}

#[tokio::test]
async fn desktop_count_only_ingress_keeps_the_original_capacity() {
    let (mut pump, incoming, reads, _source) =
        fixture(None, [body(0..2), body(10..16), body(20..21)]).await;
    assert!(futures::poll!(&mut pump).is_pending());
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    drop(incoming);
    pump.await.unwrap();
}

#[tokio::test]
async fn an_emitted_shared_packet_keeps_byte_credit_after_its_cursor_is_exhausted() {
    let first = body(0..2);
    let (mut pump, mut incoming, reads, source) =
        fixture(Some(first.len()), [first, body(10..12), body(20..21)]).await;
    assert!(futures::poll!(&mut pump).is_pending());
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    let first = incoming.recv().await.unwrap();
    let CoreMessage::Batch(raw) = &first else {
        panic!("expected the original generated packet")
    };
    assert_eq!(raw.len(), 1);
    let retained = raw[0].clone();
    assert_eq!(time(first), 0);
    assert_eq!(time(incoming.recv().await.unwrap()), 1);

    let next_before_release = {
        let next = incoming.recv();
        tokio::pin!(next);
        // This actual receive exhausts the original cursor before waiting for its successor.
        assert!(futures::poll!(&mut next).is_pending());
        assert!(futures::poll!(&mut pump).is_pending());
        match futures::poll!(&mut next) {
            Poll::Ready(result) => Some(result.unwrap()),
            Poll::Pending => None,
        }
    };
    let released_before_last_view = next_before_release.is_some();
    let reads_while_retained = reads.load(Ordering::SeqCst);
    drop(retained);
    let next = match next_before_release {
        Some(next) => next,
        None => {
            assert!(futures::poll!(&mut pump).is_pending());
            incoming.recv().await.unwrap()
        }
    };
    assert_eq!(time(next), 10);
    drop(incoming);
    drop(source);
    pump.await.unwrap();
    drop(reads);

    assert!(
        !released_before_last_view,
        "the emitted shared packet released byte credit before its last view was dropped"
    );
    assert_eq!(
        reads_while_retained, 2,
        "cursor exhaustion admitted another payload while its emitted view was retained"
    );
}

/// Builds original generated packet bodies, not a parallel byte-accounting model.
fn body(times: std::ops::Range<i32>) -> Bytes {
    crate::session_transport::batch_body(
        &times
            .map(|time| {
                crate::Packet::new(packets::SetTime {
                    time: wire::ZigZag32(time),
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

/// Observes the original packet delivered by the actual NativeRelayInbound frame cursor.
fn time(message: CoreMessage) -> i32 {
    let CoreMessage::Batch(raw) = message else {
        panic!("expected actual SetTime prefix")
    };
    assert_eq!(raw.len(), 1);
    let mut reader = wire::Reader::from_shared(&raw[0]);
    let header = wire::PacketHeader::from_raw(reader.read_var_u32().unwrap());
    let packets::Packet::SetTime(packet) =
        packets::Packet::decode_body(packets::PacketId::from_raw(header.id).unwrap(), &mut reader)
            .unwrap()
    else {
        panic!("original SetTime")
    };
    packet.time.0
}

type Driver = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

type Fixture = (
    Driver,
    NativeRelayInbound,
    Arc<AtomicUsize>,
    mpsc::Sender<Bytes>,
);

/// Uses the real common driver and its original incoming FIFO with three finite payloads.
async fn fixture(byte_budget: Option<usize>, bodies: [Bytes; 3]) -> Fixture {
    let (source, input) = mpsc::channel(bodies.len());
    for body in bodies {
        source.send(body).await.unwrap();
    }
    let reads = Arc::new(AtomicUsize::new(0));
    let prepared = native::prepare_with_byte_budget(
        ObservedConnection {
            input,
            reads: Arc::clone(&reads),
        },
        byte_budget,
    );
    let (_sender, incoming, mut driver) = split(prepared, false);
    (
        Box::pin(async move { driver.drive().await }),
        incoming,
        reads,
        source,
    )
}

/// Observes only actual PacketConnection reads driven by the existing prepared pump.
struct ObservedConnection {
    input: mpsc::Receiver<Bytes>,
    reads: Arc<AtomicUsize>,
}

impl PacketConnection for ObservedConnection {
    async fn send_body(&mut self, _: Bytes) -> io::Result<()> {
        Ok(())
    }

    async fn recv_body(&mut self) -> io::Result<Bytes> {
        let body = self
            .input
            .recv()
            .await
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(body)
    }

    async fn close(&mut self) -> io::Result<()> {
        Ok(())
    }
}

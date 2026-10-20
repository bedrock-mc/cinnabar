//! Original credited input through the desktop reader, read-ahead, and borrowed packet bodies.

use super::*;
use crate::{
    packet_tap::{PacketBody, PacketDirection, PacketOutcome, PacketRecordId, PacketTap},
    session_transport::Inbound,
};
use bedrock_protocol_1_26_51::{packets, wire};

#[tokio::test]
async fn channel_byte_credit_survives_read_ahead_and_the_last_packet_borrow() {
    check_credit_lifetime(false).await;
}

#[tokio::test]
async fn joined_tail_byte_credit_survives_read_ahead_and_the_last_packet_borrow() {
    check_credit_lifetime(true).await;
}

/// Moves one actual credited body through every retained desktop receive owner.
async fn check_credit_lifetime(joined_tail: bool) {
    let body = crate::session_transport::batch_body(&[time_packet(7), time_packet(11)]).unwrap();
    let capacity = body.len();
    let allocation = body.as_ptr_range();
    let budget = ByteBudget::new(capacity);
    let item = IncomingBody {
        body: Ok(body),
        permit: Some(budget.reserve(capacity).await),
    };
    assert!(
        credit_is_blocked(&budget),
        "the input owns its actual credit"
    );
    let tap = Arc::new(SharedTap::default());
    let (mut transport, writes) = fixture(item, joined_tail, tap.clone()).await;

    let mut cx = Context::from_waker(futures::task::noop_waker_ref());
    assert_eq!(
        transport.poll_read_ahead_one(capacity, &mut cx),
        Poll::Ready(false)
    );
    let read_ahead_retained_credit = credit_is_blocked(&budget);

    let Inbound::Packet(first) = transport.recv().await.unwrap() else {
        panic!("expected the first original packet")
    };
    assert_eq!(time(&first), 7);
    drop(first);
    let active_batch_retained_credit = credit_is_blocked(&budget);

    let Inbound::Packet(second) = transport.recv().await.unwrap() else {
        panic!("expected the second original packet")
    };
    assert_eq!(time(&second), 11);
    drop(second);
    drop(tap.take_bodies());
    let current_packet_retained_credit = credit_is_blocked(&budget);

    // The real outcome callback borrows the current packet before that owner is released.
    transport.reject_current(PacketOutcome::Skipped);
    let mut observed = tap.take_bodies();
    assert_eq!(observed.len(), 1);
    let tapped = observed.pop().unwrap();
    let tap_retained_credit = credit_is_blocked(&budget);
    assert!(allocation.contains(&tapped.as_ptr()));
    let borrowed = tapped.slice(..);
    drop(tapped);
    drop(observed);

    transport.close().await.unwrap();
    drop(transport);
    drop(writes);
    drop(tap);
    let final_borrow_retained_credit = credit_is_blocked(&budget);
    drop(borrowed);
    let mut released = budget.reserve(capacity);
    let released_credit = match released.as_mut().poll(&mut cx) {
        Poll::Ready(credit) => Some(credit),
        Poll::Pending => None,
    };
    let final_drop_released_credit = released_credit.is_some();
    drop(released_credit);
    drop(released);
    drop(budget);

    assert!(
        read_ahead_retained_credit,
        "read-ahead released the credited body before consumption"
    );
    assert!(
        active_batch_retained_credit,
        "the active split batch released its original credit"
    );
    assert!(
        current_packet_retained_credit,
        "the current observed packet released its original credit"
    );
    assert!(
        tap_retained_credit,
        "the actual tap's shared body released its original credit"
    );
    assert!(
        final_borrow_retained_credit,
        "the last shared packet borrow released its original credit"
    );
    assert!(
        final_drop_released_credit,
        "the last borrow must return all credit"
    );
}

/// Polls an actual fair reservation once and cancels it without leaving an extra owner.
fn credit_is_blocked(budget: &ByteBudget) -> bool {
    let mut reservation = budget.reserve(1);
    let mut cx = Context::from_waker(futures::task::noop_waker_ref());
    match reservation.as_mut().poll(&mut cx) {
        Poll::Pending => true,
        Poll::Ready(credit) => {
            drop(credit);
            false
        }
    }
}

/// Uses the original reader fields and write queue without an unrelated connection or task.
async fn fixture(
    item: IncomingBody,
    joined_tail: bool,
    tap: Arc<SharedTap>,
) -> (SessionTransport, Writes) {
    let (incoming, receive) = mpsc::channel(1);
    let tail = if joined_tail {
        Some(item)
    } else {
        incoming.send(item).await.unwrap();
        None
    };
    drop(incoming);
    let (sender, writes) = write_channel(QUEUED_BATCHES);
    (
        SessionTransport {
            reader: Reader::Native(NativeReader {
                incoming: receive,
                rtt: None,
                task: None,
                tail,
                failure: None,
                stop: None,
                closed: false,
            }),
            timers: bridge::tasks::Timers::tokio(),
            writer: crate::packet_relay::BatchWriter::direct(PacketSender::Native(sender)),
            relay: None,
            prefer_injected: false,
            joining: true,
            startup: Default::default(),
            received: Default::default(),
            ahead: Default::default(),
            ahead_bytes: 0,
            ended: false,
            sending: Default::default(),
            tap: Some(tap),
            current: None,
            split_frames: 0,
            delivered: (0, 0),
        },
        writes,
    )
}

/// Creates generated entries rather than a second packet or byte-accounting model.
fn time_packet(time: i32) -> crate::Packet {
    crate::Packet::new(packets::SetTime {
        time: wire::ZigZag32(time),
    })
}

/// Decodes the actual returned raw packet while leaving its original owner in place.
fn time(packet: &crate::raw_packet::RawPacket) -> i32 {
    let (_, packets::Packet::SetTime(packet)) = packet.clone().decode_parts().unwrap() else {
        panic!("expected the original SetTime")
    };
    packet.time.0
}

/// Retains only original packet views exposed by the real observation callbacks.
#[derive(Default)]
struct SharedTap(Mutex<Vec<Bytes>>);

impl SharedTap {
    /// Transfers the actual observed views so the test can release each owner explicitly.
    fn take_bodies(&self) -> Vec<Bytes> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

impl PacketTap for SharedTap {
    fn record(&self, _: PacketDirection, _: u32, body: PacketBody<'_>) -> PacketRecordId {
        let mut bodies = self.0.lock().unwrap();
        let record = PacketRecordId(u64::try_from(bodies.len()).unwrap());
        bodies.push(body.to_shared());
        record
    }

    fn reject(&self, _: PacketRecordId, _: u32, _: PacketOutcome, body: PacketBody<'_>) {
        self.0.lock().unwrap().push(body.to_shared());
    }

    fn reject_batch(&self, _: usize) {
        panic!("generated packet batch must remain valid")
    }
}

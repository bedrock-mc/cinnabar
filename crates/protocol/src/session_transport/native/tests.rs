use super::*;
use std::future::Future;

mod byte_credit;
mod default_byte_admission;
mod delay;
mod public_close;

struct ReplayConnection {
    incoming: mpsc::Receiver<Bytes>,
    fifth_read: Option<oneshot::Sender<()>>,
    reads: usize,
    closed: Option<oneshot::Sender<()>>,
}

#[tokio::test]
async fn a_permit_committed_after_receive_error_cannot_strand_its_receipt() {
    use bedrock_protocol_1_26_51::packets;
    let (incoming, receive) = mpsc::channel(1);
    let mut transport = SessionTransport::from_native(ReplayConnection {
        incoming: receive,
        fifth_read: None,
        reads: 0,
        closed: None,
    });
    let PacketSender::Native(sender) = transport.sender() else {
        panic!("native fixture")
    };
    let permit = sender.send.reserve().await.unwrap();
    drop(incoming);
    let super::super::Reader::Native(reader) = &mut transport.reader else {
        panic!("native reader")
    };
    assert!(
        std::future::poll_fn(|cx| reader.poll_driver(cx))
            .await
            .unwrap()
            .is_none()
    );
    let body =
        crate::session_transport::batch_body(&[crate::Packet::new(packets::SetTime::default())])
            .unwrap();
    assert_eq!(
        sender.commit(permit, body).unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[tokio::test]
async fn a_permit_committed_after_failure_is_rejected_without_waiting_for_ingress() {
    use bedrock_protocol_1_26_51::packets;
    let (incoming, receive) = mpsc::channel(QUEUED_BATCHES + 1);
    let (fifth_read, received_fifth) = oneshot::channel();
    let transport = SessionTransport::from_native(ReplayConnection {
        incoming: receive,
        fifth_read: Some(fifth_read),
        reads: 0,
        closed: None,
    });
    let body =
        crate::session_transport::batch_body(&[crate::Packet::new(packets::SetTime::default())])
            .unwrap();
    for _ in 0..=QUEUED_BATCHES {
        incoming.send(body.clone()).await.unwrap();
    }
    received_fifth.await.unwrap();
    let PacketSender::Native(sender) = transport.sender() else {
        panic!("native fixture")
    };
    let permit = sender.send.reserve().await.unwrap();
    assert!(transport.sender().send(body.clone()).await.is_err());
    assert_eq!(
        sender.commit(permit, body).unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[tokio::test]
async fn aborting_the_connection_task_retires_its_uncommitted_permits() {
    let (_incoming, receive) = mpsc::channel(1);
    let mut transport = SessionTransport::from_native(ReplayConnection {
        incoming: receive,
        fifth_read: None,
        reads: 0,
        closed: None,
    });
    let PacketSender::Native(sender) = transport.sender() else {
        panic!("native fixture")
    };
    let permit = sender.send.reserve().await.unwrap();
    let super::super::Reader::Native(reader) = &mut transport.reader else {
        panic!("native reader")
    };
    reader.task.as_ref().unwrap().abort();
    assert!(
        std::future::poll_fn(|cx| reader.poll_driver(cx))
            .await
            .unwrap_err()
            .get_ref()
            .and_then(|error| error.downcast_ref::<tokio::task::JoinError>())
            .is_some_and(tokio::task::JoinError::is_cancelled)
    );
    assert_eq!(
        sender
            .commit(permit, Bytes::from_static(b"unsent"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
}

impl PacketConnection for ReplayConnection {
    async fn send_body(&mut self, _: Bytes) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "peer closed"))
    }

    async fn recv_body(&mut self) -> io::Result<Bytes> {
        let bytes = self
            .incoming
            .recv()
            .await
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "peer closed"))?;
        self.reads += 1;
        if self.reads == QUEUED_BATCHES + 1
            && let Some(read) = self.fifth_read.take()
        {
            let _ = read.send(());
        }
        Ok(bytes)
    }

    async fn close(&mut self) -> io::Result<()> {
        if let Some(closed) = self.closed.take() {
            let _ = closed.send(());
        }
        Ok(())
    }
}

#[tokio::test]
async fn write_failure_keeps_a_received_transfer_behind_full_ingress() {
    use bedrock_protocol_1_26_51::{packets, wire};
    let (incoming, receive) = mpsc::channel(QUEUED_BATCHES + 1);
    let (fifth_read, received_fifth) = oneshot::channel();
    let mut transport = SessionTransport::from_native(ReplayConnection {
        incoming: receive,
        fifth_read: Some(fifth_read),
        reads: 0,
        closed: None,
    });
    for _ in 0..QUEUED_BATCHES {
        incoming
            .send(
                crate::session_transport::batch_body(&[crate::Packet::new(
                    packets::SetTime::default(),
                )])
                .unwrap(),
            )
            .await
            .unwrap();
    }
    incoming
        .send(
            crate::session_transport::batch_body(&[crate::Packet::new(packets::Transfer {
                server_address: "target.example".into(),
                server_port: wire::U16LE(19132),
                ..Default::default()
            })])
            .unwrap(),
        )
        .await
        .unwrap();
    received_fifth.await.unwrap();
    let body =
        crate::session_transport::batch_body(&[crate::Packet::new(packets::SetTime::default())])
            .unwrap();
    assert!(transport.sender().send(body).await.is_err());
    for _ in 0..QUEUED_BATCHES {
        assert!(matches!(
            transport.recv().await.unwrap(),
            super::super::Inbound::Packet(_)
        ));
    }
    let super::super::Inbound::Packet(packet) = transport.recv().await.unwrap() else {
        panic!("expected original Transfer")
    };
    assert_eq!(packet.id, packets::PacketId::Transfer);
}

#[tokio::test]
async fn cancelling_admission_neither_accepts_nor_repeats_a_batch() {
    let (sender, mut writes) = write_channel(1);
    let receive = &mut writes.receive;
    let first = sender.accept(Bytes::from_static(b"first")).await.unwrap();
    {
        let mut blocked = std::pin::pin!(sender.accept(Bytes::from_static(b"cancelled")));
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        assert!(blocked.as_mut().poll(&mut cx).is_pending());
    }
    let accepted = receive.recv().await.unwrap();
    assert_eq!(accepted.body, b"first"[..]);
    accepted.written.send(Ok(())).unwrap();
    first.await.unwrap().unwrap();
    assert!(matches!(
        receive.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    let receipt = sender.accept(Bytes::from_static(b"next")).await.unwrap();
    drop(receipt);
    let accepted = receive.recv().await.unwrap();
    assert_eq!(accepted.body, b"next"[..]);
    assert!(accepted.written.send(Ok(())).is_err());
    assert!(matches!(
        receive.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn oversized_batches_never_consume_queue_capacity() {
    let (sender, mut writes) = write_channel(1);
    let receive = &mut writes.receive;
    let bytes = Bytes::from(vec![
        0;
        bedrock_net::BatchLimits::default().max_decoded_bytes
            + 1
    ]);
    assert_eq!(
        sender.accept(bytes).await.unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(matches!(
        receive.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    let _receipt = sender.accept(Bytes::from_static(b"fits")).await.unwrap();
    assert_eq!(receive.recv().await.unwrap().body, b"fits"[..]);
}

pub(crate) mod service;

#[tokio::test]
async fn graceful_close_keeps_the_actual_pending_fifth_ingress_body() {
    use bedrock_protocol_1_26_51::{packets, wire};
    let (incoming, receive) = mpsc::channel(QUEUED_BATCHES + 1);
    let (fifth_read, received_fifth) = oneshot::channel();
    let (closed, observed_close) = oneshot::channel();
    let mut transport = SessionTransport::from_native(ReplayConnection {
        incoming: receive,
        fifth_read: Some(fifth_read),
        reads: 0,
        closed: Some(closed),
    });
    for index in 0..=QUEUED_BATCHES {
        incoming
            .send(
                crate::session_transport::batch_body(&[crate::Packet::new(packets::SetTime {
                    time: wire::ZigZag32(i32::try_from(index).unwrap()),
                })])
                .unwrap(),
            )
            .await
            .unwrap();
    }
    received_fifth.await.unwrap();
    let super::super::Reader::Native(reader) = &mut transport.reader else {
        panic!("native reader")
    };
    reader
        .stop
        .take()
        .unwrap()
        .send(tokio::time::Instant::now() + CLOSE_TIMEOUT)
        .unwrap();
    observed_close.await.unwrap();
    for index in 0..=QUEUED_BATCHES {
        let item = transport
            .recv()
            .await
            .expect("graceful close lost accepted pending ingress");
        let super::super::Inbound::Packet(raw) = item else {
            panic!("expected original pending packet prefix")
        };
        let packet = raw.decode().unwrap();
        let packets::Packet::SetTime(actual) = &*packet.data else {
            panic!("expected original SetTime")
        };
        assert_eq!(actual.time.0, i32::try_from(index).unwrap());
    }
    transport.close().await.unwrap();
    drop(incoming);
}

mod prepared;
